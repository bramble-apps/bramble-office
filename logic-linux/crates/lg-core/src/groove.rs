//! Logic's extra Quantize values and groove templates.
//!
//! A region's Quantize value (`RegionProps::quantize`) is a grid in ticks, or one of these
//! encodings: swing values ("1/8 Swing A" … "1/16 Swing F": the grid with every second point
//! later by a fixed amount), tuplets ("5-Tuplet/4", "5-Tuplet/8", "7-Tuplet", "9-Tuplet": n
//! points per 4/4 bar, not a whole number of ticks) and groove templates (an index into the
//! project's templates). A groove template is made from regions (Make Groove Template): the
//! first note (or transient) near each position of its source regions; quantizing to it moves
//! notes to the nearest of those positions (Q-Strength), and Q-Velocity / Q-Length take the
//! template's velocities / lengths in by a percentage (negative values exaggerate the
//! difference). The template follows its source region: if that region is deleted it does
//! nothing.

use crate::events::Note;

pub const SWING: u32 = 0x1000_0000;
pub const TUPLET: u32 = 0x2000_0000;
pub const GROOVE: u32 = 0x4000_0000;

/// Logic's swing letters: A 50% … F 71%.
pub const SWING_LETTERS: [(char, u8); 6] = [('A', 50), ('B', 54), ('C', 58), ('D', 62), ('E', 66), ('F', 71)];

/// The extra Quantize values, as the Quantize pop-up lists them.
pub fn extra_values() -> Vec<(String, u32)> {
    let mut v = Vec::new();
    for (grid, name) in [(480u32, "1/8"), (240, "1/16")] {
        for (l, pct) in SWING_LETTERS {
            v.push((format!("{name} Swing {l}"), SWING | (pct as u32) << 16 | grid));
        }
    }
    for (name, n) in [("5-Tuplet/4", 5u32), ("5-Tuplet/8", 10), ("7-Tuplet", 7), ("9-Tuplet", 9)] {
        v.push((name.to_string(), TUPLET | n));
    }
    v
}

/// A Quantize value as a grid (ticks, maybe fractional) and swing (50 = straight); None for
/// Off and for groove templates.
pub fn grid(q: u32, swing: u8) -> Option<(f64, u8)> {
    if q == 0 || q & GROOVE != 0 {
        None
    } else if q & SWING != 0 {
        Some(((q & 0xFFFF) as f64, ((q >> 16) & 0xFF) as u8))
    } else if q & TUPLET != 0 {
        Some((3840.0 / (q & 0xFFFF).max(1) as f64, swing))
    } else {
        Some((q as f64, swing))
    }
}

pub fn groove_index(q: u32) -> Option<usize> {
    (q & GROOVE != 0).then_some((q & 0xFFFF) as usize)
}

/// A groove template: its name and its source region (track, start, name: the region must
/// stay in the project).
#[derive(Clone, Debug, PartialEq)]
pub struct Groove {
    pub name: String,
    pub track: usize,
    pub start: u32,
    pub region: String,
    /// An audio source's transients (as positions of velocity 100), kept when it was made.
    pub audio: Option<Points>,
}

/// A template's positions: (ticks from the source's start, velocity, length), and its period
/// (the source's length rounded up to whole bars).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Points {
    pub start: u32,
    pub period: u32,
    pub points: Vec<(u32, u8, u32)>,
}

/// Positions from notes (or transients as notes of velocity 100): only the first one near a
/// position counts (`near` ticks).
pub fn points_from(notes: &[Note], start: u32, length: u32, bar: u32, near: u32) -> Points {
    let mut n: Vec<&Note> = notes.iter().filter(|n| !n.muted && n.pos >= start && n.pos < start + length).collect();
    n.sort_by_key(|n| n.pos);
    let mut points: Vec<(u32, u8, u32)> = Vec::new();
    for x in n {
        let rel = x.pos - start;
        if points.last().is_some_and(|p| rel - p.0 < near) {
            continue;
        }
        points.push((rel, x.velocity, x.len));
    }
    let bar = bar.max(1);
    Points { start, period: length.div_ceil(bar).max(1) * bar, points }
}

/// Notes quantized to a template: each to the nearest template position (Q-Strength percent of
/// the way), velocities and lengths toward the template's by Q-Velocity / Q-Length percent.
pub fn apply(n: &Note, g: &Points, strength: u8, q_velocity: i16, q_length: i16) -> Note {
    let mut out = *n;
    if g.points.is_empty() || g.period == 0 {
        return out;
    }
    let rel = n.pos as i64 - g.start as i64;
    let per = g.period as i64;
    let cycle = rel.div_euclid(per);
    let within = rel.rem_euclid(per);
    // the nearest point, this period or the next one's first
    let mut best: Option<(i64, u8, u32)> = None;
    for &(p, v, l) in &g.points {
        for c in [-1i64, 0, 1] {
            let at = p as i64 + c * per;
            if best.is_none_or(|b| (at - within).abs() < (b.0 - within).abs()) {
                best = Some((at, v, l));
            }
        }
    }
    let Some((at, v, l)) = best else { return out };
    let target = g.start as i64 + cycle * per + at;
    let off = target - n.pos as i64;
    out.pos = (n.pos as i64 + (off as f64 * strength.min(100) as f64 / 100.0).round() as i64).max(0) as u32;
    let vel = n.velocity as f64 + (v as f64 - n.velocity as f64) * q_velocity as f64 / 100.0;
    out.velocity = vel.round().clamp(1.0, 127.0) as u8;
    let len = n.len as f64 + (l as f64 - n.len as f64) * q_length as f64 / 100.0;
    out.len = len.round().max(1.0) as u32;
    out
}

/// The project's templates' positions, looked up from their source regions (None for one whose
/// region is gone).
pub fn resolve(p: &crate::Project, k: usize) -> Option<Points> {
    let g = p.grooves.get(k)?;
    let rg = p.tracks.get(g.track)?.regions.iter().find(|r| r.start == g.start && r.name == g.region)?;
    if rg.audio.is_some() {
        // audio sources: their transients, found when the template was made
        return g.audio.clone();
    }
    let bar = crate::project::bar_ticks(p.signature);
    Some(points_from(&rg.notes, rg.start, rg.length, bar, 120))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(pos: u32, vel: u8, len: u32) -> Note {
        Note { channel: 0, pitch: 60, velocity: vel, pos, len, muted: false }
    }

    #[test]
    fn extra_values_decode() {
        let v = extra_values();
        let a = v.iter().find(|x| x.0 == "1/16 Swing F").unwrap().1;
        assert_eq!(grid(a, 50), Some((240.0, 71)));
        let t = v.iter().find(|x| x.0 == "7-Tuplet").unwrap().1;
        let (g, _) = grid(t, 50).unwrap();
        assert!((g - 3840.0 / 7.0).abs() < 1e-9);
        assert_eq!(grid(TUPLET | 10, 50).unwrap().0, 384.0);
        assert_eq!(grid(GROOVE | 2, 50), None);
        assert_eq!(groove_index(GROOVE | 2), Some(2));
        assert_eq!(grid(240, 60), Some((240.0, 60)));
    }

    #[test]
    fn template_from_notes_keeps_first_near() {
        let notes = vec![note(0, 100, 200), note(30, 50, 100), note(500, 70, 240), note(980, 90, 120)];
        let g = points_from(&notes, 0, 1920, 3840, 120);
        assert_eq!(g.points, vec![(0, 100, 200), (500, 70, 240), (980, 90, 120)]);
        assert_eq!(g.period, 3840);
    }

    #[test]
    fn notes_follow_the_template() {
        let g = Points { start: 0, period: 960, points: vec![(0, 100, 200), (500, 40, 100)] };
        let n = note(1920 + 470, 80, 240);
        let q = apply(&n, &g, 100, 0, 0);
        assert_eq!(q.pos, 1920 + 500);
        assert_eq!(q.velocity, 80);
        let q = apply(&n, &g, 50, 100, 100);
        assert_eq!(q.pos, 1920 + 485);
        assert_eq!((q.velocity, q.len), (40, 100));
        // negative: the difference exaggerated
        let q = apply(&n, &g, 100, -50, 0);
        assert_eq!(q.velocity, 100);
        // near the next period's first point
        let q = apply(&note(950, 80, 10), &g, 100, 0, 0);
        assert_eq!(q.pos, 960);
    }
}
