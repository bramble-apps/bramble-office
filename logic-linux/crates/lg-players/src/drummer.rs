//! The Drummer: our own groove generator, driven by Logic's Session Player controls.
//!
//! How Logic's drummer responds (measured on its Pop Rock regions, `measured`):
//! - Intensity (the XY pad's soft↔loud axis) sets the velocities and nothing else, except that
//!   played soft the backbeat moves from the snare to the side-stick.
//! - Complexity (simple↔complex) changes the groove: kick and snare density, extra snare notes,
//!   fills that travel round the toms when simple and stay on the snare when complex.
//! - Regenerate keeps the groove and re-rolls the details (ghost notes, fills, small variations).
//! - Ghost Notes adds soft snare and half-open hat strokes (velocity under 40).
//! - Feel shifts every note early (push) or late (pull): a pure timing offset.
//! - Fills come at the end of phrases (4 bars when simple, 8 when complex at the default amount)
//!   and before section changes; a crash marks the next section's downbeat.
//!
//! Our generator builds a groove for each section from metric rules (strong beats, backbeats,
//! style leanings) seeded by the character, complexity and kit-piece variation, then plays it
//! bar by bar with fills, crashes, ghosts, an ending, swing, feel and humanizing from the region
//! seed.

use crate::character::{Character, Groove};
use crate::kit::{KitMap, Piece};
use crate::{beats_of, finish, vel, Note, RegionSpec, Rng, SectionKind};

/// Swing on eighth notes or sixteenths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwingType {
    Eighths,
    Sixteenths,
}

/// The Snare control (Details): what plays the backbeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnareMode {
    /// Snare, side-stick when played soft (Logic's Automatic)
    Automatic,
    Snare,
    SideStick,
    Rimshot,
}

/// The Tempo control (Details): Standard, Half or Double time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempoFeel {
    Standard,
    Half,
    Double,
}

/// One of the kit sections on the Main tab (Percussion, Hi-Hat / Cymbal, Kick & Snare): on or
/// off, and which pattern variation it plays (0 = automatic, chosen from complexity; 1.. the
/// variation dots).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KitSection {
    pub enabled: bool,
    pub variation: u8,
}

impl KitSection {
    pub const AUTO: KitSection = KitSection { enabled: true, variation: 0 };
    pub const OFF: KitSection = KitSection { enabled: false, variation: 0 };
}

/// The number of pattern variations of each kit section.
pub const KICK_SNARE_VARIATIONS: u8 = 6;
pub const CYMBAL_VARIATIONS: u8 = 6;
pub const PERCUSSION_VARIATIONS: u8 = 7;

/// A drummer region's settings: the controls of Logic's Session Player editor.
#[derive(Clone, Debug, PartialEq)]
pub struct DrummerParams {
    /// The character's identifier ([`Character::id`]; name or personality also work).
    pub character: String,
    /// The XY pad: 0 simple .. 1 complex
    pub complexity: f32,
    /// The XY pad: 0 soft .. 1 loud
    pub intensity: f32,
    pub kick_snare: KitSection,
    pub cymbals: KitSection,
    pub percussion: KitSection,
    /// Fills: 0 none .. 1 many and long
    pub fills: f32,
    /// Swing: 0 straight .. 1 full triplet swing
    pub swing: f32,
    pub swing_type: SwingType,
    /// Feel: -1 pull (late) .. 1 push (early)
    pub feel: f32,
    /// Ghost Notes: 0 .. 1
    pub ghost_notes: f32,
    /// Hi-Hat: 0 closed .. 1 open (0.35 = half-closed, Logic's default)
    pub hihat: f32,
    pub snare: SnareMode,
    /// Dynamics: velocity range, 1 = as measured
    pub dynamics: f32,
    /// Humanize: timing and velocity looseness, 0 .. 1
    pub humanize: f32,
    pub tempo: TempoFeel,
    /// Follow: how strongly the kick and snare follow the rhythm given to [`generate`] (0 off .. 1)
    pub follow: f32,
    pub seed: u64,
    /// MIDI channel of the notes (0-15)
    pub channel: u8,
}

impl Default for DrummerParams {
    /// Pop Rock (Kyle) as Logic's Half-pipe preset sets it up.
    fn default() -> Self {
        DrummerParams {
            character: Character::default_drummer().id.into(),
            complexity: 0.41,
            intensity: 0.87,
            kick_snare: KitSection::AUTO,
            cymbals: KitSection::AUTO,
            percussion: KitSection::OFF,
            fills: 0.35,
            swing: 0.0,
            swing_type: SwingType::Sixteenths,
            feel: 0.06,
            ghost_notes: 0.22,
            hihat: 0.35,
            snare: SnareMode::Automatic,
            dynamics: 1.0,
            humanize: 0.22,
            tempo: TempoFeel::Standard,
            follow: 0.0,
            seed: 1,
            channel: 9,
        }
    }
}

impl DrummerParams {
    /// A character with its own defaults (swing and feel from its style).
    pub fn for_character(c: &Character) -> Self {
        DrummerParams {
            character: c.id.into(),
            swing: c.style.swing,
            swing_type: if c.style.swing_sixteenths { SwingType::Sixteenths } else { SwingType::Eighths },
            feel: c.style.feel * 0.3,
            percussion: if c.style.groove == Groove::Percussion { KitSection::AUTO } else { KitSection::OFF },
            ..Default::default()
        }
    }

    pub fn character(&self) -> &'static Character {
        Character::find(&self.character).unwrap_or(Character::default_drummer())
    }

    /// Regenerate: the same settings with a new seed.
    pub fn regenerated(&self) -> Self {
        let mut r = Rng::new(self.seed);
        DrummerParams { seed: r.next_u64() >> 1, ..self.clone() }
    }
}

/// A hit before timing: where on the grid, what, how hard, how long.
#[derive(Clone, Copy, Debug)]
struct Hit {
    tick: u32,
    piece: Piece,
    vel: f32,
    len: u32,
}

const STEP: u32 = 240; // a sixteenth

/// Velocity lines measured against Logic's intensity (0..100): (offset, slope).
mod v {
    pub const KICK: (f32, f32) = (68.0, 0.565);
    pub const SNARE: (f32, f32) = (69.0, 0.55);
    pub const HAT_ON: (f32, f32) = (63.0, 0.57);
    pub const HAT_OFF: (f32, f32) = (14.0, 0.7);
    pub const TOM: (f32, f32) = (70.0, 0.55);
    pub const CRASH: (f32, f32) = (62.0, 0.6);
    pub const GHOST: (f32, f32) = (6.0, 0.14);
    pub const PERC: (f32, f32) = (40.0, 0.5);

    pub fn at(line: (f32, f32), i: f32) -> f32 {
        line.0 + line.1 * i
    }
}

/// The section's groove: one or two bars of grid steps per piece.
#[derive(Clone, Debug)]
struct GrooveBars {
    kick: [Vec<u32>; 2],
    snare_extra: [Vec<u32>; 2],
    backbeats: Vec<u32>,
    hat_steps: Vec<u32>,
    cymbal: Piece,
    open_hat_every: u32,
    perc: Vec<(u32, Piece, f32)>,
    perc_on: bool,
}

/// The drummer's performance for a region. `follow`: the onsets (song ticks) of a track to
/// follow (Logic's Follow), used when `params.follow` is above 0.
pub fn generate(params: &DrummerParams, region: &RegionSpec, follow: Option<&[u32]>) -> Vec<Note> {
    let ch = params.character();
    let style = ch.style;
    let kit = ch.kit_map();
    let bar_t = region.bar_ticks();
    let (beats, beat_t) = beats_of(region.signature);
    let steps = (bar_t / STEP).max(1);
    let c = params.complexity.clamp(0.0, 1.0);
    let percussion_only = style.groove == Groove::Percussion;
    let mut hits: Vec<Hit> = Vec::new();
    let mut detail = Rng::derive(params.seed, 1);
    let bars = region.bars();
    // section boundaries inside the region (bar indices where a new section starts)
    let section_of = |b: u32| region.section_at(region.start + b * bar_t);
    let mut groove_cache: Vec<((SectionKind, u32), GrooveBars)> = Vec::new();
    let mut fill_before_next = false;
    for b in 0..bars {
        let t0 = region.start + b * bar_t;
        let (kind, s_start, s_end) = section_of(b);
        let bar_in_section = if region.markers.is_empty() { b } else { t0.saturating_sub(s_start) / bar_t };
        let next_new_section = b + 1 < bars && section_of(b + 1).1 != s_start || (s_end != u32::MAX && t0 + bar_t >= s_end);
        let last = b + 1 == bars;
        let energy = kind.energy();
        let i = ((params.intensity + energy).clamp(0.0, 1.0)) * 100.0;
        let cs = match kind {
            SectionKind::Intro | SectionKind::Breakdown => (c - 0.25).max(0.0),
            SectionKind::Chorus => (c + 0.08).min(1.0),
            _ => c,
        };
        let groove = match groove_cache.iter().find(|g| g.0 == (kind, s_start)) {
            Some(g) => g.1.clone(),
            None => {
                let g = build_groove(params, ch, kind, cs, i, steps, beats, beat_t);
                groove_cache.push(((kind, s_start), g.clone()));
                g
            }
        };
        let which = (bar_in_section % 2) as usize;
        let snare_piece = match params.snare {
            SnareMode::Automatic if i < 25.0 => Piece::SideStick,
            SnareMode::Automatic | SnareMode::Snare => Piece::Snare,
            SnareMode::SideStick => Piece::SideStick,
            SnareMode::Rimshot => Piece::SnareRim,
        };
        let soft_intro = kind == SectionKind::Intro && params.snare == SnareMode::Automatic && i < 45.0;
        let backbeat_piece = if soft_intro || style.groove == Groove::Brush && i < 50.0 { Piece::SideStick } else { snare_piece };

        // what this bar is: ending, fill bar, or groove
        let ending = last && region.ending;
        let fills = params.fills.clamp(0.0, 1.0);
        let phrase = if fills > 0.8 { 2 } else if fills + (1.0 - c) * 0.25 > 0.55 { 4 } else { 8 };
        let fill_bar = !ending
            && fills > 0.04
            && (next_new_section || (bar_in_section + 1) % phrase == 0 || (last && !region.ending) || (b + 2 == bars && region.ending));
        let crash_here = (b > 0 && fill_before_next) || (b == 0 && region.continues && bar_in_section == 0 && fills > 0.04);
        fill_before_next = fill_bar;

        if ending {
            let v = v::at(v::CRASH, i);
            hits.push(Hit { tick: t0, piece: Piece::Crash, vel: v, len: bar_t });
            if i > 55.0 {
                hits.push(Hit { tick: t0, piece: Piece::Crash2, vel: v - 18.0, len: bar_t });
            }
            if !percussion_only {
                hits.push(Hit { tick: t0, piece: Piece::Kick, vel: v::at(v::KICK, i) + 4.0, len: STEP });
                if c > 0.4 {
                    hits.push(Hit { tick: t0, piece: snare_piece, vel: v::at(v::SNARE, i), len: STEP });
                }
            }
            continue;
        }

        // the fill's length in beats and where it starts
        let fill_beats = if fill_bar {
            let l = 1.0 + fills * 2.2 + c * 0.3 + (detail.f() - 0.5) * 0.8;
            (l.round() as u32).clamp(1, beats.max(1))
        } else {
            0
        };
        let fill_from = steps - (fill_beats * beat_t / STEP).min(steps);

        // follow: this bar's onsets on the grid
        let followed: Vec<u32> = match follow {
            Some(on) if params.follow > 0.0 => {
                let mut v: Vec<u32> = on.iter().filter(|&&t| t + STEP / 2 >= t0 && t < t0 + bar_t - STEP / 2).map(|&t| ((t + STEP / 2).saturating_sub(t0) / STEP).min(steps - 1)).collect();
                v.sort();
                v.dedup();
                v
            }
            _ => Vec::new(),
        };

        // kick and snare
        if params.kick_snare.enabled && !percussion_only {
            let mut kicks = groove.kick[which].clone();
            if !followed.is_empty() {
                let mut fr = Rng::derive(params.seed, 50 + b as u64);
                kicks.retain(|s| *s == 0 || !fr.chance(params.follow));
                for s in &followed {
                    if !groove.backbeats.contains(s) && fr.chance(params.follow) {
                        kicks.push(*s);
                    }
                }
                kicks.sort();
                kicks.dedup();
            }
            // a small variation now and then (more when complex)
            if detail.chance(0.04 + 0.16 * c) {
                let s = 2 * detail.below(steps / 2) + if c > 0.5 && detail.chance(0.4) { 1 } else { 0 };
                if !groove.backbeats.contains(&s) && s < fill_from {
                    if let Some(k) = kicks.iter().position(|k| *k == s) {
                        if s != 0 {
                            kicks.remove(k);
                        }
                    } else {
                        kicks.push(s);
                    }
                }
            }
            for &s in &kicks {
                if s < fill_from || s == 0 {
                    let accent = if s * STEP % beat_t == 0 { 2.0 } else { -3.0 };
                    hits.push(Hit { tick: t0 + s * STEP, piece: Piece::Kick, vel: v::at(v::KICK, i) + accent, len: STEP });
                }
            }
            for &s in &groove.backbeats {
                if s < fill_from {
                    hits.push(Hit { tick: t0 + s * STEP, piece: backbeat_piece, vel: v::at(v::SNARE, i), len: STEP });
                    if style.clap {
                        hits.push(Hit { tick: t0 + s * STEP, piece: Piece::Clap, vel: v::at(v::SNARE, i) - 4.0, len: STEP });
                    }
                }
            }
            for &s in &groove.snare_extra[which] {
                if s < fill_from {
                    hits.push(Hit { tick: t0 + s * STEP, piece: backbeat_piece, vel: v::at(v::SNARE, i) - 12.0 + 10.0 * c, len: STEP });
                }
            }
            if params.follow > 0.5 && c > 0.5 {
                for &s in &followed {
                    if s % 2 == 1 && s < fill_from && !kicks.contains(&s) && detail.chance((params.follow - 0.5) * c) {
                        hits.push(Hit { tick: t0 + s * STEP, piece: backbeat_piece, vel: v::at(v::SNARE, i) - 16.0, len: STEP });
                    }
                }
            }
        }

        // hats / ride / crash
        if params.cymbals.enabled && !percussion_only {
            for &s in &groove.hat_steps {
                if s >= fill_from && !(fill_from == 0 && s == 0) {
                    continue;
                }
                if s == 0 && crash_here {
                    continue;
                }
                let pos = s * STEP;
                let line = if pos % beat_t == 0 { v::HAT_ON } else { v::HAT_OFF };
                // (complex grooves accent more of the off-beats)
                let mut vel = v::at(line, i) + if pos % beat_t == 0 { 0.0 } else { 9.0 * c * (0.3 + i / 100.0) };
                if pos % (beat_t / 2).max(1) != 0 {
                    vel *= 0.78;
                }
                let mut piece = if style.groove == Groove::Brush && groove.cymbal == Piece::HatClosed { Piece::HatPedal } else { groove.cymbal };
                let open_every = groove.open_hat_every;
                let last_off = steps.saturating_sub(beat_t / 2 / STEP);
                if piece == Piece::HatClosed && open_every > 0 && s == last_off && (bar_in_section + 1) % open_every == 0 && fill_from == steps {
                    piece = Piece::HatOpen;
                    vel += 6.0;
                }
                if style.groove == Groove::FourOnFloor && piece == Piece::HatClosed && params.hihat > 0.2 && pos % beat_t == beat_t / 2 {
                    piece = Piece::HatOpen;
                    vel = v::at(v::HAT_ON, i) - 6.0;
                }
                if matches!(piece, Piece::Ride | Piece::RideBell) {
                    vel += 4.0;
                }
                hits.push(Hit { tick: t0 + pos, piece, vel, len: STEP });
            }
            if crash_here {
                let v = v::at(v::CRASH, i);
                hits.push(Hit { tick: t0, piece: Piece::Crash, vel: v + 6.0, len: STEP });
                if i > 30.0 && c < 0.4 {
                    hits.push(Hit { tick: t0, piece: Piece::Crash2, vel: v - 14.0, len: STEP });
                }
                if !percussion_only && params.kick_snare.enabled {
                    hits.push(Hit { tick: t0, piece: Piece::Kick, vel: v::at(v::KICK, i) + 3.0, len: STEP });
                }
            }
        }

        // brushes: the sweep, a soft stroke on every eighth
        if style.groove == Groove::Brush && params.kick_snare.enabled {
            let half = (beat_t / 2).max(STEP);
            for k in 0..(bar_t / half) {
                let s = k * half / STEP;
                if s < fill_from {
                    hits.push(Hit { tick: t0 + k * half, piece: Piece::Snare, vel: v::at(v::GHOST, i) + 14.0 + if k % 2 == 0 { 4.0 } else { 0.0 }, len: half });
                }
            }
        }
        // trap: hi-hat rolls (triplet and 32nd ratchets) into a beat
        if style.groove == Groove::Trap && params.cymbals.enabled && fill_from == steps && detail.chance(0.2 + 0.6 * c) {
            let k = detail.below(beats.max(1));
            let start = t0 + k * beat_t + beat_t / 2;
            let (n, gap) = if detail.chance(0.5) { (3, beat_t / 6) } else { (4, beat_t / 8) };
            hits.retain(|h| !(h.piece == Piece::HatClosed && h.tick >= start && h.tick < start + beat_t / 2));
            for j in 0..n {
                hits.push(Hit { tick: start + j * gap, piece: Piece::HatClosed, vel: v::at(v::HAT_OFF, i) + j as f32 * 6.0, len: gap });
            }
        }

        // percussion
        if groove.perc_on {
            for &(s, piece, accent) in &groove.perc {
                if s < fill_from || percussion_only {
                    hits.push(Hit { tick: t0 + s * STEP, piece, vel: v::at(v::PERC, i) + accent, len: STEP });
                }
            }
        }

        // ghost notes
        let g = params.ghost_notes.clamp(0.0, 1.0) * style.ghosts;
        if g > 0.0 && params.kick_snare.enabled && !percussion_only {
            let expect = (beats as f32 * beat_t as f32 / 960.0 / 3.0) * 1.8 * g.powf(0.9) * if fill_bar { 0.6 } else { 1.0 };
            let n = detail.round(expect);
            let mut taken: Vec<u32> = Vec::new();
            for _ in 0..n {
                // the sixteenths around a backbeat or a kick
                let cands: Vec<u32> = (0..fill_from).filter(|s| s % 2 == 1 && !taken.contains(s)).collect();
                if cands.is_empty() {
                    break;
                }
                let s = *detail.pick(&cands);
                taken.push(s);
                let use_hat = kit == KitMap::DrumKitDesigner && i >= 25.0 && groove.cymbal == Piece::HatClosed && detail.chance(0.3);
                let piece = if use_hat { Piece::HatHalf } else if backbeat_piece == Piece::SideStick { Piece::SideStick } else { Piece::Snare };
                let gv = v::at(v::GHOST, i) + if use_hat { 4.0 } else { 0.0 } + detail.normal() * 2.5;
                hits.push(Hit { tick: t0 + s * STEP, piece, vel: gv.clamp(4.0, 38.0), len: STEP });
            }
        }

        // the fill
        if fill_bar && params.kick_snare.enabled && !percussion_only {
            fill(&mut hits, &mut detail, params, ch, t0, fill_from, steps, beat_t, i, c);
        }
    }

    // timing: swing, feel, humanize
    let mut out = Vec::with_capacity(hits.len());
    let mut hr = Rng::derive(params.seed, 2);
    let h = params.humanize.clamp(0.0, 1.0);
    let tsd = (3.0 + 28.0 * h) * style.looseness * if h <= 0.0 { 0.0 } else { 1.0 };
    let vsd = 1.5 + 6.0 * h;
    // Feel: measured +12 ticks at pull -0.33, -33.5 at push 1 (a little steeper pulling)
    let f = params.feel.clamp(-1.0, 1.0);
    let feel = -(f * if f < 0.0 { 42.0 } else { 32.0 }) - 2.0 * (h > 0.0) as u8 as f32;
    let dyn_ = (params.dynamics.max(0.0)) * style.dynamics;
    for hit in hits {
        let rel = hit.tick - region.start;
        let in_beat = (rel % bar_t) % beat_t;
        let mut t = hit.tick as f32;
        match params.swing_type {
            SwingType::Eighths if in_beat == beat_t / 2 => t += params.swing.clamp(0.0, 1.0) * beat_t as f32 / 6.0,
            SwingType::Sixteenths if (in_beat / STEP) % 2 == 1 && in_beat % STEP == 0 => t += params.swing.clamp(0.0, 1.0) * STEP as f32 / 3.0,
            _ => {}
        }
        t += feel + hr.normal() * tsd;
        let pos = (t.round() as i64).clamp(region.start as i64, region.end() as i64 - 1) as u32;
        let mean = 80.0;
        let vv = mean + (hit.vel - mean) * dyn_ + hr.normal() * vsd;
        let vv = if hit.vel < 40.0 { vv.min(39.0) } else { vv };
        out.push(Note { channel: params.channel, pitch: kit.note(hit.piece), velocity: vel(vv), pos, len: hit.len });
    }
    finish(out, region)
}

/// Builds a section's groove.
#[allow(clippy::too_many_arguments)]
fn build_groove(params: &DrummerParams, ch: &Character, kind: SectionKind, c: f32, i: f32, steps: u32, beats: u32, beat_t: u32) -> GrooveBars {
    let style = ch.style;
    // the groove's skeleton depends on the character, complexity (in steps), variation and
    // section, not on the region seed: Regenerate keeps the groove and re-rolls the details
    let label = ch.id.bytes().fold(0u64, |h, b| h.wrapping_mul(131).wrapping_add(b as u64));
    let cstep = (c * 10.0).round() as u64;
    let ks_var = params.kick_snare.variation as u64;
    let mut r = Rng::derive(label ^ (ks_var << 40) ^ ((kind as u64) << 48), 100 + cstep);
    let bpb = beat_t / STEP; // steps per beat
    let beat_steps: Vec<u32> = (0..beats).map(|k| k * bpb).collect();
    let mut groove = style.groove;
    if params.tempo == TempoFeel::Half && groove != Groove::Percussion {
        groove = Groove::HalfTime;
    }
    // the backbeats
    let mut backbeats: Vec<u32> = match groove {
        Groove::HalfTime | Groove::Trap => vec![beat_steps[(beats / 2) as usize]],
        Groove::Percussion => vec![],
        _ => {
            let mut v: Vec<u32> = beat_steps.iter().enumerate().filter(|(k, _)| k % 2 == 1).map(|(_, s)| *s).collect();
            if beats % 2 == 1 && beats > 1 {
                v.push(beat_steps[beats as usize - 1]);
            }
            v
        }
    };
    if params.tempo == TempoFeel::Double {
        backbeats = beat_steps.iter().map(|s| s + bpb / 2).filter(|s| *s < steps).collect();
    }
    if beats == 1 {
        backbeats.clear();
    }
    // the kick: target hits per bar
    let ks = style.kick_syncopation;
    // kicks per beat against complexity, fitted to Logic's Pop Rock grid (the groove's share:
    // fills and crashes add their own)
    let rate = interp(&[(0.0, 0.36), (0.078, 0.375), (0.458, 0.74), (0.848, 0.606), (1.0, 0.6)], c) * (1.0 + (ks - 0.35) * 0.6);
    let kick_target = match groove {
        Groove::FourOnFloor => beats as f32 + c * 0.6 * beats as f32 / 4.0,
        Groove::HalfTime | Groove::Trap => beats as f32 * rate * 0.7,
        Groove::Brush => beats as f32 * rate * 0.6,
        _ => beats as f32 * rate,
    } * if params.tempo == TempoFeel::Half { 0.6 } else { 1.0 };
    let strength = |s: u32| -> u32 {
        if s == 0 {
            4
        } else if s % bpb == 0 {
            3
        } else if s % (bpb / 2).max(1) == 0 {
            2
        } else {
            1
        }
    };
    let mut weights: Vec<(u32, f32)> = (1..steps)
        .filter(|s| !backbeats.contains(s))
        .map(|s| {
            let w = match strength(s) {
                3 => 1.0,
                2 => 0.5 + 0.3 * ks,
                _ => (0.08 + 0.5 * ks) * c,
            };
            // just before a backbeat is a weak place for the kick
            let before_bb = backbeats.contains(&(s + 1)) as u8 as f32 * 0.4;
            (s, (w - before_bb).max(0.01) * (0.35 + r.f()))
        })
        .collect();
    weights.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut kick = [vec![0u32], vec![0u32]];
    if groove == Groove::FourOnFloor {
        kick = [beat_steps.clone(), beat_steps.clone()];
        for k in kick.iter_mut() {
            if r.chance(c * 0.8) {
                let s = steps - 1 - (r.below(2)) * 2;
                k.push(s);
            }
        }
    } else {
        // the two bars of the groove share the fractional part
        let a = (kick_target + 0.25).floor().max(1.0) as u32;
        let b = (kick_target + 0.75).floor().max(1.0) as u32;
        kick[0].extend(weights.iter().take(a.saturating_sub(1) as usize).map(|w| w.0));
        kick[1] = kick[0].clone();
        // bar B: one change to bring it to its count
        if b > a {
            if let Some(w) = weights.get(a.saturating_sub(1) as usize) {
                kick[1].push(w.0);
            }
        } else if b < a && kick[1].len() > 1 {
            kick[1].pop();
        } else if r.chance(0.3 + 0.4 * c) && kick[1].len() > 1 {
            kick[1].pop();
            if let Some(w) = weights.get(a as usize) {
                kick[1].push(w.0);
            }
        }
    }
    for k in kick.iter_mut() {
        k.sort();
        k.dedup();
    }
    // extra snares when complex
    let extra = (beats as f32 * (c - 0.33).max(0.0) * 0.65 * if groove == Groove::Brush { 0.5 } else { 1.0 }).max(0.0);
    let mut snare_extra = [Vec::new(), Vec::new()];
    for (n, se) in snare_extra.iter_mut().enumerate() {
        let count = r.round(extra);
        let mut cands: Vec<(u32, f32)> = (1..steps)
            .filter(|s| !backbeats.contains(s) && !kick[n].contains(s) && strength(*s) <= 2)
            .map(|s| (s, if strength(s) == 2 { 1.0 } else { 0.4 * c } * (0.3 + r.f())))
            .collect();
        cands.sort_by(|a, b| b.1.total_cmp(&a.1));
        se.extend(cands.iter().take(count as usize).map(|c| c.0));
    }
    // cymbals
    let cvar = if params.cymbals.variation == 0 {
        let sixteenths = style.sixteenths * (0.3 + c) > 0.5 || groove == Groove::Trap;
        let ride = (matches!(kind, SectionKind::Chorus | SectionKind::Bridge) && style.ride * (i / 100.0) > 0.3) || (style.ride > 0.45 && i > 90.0 && c > 0.7);
        if ride {
            5
        } else if sixteenths {
            3
        } else if c < 0.03 && i < 30.0 {
            1
        } else {
            2
        }
    } else {
        params.cymbals.variation.min(CYMBAL_VARIATIONS)
    };
    let (rate_steps, cymbal) = match cvar {
        1 => (bpb, Piece::HatClosed),
        2 => ((bpb / 2).max(1), Piece::HatClosed),
        3 => (1, Piece::HatClosed),
        4 => ((bpb / 2).max(1), Piece::HatOpen),
        5 => ((bpb / 2).max(1), Piece::Ride),
        _ => (bpb, Piece::RideBell),
    };
    let cymbal = if cymbal == Piece::HatClosed && params.hihat > 0.85 { Piece::HatOpen } else { cymbal };
    let mut hat_steps: Vec<u32> = (0..steps).step_by(rate_steps as usize).collect();
    if rate_steps == 1 && groove != Groove::Trap {
        // sixteenth hats breathe: drop some e's and a's when less complex
        let keep = 0.55 + 0.45 * c;
        hat_steps.retain(|s| s % 2 == 0 || r.chance(keep));
    }
    if groove == Groove::Brush {
        hat_steps = (0..steps).step_by(bpb as usize).filter(|s| backbeats.contains(s)).collect();
    }
    let open_hat_every = if cymbal == Piece::HatClosed && (style.open_hats * 0.6 + params.hihat * 0.4) > 0.25 + r.f() * 0.3 { if c > 0.5 { 2 } else { 4 } } else { 0 };
    // percussion
    let perc_on = params.percussion.enabled || style.groove == Groove::Percussion;
    let mut perc = Vec::new();
    if perc_on {
        let pv = if params.percussion.variation == 0 { 1 + (c * (PERCUSSION_VARIATIONS - 1) as f32).round() as u8 } else { params.percussion.variation.min(PERCUSSION_VARIATIONS) };
        let half = (bpb / 2).max(1);
        match pv {
            1 => {
                for &s in &backbeats {
                    perc.push((s, Piece::Tambourine, 10.0));
                }
            }
            2 => {
                for s in (0..steps).step_by(half as usize) {
                    perc.push((s, Piece::Tambourine, if s % bpb == 0 { 8.0 } else { -2.0 }));
                }
            }
            3 => {
                for s in (0..steps).step_by(half as usize) {
                    perc.push((s, Piece::Shaker, if s % bpb == 0 { 0.0 } else { 6.0 }));
                }
            }
            4 => {
                for s in 0..steps {
                    perc.push((s, Piece::Shaker, if s % 2 == 1 { 4.0 } else { -6.0 }));
                }
            }
            5 => {
                for s in (0..steps).step_by(bpb as usize) {
                    perc.push((s, Piece::Cowbell, if s == 0 { 6.0 } else { 0.0 }));
                }
                for &s in &backbeats {
                    perc.push((s, Piece::Tambourine, 6.0));
                }
            }
            6 => {
                for s in 0..steps {
                    let p = if s % 4 == 3 { Some(Piece::Conga) } else if s % 4 == 2 && r.chance(0.6) { Some(Piece::Bongo) } else { None };
                    if let Some(p) = p {
                        perc.push((s, p, 2.0));
                    }
                }
                for s in (0..steps).step_by(half as usize) {
                    perc.push((s, Piece::Shaker, -4.0));
                }
            }
            _ => {
                for s in 0..steps {
                    if r.chance(0.3 + 0.4 * c) || s % bpb == 0 {
                        let p = *r.pick(&[Piece::Conga, Piece::Bongo, Piece::Timbale, Piece::Woodblock]);
                        perc.push((s, p, if s % bpb == 0 { 8.0 } else { 0.0 }));
                    }
                }
                for &s in &backbeats {
                    perc.push((s, Piece::Clap, 10.0));
                }
            }
        }
    }
    GrooveBars { kick, snare_extra, backbeats, hat_steps, cymbal, open_hat_every, perc, perc_on }
}

/// Piecewise-linear interpolation through (x, y) points (flat outside them).
fn interp(pts: &[(f32, f32)], x: f32) -> f32 {
    if x <= pts[0].0 {
        return pts[0].1;
    }
    for w in pts.windows(2) {
        if x <= w[1].0 {
            let t = (x - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
            return w[0].1 + (w[1].1 - w[0].1) * t;
        }
    }
    pts[pts.len() - 1].1
}

/// A fill from grid step `from` to the bar's end.
#[allow(clippy::too_many_arguments)]
fn fill(hits: &mut Vec<Hit>, r: &mut Rng, p: &DrummerParams, ch: &Character, t0: u32, from: u32, steps: u32, beat_t: u32, i: f32, c: f32) {
    let style = ch.style;
    // simple fills move in eighths, complex ones in sixteenths (with gaps)
    let sub = if c < 0.35 { 2 } else { 1 };
    let positions: Vec<u32> = (from..steps).filter(|s| (s - from) % sub == 0).collect();
    if positions.is_empty() {
        return;
    }
    let n = positions.len();
    // simple fills go round the toms, complex ones stay on the snare
    let around_kit = r.chance(style.toms * (1.2 - c).max(0.0).powi(3) + 0.05);
    let toms = [Piece::TomHigh, Piece::TomMid, Piece::TomLow, Piece::TomFloor];
    let snare_part = if around_kit { (n as f32 * (c * 0.4 + r.f() * 0.2)).round() as usize } else { n };
    let base = v::at(v::TOM, i);
    let snare_v = v::at(v::SNARE, i);
    for (k, &s) in positions.iter().enumerate() {
        let firstlast = k == 0 || k + 1 == n;
        if !firstlast && r.chance(0.12 + (1.0 - c) * 0.1 + if sub == 1 { 0.15 } else { 0.0 }) {
            continue;
        }
        let tick = t0 + s * STEP;
        let ramp = (k as f32 / n.max(1) as f32) * 10.0 - 6.0;
        let on_beat = (s * STEP) % beat_t == 0;
        if k < snare_part {
            // snare, with rim accents on the beats when complex
            let piece = if on_beat && c > 0.35 && r.chance(0.5) { Piece::SnareRim } else { Piece::Snare };
            hits.push(Hit { tick, piece, vel: snare_v + ramp + if on_beat { 4.0 } else { -6.0 }, len: 100 });
        } else {
            let left = n - snare_part;
            let idx = ((k - snare_part) * toms.len()) / left.max(1);
            let tom = toms[idx.min(toms.len() - 1)];
            hits.push(Hit { tick, piece: tom, vel: base + ramp, len: 100 });
            // simple fills double the snare with the tom now and then
            if r.chance((1.0 - c) * 0.35) {
                hits.push(Hit { tick, piece: Piece::Snare, vel: snare_v + ramp - 4.0, len: 100 });
            }
        }
        // the kick under the fill's beats
        if on_beat && (k == 0 || r.chance(0.35 + (1.0 - c) * 0.3)) {
            hits.push(Hit { tick, piece: Piece::Kick, vel: v::at(v::KICK, i) - 4.0, len: 100 });
        }
    }
    // a last flourish on the bar's last sixteenth when complex
    if c > 0.6 && sub == 1 && r.chance(0.5) {
        let last = t0 + (steps - 1) * STEP;
        if !hits.iter().any(|h| h.tick == last && h.piece == Piece::Snare) {
            hits.push(Hit { tick: last, piece: Piece::Snare, vel: snare_v - 2.0, len: 100 });
        }
    }
    let _ = p;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_notes() {
        let r = RegionSpec::new(0, 3840 * 8, (4, 4));
        let n = generate(&DrummerParams::default(), &r, None);
        assert!(n.len() > 60, "{}", n.len());
    }
}
