//! The song's time: tempo changes with tempo curves, tempo sets, time and key signature
//! changes, and the tempo edits Logic offers (Tempo track points and curves, Tempo List,
//! Tempo Operations, beat mapping, Adjust Tempo using Region Length and Locators).
//!
//! - Tempo points are `Project::tempo_map` (tick, BPM), the first at 0; `Project::tempo_curves`
//!   holds the curve of the segment that starts at a point (-99..99, the automation curve
//!   shape; missing or 0: the tempo jumps at the next point). A curved segment glides from its
//!   point's tempo to the next point's.
//! - Time signature changes are kept by bar (0-based) so they stay on their bar when earlier
//!   bars change length; the first signature is `Project::signature`.

use crate::events::PPQ;
use crate::project::Project;

/// A tempo set: its own tempo changes (Logic: Tempo List ▸ Tempo Set pop-up).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TempoSet {
    pub name: String,
    pub map: Vec<(u32, f64)>,
    pub curves: Vec<(u32, i8)>,
}

/// The project's global extras (in `Project::global`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Global {
    /// Curved tempo segments: (the segment's first point's tick, curve).
    pub tempo_curves: Vec<(u32, i8)>,
    /// Time signature changes after the first: (bar, numerator, denominator).
    pub signatures: Vec<(u32, u8, u8)>,
    /// Key changes after the first: (bar, key name as `Project::key`).
    pub keys: Vec<(u32, String)>,
    /// Tempo sets (the active one's points are in `tempo_map`); empty: one unnamed set.
    pub tempo_sets: Vec<TempoSet>,
    pub tempo_set: usize,
    /// Beat mappings made (tick of the ruler position, seconds it maps to), for the Beat
    /// Mapping track's lines.
    pub beat_maps: Vec<(u32, f64)>,
    /// Chord track: (tick, length, chord name).
    pub chords: Vec<(u32, u32, String)>,
}

/// The tempo and signature map, ready to look things up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map {
    pub base: f64,
    pub points: Vec<(u32, f64)>,
    pub curves: Vec<(u32, i8)>,
    pub sig0: (u8, u8),
    /// (bar, numerator, denominator), sorted.
    pub sigs: Vec<(u32, u8, u8)>,
}

fn shape(t: f64, curve: i8) -> f64 {
    crate::automation::shape(t as f32, curve) as f64
}

impl Map {
    pub fn of(p: &Project) -> Map {
        let mut points: Vec<(u32, f64)> = p.tempo_map.clone();
        points.sort_by_key(|x| x.0);
        if points.first().is_none_or(|x| x.0 > 0) {
            points.insert(0, (0, if p.tempo > 0.0 { p.tempo } else { 120.0 }));
        }
        let mut sigs = p.global.signatures.clone();
        sigs.sort_by_key(|s| s.0);
        sigs.retain(|s| s.0 > 0);
        Map { base: p.tempo, points, curves: p.global.tempo_curves.clone(), sig0: if p.signature.0 == 0 { (4, 4) } else { p.signature }, sigs }
    }

    pub fn tempo(bpm: f64) -> Map {
        Map { base: bpm, points: vec![(0, bpm)], curves: Vec::new(), sig0: (4, 4), sigs: Vec::new() }
    }

    fn curve_at(&self, tick: u32) -> i8 {
        self.curves.iter().find(|c| c.0 == tick).map_or(0, |c| c.1)
    }

    /// The tempo at `tick`.
    pub fn bpm_at(&self, tick: f64) -> f64 {
        let k = self.points.iter().rposition(|p| p.0 as f64 <= tick).unwrap_or(0);
        let Some(&(t0, b0)) = self.points.get(k) else { return if self.base > 0.0 { self.base } else { 120.0 } };
        let c = self.curve_at(t0);
        match self.points.get(k + 1) {
            Some(&(t1, b1)) if c != 0 && t1 > t0 => b0 + (b1 - b0) * shape((tick - t0 as f64) / (t1 - t0) as f64, c),
            _ => b0,
        }
    }

    /// Seconds from bar 1 to `tick`.
    pub fn secs_at(&self, tick: f64) -> f64 {
        let mut s = 0.0;
        for (k, &(t0, b0)) in self.points.iter().enumerate() {
            if tick <= t0 as f64 {
                break;
            }
            let t1 = self.points.get(k + 1).map_or(f64::INFINITY, |p| p.0 as f64);
            let end = tick.min(t1);
            let c = self.curve_at(t0);
            if c == 0 || t1.is_infinite() {
                s += (end - t0 as f64) / (b0.max(1.0) / 60.0 * PPQ as f64);
            } else {
                // a curve: integrated in small steps
                let n = 64;
                let h = (end - t0 as f64) / n as f64;
                for i in 0..n {
                    let mid = t0 as f64 + h * (i as f64 + 0.5);
                    s += h / (self.bpm_at(mid).max(1.0) / 60.0 * PPQ as f64);
                }
            }
        }
        s + tick.min(0.0) / (self.bpm_at(0.0).max(1.0) / 60.0 * PPQ as f64)
    }

    /// The tick at `secs` from bar 1.
    pub fn tick_at(&self, secs: f64) -> f64 {
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        while self.secs_at(hi) < secs && hi < 1e12 {
            hi *= 2.0;
        }
        for _ in 0..80 {
            let mid = (lo + hi) / 2.0;
            if self.secs_at(mid) < secs { lo = mid } else { hi = mid }
        }
        (lo + hi) / 2.0
    }

    /// The signature in force at bar `bar` (0-based).
    pub fn sig_of_bar(&self, bar: u32) -> (u8, u8) {
        self.sigs.iter().rev().find(|s| s.0 <= bar).map_or(self.sig0, |s| (s.1, s.2))
    }

    pub fn bar_len(sig: (u8, u8)) -> u32 {
        PPQ * 4 / sig.1.max(1) as u32 * sig.0.max(1) as u32
    }

    pub fn beat_len(sig: (u8, u8)) -> u32 {
        PPQ * 4 / sig.1.max(1) as u32
    }

    /// Where bar `bar` (0-based) starts.
    pub fn bar_start(&self, bar: u32) -> u32 {
        let mut t = 0u32;
        let mut b = 0u32;
        let mut sig = self.sig0;
        for &(at, n, d) in &self.sigs {
            if at >= bar {
                break;
            }
            t += (at - b) * Self::bar_len(sig);
            b = at;
            sig = (n, d);
        }
        t + (bar - b) * Self::bar_len(sig)
    }

    /// The bar (0-based) `tick` is in, its start and its signature.
    pub fn bar_of(&self, tick: u32) -> (u32, u32, (u8, u8)) {
        let mut t = 0u32;
        let mut b = 0u32;
        let mut sig = self.sig0;
        for &(at, n, d) in &self.sigs {
            let start = t + (at - b) * Self::bar_len(sig);
            if start > tick {
                break;
            }
            t = start;
            b = at;
            sig = (n, d);
        }
        let len = Self::bar_len(sig).max(1);
        let k = (tick - t) / len;
        (b + k, t + k * len, sig)
    }

    /// The signature at `tick`.
    pub fn sig_at(&self, tick: u32) -> (u8, u8) {
        self.bar_of(tick).2
    }

    /// Bar, beat, division (of `div` ticks), tick, all 1-based, as the LCD shows them.
    pub fn bbdt(&self, tick: u32, div: u32) -> (u32, u32, u32, u32) {
        let (bar, start, sig) = self.bar_of(tick);
        let rel = tick - start;
        let beat = Self::beat_len(sig);
        let div = div.clamp(1, beat);
        (bar + 1, rel / beat + 1, rel % beat / div + 1, rel % beat % div + 1)
    }

    /// Bar lines from `from` to `to`: (tick, 1-based bar number, signature).
    pub fn bars(&self, from: u32, to: u32) -> Vec<(u32, u32, (u8, u8))> {
        let (mut bar, mut t, _) = self.bar_of(from);
        let mut out = Vec::new();
        while t <= to && out.len() < 100_000 {
            let sig = self.sig_of_bar(bar);
            out.push((t, bar + 1, sig));
            t += Self::bar_len(sig);
            bar += 1;
        }
        out
    }

    /// The tempo events a curve stands for, every `granularity` ticks (the Tempo List's
    /// Additional Info, Options ▸ Granularity).
    pub fn expanded(&self, granularity: u32) -> Vec<(u32, f64, bool)> {
        let mut out = Vec::new();
        for (k, &(t0, b0)) in self.points.iter().enumerate() {
            out.push((t0, b0, false));
            if let Some(&(t1, _)) = self.points.get(k + 1) {
                if self.curve_at(t0) != 0 {
                    let g = granularity.max(1);
                    let mut t = t0 + g;
                    while t < t1 {
                        out.push((t, self.bpm_at(t as f64), true));
                        t += g;
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------
// tempo edits (on the project, so they undo with it)

fn sort_dedup(p: &mut Project) {
    p.tempo_map.sort_by_key(|x| x.0);
    p.tempo_map.dedup_by_key(|x| x.0);
    let points: Vec<u32> = p.tempo_map.iter().map(|x| x.0).collect();
    p.global.tempo_curves.retain(|c| points.contains(&c.0) && c.1 != 0);
    if let Some(first) = p.tempo_map.first() {
        if first.0 == 0 {
            p.tempo = first.1;
        }
    }
}

fn ensure_first(p: &mut Project) {
    if p.tempo_map.first().is_none_or(|x| x.0 > 0) {
        let t = if p.tempo > 0.0 { p.tempo } else { 120.0 };
        p.tempo_map.insert(0, (0, t));
    }
}

/// Adds (or replaces) a tempo point.
pub fn set_point(p: &mut Project, tick: u32, bpm: f64) {
    ensure_first(p);
    let bpm = bpm.clamp(5.0, 990.0);
    match p.tempo_map.iter_mut().find(|x| x.0 == tick) {
        Some(x) => x.1 = bpm,
        None => p.tempo_map.push((tick, bpm)),
    }
    sort_dedup(p);
}

/// Deletes the point at `tick` (not the first).
pub fn delete_point(p: &mut Project, tick: u32) -> bool {
    if tick == 0 {
        return false;
    }
    let n = p.tempo_map.len();
    p.tempo_map.retain(|x| x.0 != tick);
    sort_dedup(p);
    p.tempo_map.len() != n
}

/// Moves a point in time; it replaces any point already there. Not the first.
pub fn move_point(p: &mut Project, from: u32, to: u32) -> bool {
    if from == 0 {
        return false;
    }
    let Some(k) = p.tempo_map.iter().position(|x| x.0 == from) else { return false };
    let bpm = p.tempo_map[k].1;
    let curve = p.global.tempo_curves.iter().find(|c| c.0 == from).map(|c| c.1);
    p.tempo_map.remove(k);
    p.global.tempo_curves.retain(|c| c.0 != from);
    p.tempo_map.retain(|x| x.0 != to);
    p.tempo_map.push((to.max(1), bpm));
    if let Some(c) = curve {
        p.global.tempo_curves.push((to.max(1), c));
    }
    sort_dedup(p);
    true
}

/// Copies a point (Option-drag, paste).
pub fn copy_point(p: &mut Project, from: u32, to: u32) {
    if let Some(bpm) = p.tempo_map.iter().find(|x| x.0 == from).map(|x| x.1) {
        set_point(p, to, bpm);
    }
}

/// Extend Left: the point moves to the previous point's position and replaces it.
pub fn extend_left(p: &mut Project, tick: u32) -> bool {
    let Some(k) = p.tempo_map.iter().position(|x| x.0 == tick) else { return false };
    if k == 0 {
        return false;
    }
    let prev = p.tempo_map[k - 1].0;
    let bpm = p.tempo_map[k].1;
    p.tempo_map.remove(k);
    p.tempo_map[k - 1].1 = bpm;
    let _ = prev;
    sort_dedup(p);
    true
}

/// Curves the segment starting at `tick` (0: straight, the tempo jumps).
pub fn set_curve(p: &mut Project, tick: u32, curve: i8) {
    p.global.tempo_curves.retain(|c| c.0 != tick);
    if curve != 0 {
        p.global.tempo_curves.push((tick, curve.clamp(-99, 99)));
    }
    sort_dedup(p);
}

/// Replace Selected Points by Average Tempo: one point at the first with the time-weighted
/// average up to the last, so the section's duration stays.
pub fn replace_by_average(p: &mut Project, from: u32, to: u32) {
    if to <= from {
        return;
    }
    let m = Map::of(p);
    let secs = m.secs_at(to as f64) - m.secs_at(from as f64);
    let after = m.bpm_at(to as f64);
    let avg = (to - from) as f64 / PPQ as f64 / secs.max(1e-9) * 60.0;
    p.tempo_map.retain(|x| x.0 < from || x.0 > to);
    p.global.tempo_curves.retain(|c| c.0 < from || c.0 > to);
    set_point(p, from, avg);
    if !p.tempo_map.iter().any(|x| x.0 > to) || m.points.iter().any(|x| x.0 == to) {
        set_point(p, to, after);
    }
}

/// Apply Smoothing to Selected Points: each point moves halfway to its neighbours' mean
/// (duration and average tempo kept by scaling afterwards).
pub fn smooth(p: &mut Project, from: u32, to: u32) {
    let m = Map::of(p);
    let secs = m.secs_at(to as f64) - m.secs_at(from as f64);
    let idx: Vec<usize> = (0..p.tempo_map.len()).filter(|&k| p.tempo_map[k].0 >= from && p.tempo_map[k].0 < to).collect();
    if idx.len() < 2 {
        return;
    }
    let orig: Vec<f64> = p.tempo_map.iter().map(|x| x.1).collect();
    for &k in &idx {
        let a = orig[k.saturating_sub(1)];
        let b = orig.get(k + 1).copied().unwrap_or(orig[k]);
        p.tempo_map[k].1 = orig[k] * 0.5 + (a + b) * 0.25;
    }
    // keep the duration
    let m2 = Map::of(p);
    let secs2 = m2.secs_at(to as f64) - m2.secs_at(from as f64);
    let f = secs2 / secs.max(1e-9);
    for &k in &idx {
        p.tempo_map[k].1 *= f;
    }
    sort_dedup(p);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveType {
    /// Even steps.
    Linear,
    /// Curvature bends it (positive: starts slow, speeds up late).
    Curved,
    /// Slow at both ends.
    SCurve,
}

/// Tempo Operations: the operations, applied between `from` and `to` (ticks).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Operation {
    /// Create Tempo Curve: start / end tempo, curve type and curvature (-99..99), density
    /// (events per bar: 1..32), continue with the new tempo afterwards.
    Curve { start: f64, end: f64, kind: CurveType, curvature: i8, density: u32, keep: bool },
    /// Create Constant Tempo: the right position keeps its time (the default tempo is the
    /// section's average); with `keep_time` false the given tempo is used as is.
    Constant { bpm: Option<f64> },
    /// Scale Existing Tempo Changes by a percentage (+: faster).
    Scale { percent: f64 },
    /// Stretch Existing Tempo Changes: the points between are spread to a new end.
    Stretch { new_end: u32 },
    /// Thin Out Existing Tempo Changes: at most `density` events per bar.
    ThinOut { density: u32 },
    /// Randomize Tempo: up to ±`bpm`, `density` events per bar.
    Randomize { bpm: f64, density: u32, seed: u64 },
    /// Round Existing Tempo Changes: to whole bpm (`tenths` false) or 1/10 bpm.
    Round { tenths: bool },
}

/// Runs a Tempo Operations operation.
pub fn operate(p: &mut Project, from: u32, to: u32, op: Operation) {
    ensure_first(p);
    if to <= from {
        return;
    }
    let m = Map::of(p);
    let bar = Map::bar_len(m.sig_at(from)).max(1);
    match op {
        Operation::Curve { start, end, kind, curvature, density, keep } => {
            let after = m.bpm_at(to as f64);
            p.tempo_map.retain(|x| x.0 < from || x.0 >= to);
            p.global.tempo_curves.retain(|c| c.0 < from || c.0 >= to);
            let step = (bar / density.clamp(1, 32)).max(1);
            let n = ((to - from) / step).max(1);
            for i in 0..n {
                let t = i as f64 / n as f64;
                let s = match kind {
                    CurveType::Linear => t,
                    CurveType::Curved => shape(t, curvature),
                    CurveType::SCurve => 0.5 - 0.5 * (t * std::f64::consts::PI).cos(),
                };
                set_point(p, from + i * step, start + (end - start) * s);
            }
            set_point(p, to, if keep { end } else { after });
            if keep {
                // continue with the new tempo: later changes are dropped up to the next one
            }
        }
        Operation::Constant { bpm } => {
            let secs = m.secs_at(to as f64) - m.secs_at(from as f64);
            let avg = (to - from) as f64 / PPQ as f64 / secs.max(1e-9) * 60.0;
            let after = m.bpm_at(to as f64);
            p.tempo_map.retain(|x| x.0 < from || x.0 > to);
            p.global.tempo_curves.retain(|c| c.0 < from || c.0 >= to);
            set_point(p, from, bpm.unwrap_or(avg));
            set_point(p, to, after);
        }
        Operation::Scale { percent } => {
            for x in p.tempo_map.iter_mut().filter(|x| x.0 >= from && x.0 < to) {
                x.1 = (x.1 * (1.0 + percent / 100.0)).clamp(5.0, 990.0);
            }
        }
        Operation::Stretch { new_end } => {
            let f = (new_end.max(from + 1) - from) as f64 / (to - from) as f64;
            let pts: Vec<(u32, f64)> = p.tempo_map.iter().copied().filter(|x| x.0 > from && x.0 <= to).collect();
            let curves = p.global.tempo_curves.clone();
            p.tempo_map.retain(|x| x.0 <= from || x.0 > to);
            p.global.tempo_curves.retain(|c| c.0 <= from || c.0 > to);
            for (t, b) in pts {
                let nt = from + ((t - from) as f64 * f).round() as u32;
                p.tempo_map.retain(|x| x.0 != nt);
                p.tempo_map.push((nt, b));
                if let Some(c) = curves.iter().find(|c| c.0 == t) {
                    p.global.tempo_curves.push((nt, c.1));
                }
            }
        }
        Operation::ThinOut { density } => {
            let step = (bar / density.clamp(1, 32)).max(1);
            let mut last: Option<u32> = None;
            let keep: Vec<(u32, f64)> = p.tempo_map.iter().copied().filter(|x| {
                if x.0 < from || x.0 >= to {
                    return true;
                }
                if last.is_some_and(|l| x.0 - l < step) {
                    return false;
                }
                last = Some(x.0);
                true
            }).collect();
            p.tempo_map = keep;
        }
        Operation::Randomize { bpm, density, seed } => {
            let step = (bar / density.clamp(1, 32)).max(1);
            let mut s = seed.max(1);
            let mut t = from;
            let after = m.bpm_at(to as f64);
            while t < to {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                let r = (s % 20001) as f64 / 10000.0 - 1.0;
                set_point(p, t, m.bpm_at(t as f64) + r * bpm);
                t += step;
            }
            set_point(p, to, after);
        }
        Operation::Round { tenths } => {
            for x in p.tempo_map.iter_mut().filter(|x| x.0 >= from && x.0 < to) {
                x.1 = if tenths { (x.1 * 10.0).round() / 10.0 } else { x.1.round() };
            }
        }
    }
    sort_dedup(p);
}

/// Beat mapping: the ruler position `tick` is made to sound at `secs` (where a played note
/// is), by a tempo change at the previous mapping (or point) before it; the absolute time of
/// everything before stays. Returns false when the tempo would leave 5..990 bpm.
pub fn beat_map(p: &mut Project, tick: u32, secs: f64) -> bool {
    ensure_first(p);
    let m = Map::of(p);
    // the anchor: the last point (or mapping) before `tick`
    let anchor = p.tempo_map.iter().map(|x| x.0).filter(|&t| t < tick).max().unwrap_or(0);
    let s0 = m.secs_at(anchor as f64);
    if secs <= s0 {
        return false;
    }
    let bpm = (tick - anchor) as f64 / PPQ as f64 / (secs - s0) * 60.0;
    if !(5.0..=990.0).contains(&bpm) {
        return false;
    }
    // what follows keeps its tempo from `tick` on
    let next = m.bpm_at(tick as f64);
    p.global.tempo_curves.retain(|c| c.0 != anchor);
    p.tempo_map.retain(|x| x.0 <= anchor || x.0 > tick);
    set_point(p, anchor, bpm);
    if !p.tempo_map.iter().any(|x| x.0 == tick) {
        set_point(p, tick, next);
    }
    p.global.beat_maps.retain(|b| b.0 != tick);
    p.global.beat_maps.push((tick, secs));
    p.global.beat_maps.sort_by_key(|b| b.0);
    true
}

/// Adjust Tempo using Region Length and Locators: the tempo that makes a region of `secs`
/// seconds last from `from` to `to`; `globally` sets the project tempo, otherwise a change at
/// `from` and the old tempo back at `to`.
pub fn fit_region(p: &mut Project, from: u32, to: u32, secs: f64, globally: bool) {
    if to <= from || secs <= 0.0 {
        return;
    }
    let bpm = (to - from) as f64 / PPQ as f64 / secs * 60.0;
    if globally {
        p.tempo_map.clear();
        p.global.tempo_curves.clear();
        p.tempo = bpm.clamp(5.0, 990.0);
        p.tempo_map.push((0, p.tempo));
    } else {
        let m = Map::of(p);
        let after = m.bpm_at(to as f64);
        p.tempo_map.retain(|x| x.0 <= from || x.0 > to);
        set_point(p, from, bpm);
        set_point(p, to, after);
    }
}

/// Tempo sets: the active points stored, another made current.
pub fn switch_set(p: &mut Project, k: usize) {
    ensure_sets(p);
    if k >= p.global.tempo_sets.len() || k == p.global.tempo_set {
        return;
    }
    let cur = p.global.tempo_set;
    p.global.tempo_sets[cur].map = p.tempo_map.clone();
    p.global.tempo_sets[cur].curves = p.global.tempo_curves.clone();
    // the initial tempo stays in every set
    let first = p.tempo_map.first().map_or(p.tempo, |x| x.1);
    let s = p.global.tempo_sets[k].clone();
    p.tempo_map = if s.map.is_empty() { vec![(0, first)] } else { s.map };
    if let Some(x) = p.tempo_map.first_mut().filter(|x| x.0 == 0) {
        x.1 = first;
    }
    p.global.tempo_curves = s.curves;
    p.global.tempo_set = k;
    sort_dedup(p);
}

fn ensure_sets(p: &mut Project) {
    if p.global.tempo_sets.is_empty() {
        p.global.tempo_sets.push(TempoSet { name: "Tempo Set 1".into(), map: p.tempo_map.clone(), curves: p.global.tempo_curves.clone() });
        p.global.tempo_set = 0;
    }
}

/// New Set: an empty set (the initial tempo only) made current.
pub fn new_set(p: &mut Project, name: &str) -> usize {
    ensure_sets(p);
    p.global.tempo_sets.push(TempoSet { name: name.to_string(), map: Vec::new(), curves: Vec::new() });
    let k = p.global.tempo_sets.len() - 1;
    switch_set(p, k);
    k
}

pub fn rename_set(p: &mut Project, k: usize, name: &str) {
    ensure_sets(p);
    if let Some(s) = p.global.tempo_sets.get_mut(k) {
        s.name = name.to_string();
    }
}

/// Delete Set: the current set goes, the one before becomes current.
pub fn delete_set(p: &mut Project) {
    ensure_sets(p);
    if p.global.tempo_sets.len() < 2 {
        return;
    }
    let k = p.global.tempo_set;
    let to = if k > 0 { k - 1 } else { 1 };
    switch_set(p, to);
    p.global.tempo_sets.remove(k);
    if p.global.tempo_set > k {
        p.global.tempo_set -= 1;
    }
}

pub fn set_names(p: &Project) -> Vec<String> {
    if p.global.tempo_sets.is_empty() { vec!["Tempo Set 1".into()] } else { p.global.tempo_sets.iter().map(|s| s.name.clone()).collect() }
}


/// After a tempo change made by beat mapping: every region and note keeps its time in
/// seconds (its ticks follow), `old` being the map before.
pub fn retime(p: &mut Project, old: &Map) {
    let new = Map::of(p);
    let conv = |t: u32| new.tick_at(old.secs_at(t as f64)).round().max(0.0) as u32;
    for tr in &mut p.tracks {
        for r in &mut tr.regions {
            let s = conv(r.start);
            let e = conv(r.start + r.length);
            for n in &mut r.notes {
                let a = conv(n.pos);
                let b = conv(n.pos + n.len);
                n.pos = a;
                n.len = b.saturating_sub(a).max(1);
            }
            if r.props.loop_to > 0 {
                r.props.loop_to = conv(r.props.loop_to);
            }
            r.start = s;
            r.length = e.saturating_sub(s).max(1);
        }
    }
}

/// Beats from Region (automatic beat mapping): the guide notes at `ticks` (sorted) are mapped
/// to successive beats of `step` ticks, the first to its nearest; everything keeps its time.
/// Returns how many were mapped (stops when a tempo would leave 5..990 bpm).
pub fn beats_from_notes(p: &mut Project, ticks: &[u32], step: u32) -> usize {
    let old = Map::of(p);
    let step = step.max(1);
    let Some(&first) = ticks.first() else { return 0 };
    let base = ((first as f64 / step as f64).round() as u32) * step;
    let mut n = 0;
    let mut last_target = None;
    for (i, &t) in ticks.iter().enumerate() {
        let target = base + i as u32 * step;
        if target == 0 || last_target == Some(target) {
            continue;
        }
        if !beat_map(p, target, old.secs_at(t as f64)) {
            break;
        }
        last_target = Some(target);
        n += 1;
    }
    retime(p, &old);
    n
}

// ---------------------------------------------------------------------------------------------
// signatures

/// Adds or replaces a time signature at `bar` (0-based; bar 0 changes the project's).
pub fn set_signature(p: &mut Project, bar: u32, sig: (u8, u8)) {
    let sig = (sig.0.clamp(1, 99), [1u8, 2, 4, 8, 16, 32, 64].into_iter().min_by_key(|d| (*d as i32 - sig.1 as i32).abs()).unwrap_or(4));
    if bar == 0 {
        p.signature = sig;
        return;
    }
    p.global.signatures.retain(|s| s.0 != bar);
    p.global.signatures.push((bar, sig.0, sig.1));
    p.global.signatures.sort_by_key(|s| s.0);
}

/// Deletes the signature at `bar` (the initial one can't be deleted).
pub fn delete_signature(p: &mut Project, bar: u32) -> bool {
    let n = p.global.signatures.len();
    p.global.signatures.retain(|s| s.0 != bar || bar == 0);
    p.global.signatures.len() != n
}

pub fn move_signature(p: &mut Project, from: u32, to: u32) -> bool {
    if from == 0 || to == 0 {
        return false;
    }
    let Some(s) = p.global.signatures.iter().find(|s| s.0 == from).copied() else { return false };
    p.global.signatures.retain(|x| x.0 != from && x.0 != to);
    p.global.signatures.push((to, s.1, s.2));
    p.global.signatures.sort_by_key(|s| s.0);
    true
}

/// The key at `bar`.
pub fn key_at(p: &Project, bar: u32) -> String {
    p.global.keys.iter().rev().find(|k| k.0 <= bar).map_or(p.key.clone(), |k| k.1.clone())
}

pub fn set_key(p: &mut Project, bar: u32, key: &str) {
    if bar == 0 {
        p.key = key.to_string();
        return;
    }
    p.global.keys.retain(|k| k.0 != bar);
    p.global.keys.push((bar, key.to_string()));
    p.global.keys.sort_by_key(|k| k.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proj() -> Project {
        Project { tempo: 120.0, signature: (4, 4), tempo_map: vec![(0, 120.0)], ..Default::default() }
    }

    #[test]
    fn tempo_steps_and_curves() {
        let mut p = proj();
        set_point(&mut p, 3840, 60.0);
        let m = Map::of(&p);
        assert_eq!(m.bpm_at(100.0), 120.0);
        assert_eq!(m.bpm_at(3840.0), 60.0);
        // bar 1 at 120 = 2 s, then a bar at 60 = 4 s
        assert!((m.secs_at(3840.0) - 2.0).abs() < 1e-9);
        assert!((m.secs_at(7680.0) - 6.0).abs() < 1e-9);
        assert!((m.tick_at(6.0) - 7680.0).abs() < 0.01);
        // a curve glides between the two
        set_curve(&mut p, 0, 0);
        p.global.tempo_curves.push((0, 1));
        let m = Map::of(&p);
        let mid = m.bpm_at(1920.0);
        assert!(mid < 120.0 && mid > 60.0, "{mid}");
        assert!(m.secs_at(3840.0) > 2.0);
        assert!(m.expanded(240).len() > 10);
        // deleting the point removes its curve's end; the first can't go
        assert!(!delete_point(&mut p, 0));
        assert!(delete_point(&mut p, 3840));
        assert!(p.global.tempo_curves.is_empty() || Map::of(&p).bpm_at(5000.0) == 120.0);
    }

    #[test]
    fn signatures_change_the_bars() {
        let mut p = proj();
        set_signature(&mut p, 2, (3, 4));
        set_signature(&mut p, 4, (7, 8));
        let m = Map::of(&p);
        assert_eq!(m.bar_start(2), 7680);
        assert_eq!(m.bar_start(3), 7680 + 2880);
        assert_eq!(m.bar_start(5), 7680 + 2 * 2880 + 3360);
        assert_eq!(m.bar_of(7680 + 2880 + 10), (3, 7680 + 2880, (3, 4)));
        assert_eq!(m.bbdt(7680 + 960 + 240, 240), (3, 2, 2, 1));
        let bars = m.bars(0, 20000);
        assert_eq!(bars[3].0, 10560);
        assert_eq!(bars[4].2, (7, 8));
        // a signature stays on its bar when an earlier one changes
        set_signature(&mut p, 0, (2, 4));
        assert_eq!(Map::of(&p).bar_start(2), 3840);
        assert!(move_signature(&mut p, 4, 6));
        assert!(!delete_signature(&mut p, 0));
        assert!(delete_signature(&mut p, 6));
        assert_eq!(p.global.signatures, vec![(2, 3, 4)]);
    }

    #[test]
    fn tempo_operations() {
        let mut p = proj();
        operate(&mut p, 0, 7680, Operation::Curve { start: 100.0, end: 140.0, kind: CurveType::Linear, curvature: 0, density: 4, keep: true });
        let m = Map::of(&p);
        assert_eq!(m.bpm_at(0.0), 100.0);
        assert!(m.bpm_at(3840.0) > 115.0 && m.bpm_at(3840.0) < 125.0);
        assert_eq!(m.bpm_at(8000.0), 140.0);
        // a constant tempo keeps the right position's time
        let before = Map::of(&p).secs_at(7680.0);
        operate(&mut p, 0, 7680, Operation::Constant { bpm: None });
        let m = Map::of(&p);
        assert!((m.secs_at(7680.0) - before).abs() < 1e-6);
        assert!(p.tempo_map.iter().filter(|x| x.0 < 7680).count() == 1);
        operate(&mut p, 0, 7680, Operation::Scale { percent: 10.0 });
        operate(&mut p, 0, 7680, Operation::Round { tenths: false });
        assert_eq!(p.tempo_map[0].1.fract(), 0.0);
        let mut q = proj();
        operate(&mut q, 0, 7680, Operation::Randomize { bpm: 10.0, density: 4, seed: 3 });
        assert!(q.tempo_map.len() > 5);
        operate(&mut q, 0, 7680, Operation::ThinOut { density: 1 });
        assert!(q.tempo_map.iter().filter(|x| x.0 < 7680).count() <= 2);
    }

    #[test]
    fn beat_mapping_moves_a_beat_to_a_time() {
        let mut p = proj();
        // bar 2 (tick 3840) should land at 2.5 s instead of 2.0
        assert!(beat_map(&mut p, 3840, 2.5));
        let m = Map::of(&p);
        assert!((m.secs_at(3840.0) - 2.5).abs() < 1e-9);
        assert_eq!(m.bpm_at(5000.0), 120.0);
        // the next mapping keeps the first in place
        assert!(beat_map(&mut p, 7680, 4.0));
        let m = Map::of(&p);
        assert!((m.secs_at(3840.0) - 2.5).abs() < 1e-9);
        assert!((m.secs_at(7680.0) - 4.0).abs() < 1e-9);
        assert!(!beat_map(&mut p, 9000, 3.0));
    }

    #[test]
    fn beats_from_a_freely_played_region() {
        let mut p = proj();
        let mut t = crate::Track::new("K", crate::TrackKind::Instrument);
        // played a little slower than the grid: every quarter 1100 ticks apart
        let notes: Vec<crate::events::Note> = (0..4).map(|i| crate::events::Note { channel: 0, pitch: 60, velocity: 90, pos: 960 + i * 1100, len: 100, muted: false }).collect();
        let ticks: Vec<u32> = notes.iter().map(|n| n.pos).collect();
        t.regions.push(crate::Region { name: "g".into(), start: 0, length: 7680, notes, audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
        p.tracks.push(t);
        let secs: Vec<f64> = ticks.iter().map(|&x| Map::of(&p).secs_at(x as f64)).collect();
        assert_eq!(beats_from_notes(&mut p, &ticks, 960), 4);
        let m = Map::of(&p);
        let now: Vec<u32> = p.tracks[0].regions[0].notes.iter().map(|n| n.pos).collect();
        // the notes are on the beats now, at the same times
        assert_eq!(now, vec![960, 1920, 2880, 3840]);
        for (k, &x) in now.iter().enumerate() {
            assert!((m.secs_at(x as f64) - secs[k]).abs() < 0.002);
        }
    }

    #[test]
    fn tempo_sets_and_average() {
        let mut p = proj();
        set_point(&mut p, 3840, 90.0);
        let k = new_set(&mut p, "Fast");
        assert_eq!(k, 1);
        assert_eq!(p.tempo_map, vec![(0, 120.0)]);
        set_point(&mut p, 7680, 150.0);
        switch_set(&mut p, 0);
        assert_eq!(p.tempo_map, vec![(0, 120.0), (3840, 90.0)]);
        switch_set(&mut p, 1);
        assert_eq!(p.tempo_map.len(), 2);
        delete_set(&mut p);
        assert_eq!(set_names(&p), vec!["Tempo Set 1".to_string()]);
        // average keeps the duration
        let mut p = proj();
        set_point(&mut p, 3840, 60.0);
        let before = Map::of(&p).secs_at(7680.0);
        replace_by_average(&mut p, 0, 7680);
        assert!((Map::of(&p).secs_at(7680.0) - before).abs() < 1e-6);
        fit_region(&mut p, 0, 7680, 4.0, true);
        assert_eq!(p.tempo, 120.0);
    }
}
