//! The Keyboard Player: chord voicings that lead smoothly from one chord to the next, a rhythm
//! per style (Strummed, Full Chords, Broken Chords, Arpeggiated, Supporting Pad), a left hand,
//! the complexity × intensity pad and the Details (Grace Notes, Feel, Dynamics, Tempo,
//! Humanize), the upper / lower hand positions and voicing styles of Logic's presets.
//!
//! Measured on Logic's Strummed piano ("Freely", intensity 71, 3/4): right-hand voicings of four
//! notes with an added colour tone (C: C D E G) around the upper position, the left hand on the
//! root in the octave below the lower position (C 48, D 50, A 45 for position 55), chords rolled
//! over about 30 ticks, re-struck in part on the off-beats, about ten notes a bar, the top voice
//! loudest. Our voicings and rhythms come from rules, not from Logic's figures.

use crate::chords::{chord_at, next_change, Chord, ChordAt};
use crate::drummer::{SwingType, TempoFeel};
use crate::{beats_of, vel, Note, RegionSpec, Rng};

/// The keyboard characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeysStyle {
    Strummed,
    FullChords,
    BrokenChords,
    Arpeggiated,
    SupportingPad,
}

impl KeysStyle {
    pub const ALL: [KeysStyle; 5] = [KeysStyle::Strummed, KeysStyle::FullChords, KeysStyle::BrokenChords, KeysStyle::Arpeggiated, KeysStyle::SupportingPad];

    pub fn id(self) -> &'static str {
        match self {
            KeysStyle::Strummed => "Acoustic Piano - Strummed",
            KeysStyle::FullChords => "Acoustic Piano - Full Chords",
            KeysStyle::BrokenChords => "Acoustic Piano - Broken Chords",
            KeysStyle::Arpeggiated => "Acoustic Piano - Arpeggiated",
            KeysStyle::SupportingPad => "Keyboard - Supporting Pad",
        }
    }

    pub fn find(id: &str) -> Option<KeysStyle> {
        let k = id.trim();
        KeysStyle::ALL.into_iter().find(|s| s.id().eq_ignore_ascii_case(k) || s.id().split(" - ").nth(1).is_some_and(|n| n.eq_ignore_ascii_case(k)))
    }
}

/// The left hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftHand {
    Off,
    /// the root, held
    Root,
    /// root and fifth
    RootFifth,
    /// root in octaves
    Octaves,
    /// a low triad
    Chord,
    /// root, then moving through the chord with the rhythm
    Moving,
}

/// A keyboard region's settings.
#[derive(Clone, Debug, PartialEq)]
pub struct KeysParams {
    pub style: KeysStyle,
    pub complexity: f32,
    pub intensity: f32,
    pub fills: f32,
    pub swing: f32,
    pub swing_type: SwingType,
    pub feel: f32,
    /// Grace Notes 0..1
    pub grace_notes: f32,
    pub dynamics: f32,
    pub humanize: f32,
    /// The right hand's centre (MIDI note; Logic's upperPosition, 57-82 in the presets)
    pub upper_position: u8,
    /// The left hand's top (Logic's lowerPosition, 41-78)
    pub lower_position: u8,
    /// Right-hand voicing style 1-8: 1 triad, 2 triad with the top doubled, 3 sevenths,
    /// 4 suspended colours, 5 added ninth (Logic's 5 measured as C D E G on C), 6 open (spread),
    /// 7 four-note with ninth, 8 shells
    pub upper_voicing: u8,
    pub left_hand: LeftHand,
    /// Arpeggio direction (Arpeggiated): 1 up, 2 down, 3 up-down, 4 down-up, 5 random, 6 as played
    pub arp_direction: u8,
    pub tempo: TempoFeel,
    pub seed: u64,
    pub channel: u8,
}

impl Default for KeysParams {
    fn default() -> Self {
        KeysParams {
            style: KeysStyle::Strummed,
            complexity: 0.42,
            intensity: 0.71,
            fills: 0.0,
            swing: 0.0,
            swing_type: SwingType::Eighths,
            feel: -0.16,
            grace_notes: 0.33,
            dynamics: 1.0,
            humanize: 0.37,
            upper_position: 62,
            lower_position: 55,
            upper_voicing: 5,
            left_hand: LeftHand::Root,
            arp_direction: 1,
            tempo: TempoFeel::Standard,
            seed: 1,
            channel: 0,
        }
    }
}

impl KeysParams {
    pub fn regenerated(&self) -> Self {
        let mut r = Rng::new(self.seed);
        KeysParams { seed: r.next_u64() >> 1, ..self.clone() }
    }
}

/// The pitch classes a voicing style adds to a chord (intervals above the root).
fn colour(chord: &Chord, style: u8) -> Vec<u8> {
    let third = chord.third();
    let fifth = chord.fifth();
    let seventh = chord.seventh();
    let sev = seventh.unwrap_or(if chord.is_minor() { 10 } else { 11 });
    let mut v: Vec<u8> = match style {
        1 | 2 => vec![0, third, fifth],
        3 => vec![0, third, fifth, sev],
        4 => vec![0, if chord.is_minor() { 5 } else { 2 }, fifth],
        5 => vec![0, 2, third, fifth],
        6 => vec![0, fifth, third + 12],
        7 => vec![0, third, sev, 14],
        _ => vec![third, sev],
    };
    if let Some(s) = seventh {
        if !v.contains(&s) && style != 1 {
            v.push(s);
        }
    }
    v
}

/// A voicing for a chord: pitches near `centre`, as close as possible to the previous voicing.
pub fn voice(chord: &Chord, style: u8, centre: u8, prev: Option<&[u8]>) -> Vec<u8> {
    let ints = colour(chord, style);
    let mut best: Option<(f32, Vec<i32>)> = None;
    // try every inversion at the octaves around the centre
    for rot in 0..ints.len() {
        for base_oct in [-24i32, -12, 0, 12] {
            let mut v: Vec<i32> = Vec::new();
            let mut last = i32::MIN;
            for k in 0..ints.len() {
                let iv = ints[(rot + k) % ints.len()] as i32;
                let mut p = (centre as i32 / 12) * 12 + base_oct + chord.root as i32 + iv;
                while p <= last {
                    p += 12;
                }
                v.push(p);
                last = p;
            }
            if style == 2 {
                v.push(v[0] + 12);
            }
            if v.iter().any(|p| !(21..=108).contains(p)) {
                continue;
            }
            let mean = v.iter().sum::<i32>() as f32 / v.len() as f32;
            let mut cost = (mean - centre as f32 - 3.0).abs() * 0.6 + (v[v.len() - 1] - v[0]) as f32 * 0.05;
            if let Some(pv) = prev {
                // voice leading: the movement of the outer voices and the nearest-note distance
                let moves: f32 = v.iter().map(|p| pv.iter().map(|q| (p - *q as i32).abs()).min().unwrap_or(0) as f32).sum();
                cost += moves * 0.35 + (v[v.len() - 1] - *pv.last().unwrap() as i32).abs() as f32 * 0.25;
            }
            if best.as_ref().is_none_or(|b| cost < b.0) {
                best = Some((cost, v.clone()));
            }
        }
    }
    best.map(|b| b.1.into_iter().map(|p| p as u8).collect()).unwrap_or_default()
}

/// The keyboard performance for a region over `chords`.
pub fn generate(params: &KeysParams, region: &RegionSpec, chords: &[ChordAt]) -> Vec<Note> {
    let bar_t = region.bar_ticks();
    let (_, beat_t) = beats_of(region.signature);
    let beat_t = match params.tempo {
        TempoFeel::Half => beat_t * 2,
        TempoFeel::Double => beat_t / 2,
        TempoFeel::Standard => beat_t,
    };
    let beats = (bar_t / beat_t).max(1);
    let c = params.complexity.clamp(0.0, 1.0);
    let fallback = [ChordAt { pos: region.start, chord: Chord::major(0) }];
    let chords = if chords.is_empty() { &fallback[..] } else { chords };
    let mut r = Rng::derive(params.seed, 21);
    let ch = params.channel;
    let mut notes: Vec<Note> = Vec::new();
    let mut prev: Option<Vec<u8>> = None;
    let half = beat_t / 2;
    // the bar's rhythm: onsets and how much of the chord each strikes (1 = all)
    let rhythm: Vec<(u32, f32)> = match params.style {
        // (strummed: made per bar below)
        KeysStyle::Strummed => vec![(0, 1.0)],
        KeysStyle::FullChords => {
            let step = if c > 0.5 { half } else { beat_t };
            let mut v: Vec<(u32, f32)> = (0..bar_t / step).map(|k| (k * step, if (k * step) % beat_t == 0 { 1.0 } else { 0.8 })).collect();
            if c > 0.75 {
                // a syncopated push: drop a beat, anticipate it
                if let Some(k) = v.iter().position(|x| x.0 == 2 * beat_t) {
                    v.remove(k);
                }
            }
            v
        }
        KeysStyle::BrokenChords | KeysStyle::Arpeggiated => {
            let step = if c > 0.55 { beat_t / 4 } else { half };
            (0..bar_t / step).map(|k| (k * step, 0.0)).collect()
        }
        KeysStyle::SupportingPad => vec![(0, 1.0)],
    };
    for b in 0..region.bars() {
        let t0 = region.start + b * bar_t;
        let (kind, _, _) = region.section_at(t0);
        let i = (params.intensity + kind.energy()).clamp(0.0, 1.0) * 100.0;
        let base_v = 40.0 + 0.55 * i;
        // onsets: the rhythm plus every chord change
        let mut onsets: Vec<(u32, f32)> = rhythm.clone();
        if params.style == KeysStyle::Strummed {
            // the full chord on the downbeat, parts of it struck again on the beats and, when
            // complex, the off-beats (about ten notes a bar at Logic's defaults)
            for k in 0..beats {
                if k > 0 && r.chance(0.5 + 0.3 * c) {
                    onsets.push((k * beat_t, 0.5));
                }
                if (k > 0 || c > 0.5) && r.chance((c - 0.1).max(0.0) * 0.8) {
                    onsets.push((k * beat_t + half, 0.35));
                }
            }
        }
        for chg in chords.iter().filter(|x| x.pos > t0 && x.pos < t0 + bar_t) {
            let at = chg.pos - t0;
            onsets.retain(|o| o.0.abs_diff(at) >= beat_t / 4);
            onsets.push((at, if params.style == KeysStyle::Arpeggiated || params.style == KeysStyle::BrokenChords { 0.0 } else { 1.0 }));
        }
        onsets.sort_by_key(|o| o.0);
        let mut voicing_at: Option<(Chord, Vec<u8>)> = None;
        let mut arp_k = 0usize;
        for (k, &(at, part)) in onsets.iter().enumerate() {
            let tick = t0 + at;
            if tick >= region.end() {
                break;
            }
            let chord = chord_at(chords, tick).unwrap_or(Chord::major(0));
            let voicing = match &voicing_at {
                Some((c0, v)) if *c0 == chord => v.clone(),
                _ => {
                    let v = voice(&chord, params.upper_voicing.clamp(1, 8), params.upper_position, prev.as_deref());
                    voicing_at = Some((chord, v.clone()));
                    prev = Some(v.clone());
                    arp_k = 0;
                    v
                }
            };
            let next_on = onsets.get(k + 1).map_or(t0 + bar_t, |o| t0 + o.0).min(region.end());
            let to_change = next_change(chords, tick).map_or(region.end(), |x| x.pos).min(region.end());
            let span = next_on - tick;
            let legato = match params.style {
                KeysStyle::SupportingPad => to_change - tick,
                KeysStyle::Strummed => (span as f32 * 0.92) as u32,
                _ => (span as f32 * (0.85 - 0.25 * c)) as u32,
            };
            let top = voicing.len().saturating_sub(1);
            match params.style {
                KeysStyle::BrokenChords | KeysStyle::Arpeggiated => {
                    let n = voicing.len().max(1);
                    let idx = arp_index(params.arp_direction, arp_k, n, &mut r, params.style == KeysStyle::BrokenChords);
                    arp_k += 1;
                    let p = voicing[idx];
                    let accent = if at % beat_t == 0 { 6.0 } else { 0.0 };
                    let len = if params.style == KeysStyle::Arpeggiated { (span * 2).min(to_change - tick) } else { legato.max(span) };
                    notes.push(Note { channel: ch, pitch: p, velocity: vel(base_v - 6.0 + accent + if idx == top { 6.0 } else { 0.0 }), pos: tick, len });
                }
                _ => {
                    // strike part of the chord (the top and some inner voices) on lighter hits
                    let count = ((voicing.len() as f32 * part).ceil() as usize).clamp(1, voicing.len());
                    let mut chosen: Vec<usize> = (0..voicing.len()).collect();
                    while chosen.len() > count {
                        let k = r.below(chosen.len().saturating_sub(1) as u32) as usize;
                        chosen.remove(k);
                    }
                    let roll = params.style == KeysStyle::Strummed;
                    for (j, &vi) in chosen.iter().enumerate() {
                        let off = if roll { (j as u32) * (6 + (c * 6.0) as u32) } else { 0 };
                        let accent = if vi == top { 12.0 } else { -4.0 } + if at % beat_t == 0 { 3.0 } else { -6.0 };
                        let pos = (tick + off).min(region.end() - 1);
                        notes.push(Note { channel: ch, pitch: voicing[vi], velocity: vel(base_v + accent), pos, len: legato.saturating_sub(off).max(30) });
                    }
                    // grace notes: a quick step into the top voice
                    if params.grace_notes > 0.0 && part >= 0.75 && tick >= region.start + 50 && r.chance(params.grace_notes * 0.35) {
                        let g = voicing[top].saturating_sub(if r.chance(0.5) { 1 } else { 2 });
                        notes.push(Note { channel: ch, pitch: g, velocity: vel(base_v - 16.0), pos: tick - 40, len: 40 });
                    }
                }
            }
            // left hand: on the chord's start and the bar's downbeat (moving: with the rhythm)
            let lh_hit = at == 0 || chords.iter().any(|x| x.pos == tick) || (params.left_hand == LeftHand::Moving && part >= 0.75) || (params.style == KeysStyle::Strummed && c > 0.3 && at == beats / 2 * beat_t + if beats % 2 == 1 { half } else { 0 });
            if lh_hit && params.left_hand != LeftHand::Off {
                let top_lh = params.lower_position.clamp(30, 90);
                let root = top_lh - 12 + (chord.bass_pc() + 12 - (top_lh - 12) % 12) % 12;
                let len = (to_change.min(next_on.max(tick + beat_t)) - tick).max(beat_t / 2).min(to_change - tick).max(30);
                let lv = vel(base_v - 2.0 + if at == 0 { 4.0 } else { -8.0 });
                let mut lh = vec![root];
                match params.left_hand {
                    LeftHand::RootFifth => lh.push(root + chord.fifth()),
                    LeftHand::Octaves => lh.push(root.saturating_sub(12).max(24)),
                    LeftHand::Chord => {
                        lh.push(root + chord.third());
                        lh.push(root + chord.fifth());
                    }
                    LeftHand::Moving if at != 0 => {
                        lh = vec![root + [chord.fifth(), 12, chord.third()][(at / beat_t) as usize % 3]];
                    }
                    _ => {}
                }
                for p in lh {
                    if !voicing.contains(&p) {
                        notes.push(Note { channel: ch, pitch: p, velocity: lv, pos: tick, len });
                    }
                }
            }
        }
        // a fill: a run up to the next chord's top voice at the end of a section
        let next = t0 + bar_t;
        if params.fills > 0.05 && next < region.end() && region.section_at(next).1 != region.section_at(t0).1 {
            let target = chord_at(chords, next).unwrap_or(Chord::major(0));
            let tv = voice(&target, params.upper_voicing.clamp(1, 8), params.upper_position, prev.as_deref());
            let run = (2.0 + params.fills * 4.0) as u32;
            let step = beat_t / 4;
            let top = *tv.last().unwrap_or(&72);
            for k in 0..run {
                let at = next - (run - k) * step;
                let pcs = target.pitch_classes();
                let mut p = top as i32 - ((run - k) as i32) * 2;
                while !pcs.contains(&((p.rem_euclid(12)) as u8)) && p > 30 {
                    p -= 1;
                }
                notes.push(Note { channel: ch, pitch: p.clamp(21, 108) as u8, velocity: vel(base_v - 8.0 + k as f32 * 2.0), pos: at, len: step });
            }
        }
    }
    crate::bass::humanize(notes, region, params.humanize * 0.6, params.feel, params.swing, params.swing_type, params.dynamics, params.seed ^ 0x55, beat_t)
}

fn arp_index(direction: u8, k: usize, n: usize, r: &mut Rng, broken: bool) -> usize {
    if broken {
        // broken chords: bottom, top, middle, top …
        return match k % 4 {
            0 => 0,
            1 | 3 => n - 1,
            _ => n / 2,
        };
    }
    let cycle = (2 * n).saturating_sub(2).max(1);
    match direction {
        2 => n - 1 - k % n,
        3 => {
            let j = k % cycle;
            if j < n { j } else { cycle - j }
        }
        4 => {
            let j = k % cycle;
            if j < n { n - 1 - j } else { j - n + 1 }
        }
        5 => r.below(n as u32) as usize,
        _ => k % n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chords::parse_bars;

    #[test]
    fn voicings_lead_smoothly() {
        let c = voice(&Chord::parse("C").unwrap(), 5, 62, None);
        assert!(c.iter().all(|p| (52..80).contains(p)), "{c:?}");
        let f = voice(&Chord::parse("F").unwrap(), 5, 62, Some(&c));
        let moved: i32 = c.iter().zip(&f).map(|(a, b)| (*a as i32 - *b as i32).abs()).sum();
        assert!(moved <= 12, "{c:?} -> {f:?}");
    }

    #[test]
    fn plays_the_chords() {
        let region = RegionSpec::new(0, 2880 * 4, (3, 4));
        let chords = parse_bars("C Am F G", 0, 2880);
        let n = generate(&KeysParams::default(), &region, &chords);
        for b in 0..4u32 {
            let ch = chords[b as usize].chord;
            let mut pcs = ch.pitch_classes();
            pcs.push((ch.root + 2) % 12); // the added ninth of voicing style 5
            let in_bar: Vec<_> = n.iter().filter(|x| x.pos >= b * 2880 + 60 && x.pos < (b + 1) * 2880 - 300 && x.len > 60).collect();
            assert!(in_bar.iter().filter(|x| !pcs.contains(&(x.pitch % 12))).count() * 5 <= in_bar.len() * 2, "bar {b}");
        }
    }
}
