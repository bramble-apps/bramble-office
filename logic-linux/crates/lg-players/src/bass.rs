//! The Bass Player: follows the region's (or the chord track's) chords with a style's rhythm,
//! the complexity × intensity pad and the Details controls (Dead Notes, Slides, Pickup Hits,
//! Dynamic Mute, Align Root, Double Stops, Mute Offset, Feel, Dynamics, Tempo, Humanize).
//!
//! Measured on Logic's Pop Songwriter bass (intensity 71, complexity 50, 3/4, C-Am-F-G): one
//! root figure per bar (the downbeat held about a beat, then the off-beats), the roots in the
//! octave above the Lowest Note (40, E1: C 48, A 45, F 41, G 43), velocities falling through the
//! bar (79, 73, 67), a hard sixteenth pickup on the next root before some chord changes, timing
//! within a few ticks. Our figures are built from rules per style; Logic's are not copied.

use crate::chords::{chord_at, next_change, Chord, ChordAt};
use crate::drummer::{SwingType, TempoFeel};
use crate::{beats_of, finish, vel, Note, RegionSpec, Rng, SectionKind};

/// The bass characters (CharacterIdentifier as Logic stores it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BassStyle {
    PopSongwriter,
    PopRock,
    ClassicRock,
    ModernRnB,
    ModernMotown,
    IndieDisco,
    Slap,
    RootsSongwriter,
}

impl BassStyle {
    pub const ALL: [BassStyle; 8] = [
        BassStyle::PopSongwriter,
        BassStyle::PopRock,
        BassStyle::ClassicRock,
        BassStyle::ModernRnB,
        BassStyle::ModernMotown,
        BassStyle::IndieDisco,
        BassStyle::Slap,
        BassStyle::RootsSongwriter,
    ];

    pub fn id(self) -> &'static str {
        match self {
            BassStyle::PopSongwriter => "Electric Bass - Pop Songwriter",
            BassStyle::PopRock => "Electric Bass - Pop Rock",
            BassStyle::ClassicRock => "Electric Bass - Classic Rock",
            BassStyle::ModernRnB => "Electric Bass - Modern R&B",
            BassStyle::ModernMotown => "Electric Bass - Modern Motown",
            BassStyle::IndieDisco => "Electric Bass - Indie Disco",
            BassStyle::Slap => "Electric Bass - Slap",
            BassStyle::RootsSongwriter => "Upright Bass - Roots Songwriter",
        }
    }

    pub fn name(self) -> &'static str {
        self.id().split(" - ").nth(1).unwrap_or("")
    }

    pub fn find(id: &str) -> Option<BassStyle> {
        let k = id.trim();
        BassStyle::ALL.into_iter().find(|s| s.id().eq_ignore_ascii_case(k) || s.name().eq_ignore_ascii_case(k))
    }
}

/// A bass region's settings.
#[derive(Clone, Debug, PartialEq)]
pub struct BassParams {
    pub style: BassStyle,
    /// 0 simple .. 1 complex
    pub complexity: f32,
    /// 0 soft .. 1 loud
    pub intensity: f32,
    /// fills into new sections, 0..1
    pub fills: f32,
    pub swing: f32,
    pub swing_type: SwingType,
    /// -1 pull .. 1 push
    pub feel: f32,
    /// Dead Notes 0..1
    pub dead_notes: f32,
    /// Slides 0..1 (played as a short grace note a tone away; Logic bends the pitch)
    pub slides: f32,
    /// Pickup Hits: a hard sixteenth on the next root before every chord change
    pub pickup_hits: bool,
    /// Dynamic Mute: softer playing is also shorter (palm muted)
    pub dynamic_mute: bool,
    /// Align Root: always the chord's root on a change (off: the slash chord's bass note)
    pub align_root: bool,
    /// Double Stops 0..1
    pub double_stops: f32,
    /// Mute Offset -1 (longer) .. 1 (shorter)
    pub mute_offset: f32,
    pub dynamics: f32,
    pub humanize: f32,
    /// The lowest note the bass plays (40 = E1, a four-string's open E)
    pub lowest_note: u8,
    pub tempo: TempoFeel,
    pub seed: u64,
    pub channel: u8,
}

impl Default for BassParams {
    fn default() -> Self {
        BassParams {
            style: BassStyle::PopSongwriter,
            complexity: 0.5,
            intensity: 0.71,
            fills: 0.0,
            swing: 0.0,
            swing_type: SwingType::Sixteenths,
            feel: 0.0,
            dead_notes: 0.0,
            slides: 0.5,
            pickup_hits: false,
            dynamic_mute: true,
            align_root: true,
            double_stops: 0.0,
            mute_offset: 0.0,
            dynamics: 1.0,
            humanize: 0.25,
            lowest_note: 40,
            tempo: TempoFeel::Standard,
            seed: 1,
            channel: 0,
        }
    }
}

impl BassParams {
    pub fn regenerated(&self) -> Self {
        let mut r = Rng::new(self.seed);
        BassParams { seed: r.next_u64() >> 1, ..self.clone() }
    }
}

/// What a figure's step plays.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tone {
    Root,
    Fifth,
    Octave,
    Third,
    /// a step toward the next chord's root
    Approach,
    /// a muted, pitchless "dead" note
    Dead,
}

#[derive(Clone, Copy, Debug)]
struct Step {
    /// position in the bar, ticks
    at: u32,
    tone: Tone,
    accent: f32,
}

/// One bar's rhythm for a style at a complexity (onsets in ticks from the bar's start).
fn figure(style: BassStyle, c: f32, i: f32, beats: u32, beat_t: u32, r: &mut Rng) -> Vec<Step> {
    let half = beat_t / 2;
    let q = beat_t / 4;
    let mut v: Vec<Step> = vec![Step { at: 0, tone: Tone::Root, accent: 0.0 }];
    let push = |v: &mut Vec<Step>, at: u32, tone: Tone, accent: f32| {
        if at < beats * beat_t && !v.iter().any(|s| s.at == at) {
            v.push(Step { at, tone, accent });
        }
    };
    match style {
        BassStyle::PopSongwriter => {
            // the downbeat held, then the off-beats of the following beats; complex: more
            for k in 1..beats {
                if c >= 0.25 || k == beats / 2 + beats % 2 {
                    push(&mut v, k * beat_t + half, Tone::Root, -6.0 * k as f32);
                }
                if c > 0.65 && r.chance(c - 0.5) {
                    push(&mut v, k * beat_t, if r.chance(0.5) { Tone::Fifth } else { Tone::Octave }, -8.0);
                }
            }
        }
        BassStyle::PopRock | BassStyle::ClassicRock => {
            // eighths when loud, quarters when soft; fifths and octaves when complex
            let eighths = i > 0.55 || c > 0.6;
            for k in 0..beats {
                if k > 0 {
                    push(&mut v, k * beat_t, Tone::Root, -2.0);
                }
                if eighths {
                    let tone = if c > 0.5 && r.chance(c * 0.4) { if r.chance(0.5) { Tone::Octave } else { Tone::Fifth } } else { Tone::Root };
                    push(&mut v, k * beat_t + half, tone, -8.0);
                }
            }
            if style == BassStyle::ClassicRock && c > 0.4 {
                push(&mut v, (beats - 1) * beat_t + half, Tone::Approach, -4.0);
            }
        }
        BassStyle::ModernRnB => {
            // sparse and syncopated: the downbeat, the "a" of a beat, a late off-beat
            if beats > 1 {
                push(&mut v, beat_t + 3 * q, Tone::Root, -6.0);
            }
            if c > 0.3 && beats > 2 {
                push(&mut v, 2 * beat_t + half, Tone::Octave, -10.0);
            }
            if c > 0.6 {
                push(&mut v, (beats - 1) * beat_t + 3 * q, Tone::Approach, -8.0);
                push(&mut v, q * 3, Tone::Fifth, -14.0);
            }
        }
        BassStyle::ModernMotown => {
            // quarter notes walking through the chord, approach into the next
            for k in 1..beats {
                let tone = match k % 3 {
                    1 => Tone::Fifth,
                    2 => Tone::Octave,
                    _ => Tone::Third,
                };
                push(&mut v, k * beat_t, tone, -4.0);
                if c > 0.5 && r.chance(c * 0.5) {
                    push(&mut v, k * beat_t + half, Tone::Root, -12.0);
                }
            }
        }
        BassStyle::IndieDisco => {
            // octave eighths: root on the beat, octave on the off-beat
            for k in 0..beats {
                if k > 0 {
                    push(&mut v, k * beat_t, Tone::Root, -2.0);
                }
                push(&mut v, k * beat_t + half, Tone::Octave, -4.0);
                if c > 0.6 && r.chance(c - 0.5) {
                    push(&mut v, k * beat_t + 3 * q, Tone::Octave, -12.0);
                }
            }
        }
        BassStyle::Slap => {
            // thumb on the beats, pops on the octave, dead notes between
            for k in 0..beats {
                if k > 0 && r.chance(0.6 + 0.4 * c) {
                    push(&mut v, k * beat_t, Tone::Root, -2.0);
                }
                push(&mut v, k * beat_t + half, Tone::Octave, 6.0);
                if r.chance(0.3 + 0.5 * c) {
                    push(&mut v, k * beat_t + q, Tone::Dead, -30.0);
                }
                if c > 0.5 && r.chance(c * 0.5) {
                    push(&mut v, k * beat_t + 3 * q, Tone::Fifth, -6.0);
                }
            }
        }
        BassStyle::RootsSongwriter => {
            // two-feel (root, fifth); complex: walking quarters
            let mid = (beats / 2).max(1);
            if beats > 1 {
                push(&mut v, mid * beat_t, Tone::Fifth, -4.0);
            }
            if c > 0.45 {
                for k in 1..beats {
                    if k != mid {
                        push(&mut v, k * beat_t, if k + 1 == beats { Tone::Approach } else { Tone::Third }, -6.0);
                    }
                }
            }
        }
    }
    v.sort_by_key(|s| s.at);
    v
}

/// The lowest pitch of a pitch class at or above `lowest`.
fn place(pc: u8, lowest: u8) -> u8 {
    (lowest as i32 + (pc as i32 - lowest as i32).rem_euclid(12)).min(127) as u8
}

/// The bass performance for a region over `chords` (song ticks; one chord per bar or more).
pub fn generate(params: &BassParams, region: &RegionSpec, chords: &[ChordAt]) -> Vec<Note> {
    let bar_t = region.bar_ticks();
    let (_, beat_t) = beats_of(region.signature);
    let beat_t = match params.tempo {
        TempoFeel::Half => beat_t * 2,
        TempoFeel::Double => beat_t / 2,
        TempoFeel::Standard => beat_t,
    };
    let beats = (bar_t / beat_t).max(1);
    let c = params.complexity.clamp(0.0, 1.0);
    let lowest = params.lowest_note.clamp(20, 70);
    let mut r = Rng::derive(params.seed, 11);
    let fallback = [ChordAt { pos: region.start, chord: Chord::major(0) }];
    let chords = if chords.is_empty() { &fallback[..] } else { chords };
    let mut notes: Vec<Note> = Vec::new();
    // the figure belongs to the style and complexity; Regenerate re-rolls the details
    let mut groove_r = Rng::derive(params.style as u64 * 7919 + (c * 10.0).round() as u64, 12);
    let base_figure = figure(params.style, c, params.intensity, beats, beat_t, &mut groove_r);
    let alt_figure = figure(params.style, c, params.intensity, beats, beat_t, &mut groove_r);
    let mut prev_pitch: Option<u8> = None;
    let ch = params.channel;
    for b in 0..region.bars() {
        let t0 = region.start + b * bar_t;
        let (kind, s0, _) = region.section_at(t0);
        let i = (params.intensity + kind.energy()).clamp(0.0, 1.0) * 100.0;
        let fig = if b % 2 == 1 && c > 0.4 { &alt_figure } else { &base_figure };
        // onsets: the figure, plus a downbeat at every chord change inside the bar
        let mut steps: Vec<Step> = fig.clone();
        for chg in chords.iter().filter(|x| x.pos > t0 && x.pos < t0 + bar_t) {
            let at = chg.pos - t0;
            steps.retain(|s| s.at.abs_diff(at) >= beat_t / 4 || s.at == at);
            if !steps.iter().any(|s| s.at == at) {
                steps.push(Step { at, tone: Tone::Root, accent: -2.0 });
            }
        }
        // a fill at the end of a section (and now and then at phrase ends): walking in
        let next = t0 + bar_t;
        let fill = params.fills > 0.05 && ((region.section_at(next).1 != s0 && next < region.end()) || ((b + 1) % 4 == 0 && r.chance(params.fills * 0.6)));
        if fill {
            let from = beats.saturating_sub(1).max(1) * beat_t;
            for k in 0..2 {
                let at = from + k * beat_t / 2;
                if at < bar_t && !steps.iter().any(|s| s.at == at) {
                    steps.push(Step { at, tone: if k == 1 { Tone::Approach } else { Tone::Fifth }, accent: -4.0 });
                }
            }
        }
        steps.sort_by_key(|s| s.at);
        let base_vel = 42.0 + 0.5 * i;
        for (k, s) in steps.iter().enumerate() {
            let tick = t0 + s.at;
            if tick >= region.end() {
                break;
            }
            let chord = chord_at(chords, tick).unwrap_or(Chord::major(0));
            let next_on = steps.get(k + 1).map_or(t0 + bar_t, |n| t0 + n.at).min(region.end());
            let change = next_change(chords, tick);
            let root_pc = if params.align_root { chord.root } else { chord.bass_pc() };
            let root = place(root_pc, lowest);
            let mut tone = s.tone;
            // complexity: passing fifths and octaves on weak steps, approach notes before changes
            if tone == Tone::Root && s.at != 0 && c > 0.55 && r.chance((c - 0.55) * 0.6) {
                tone = if r.chance(0.5) { Tone::Fifth } else { Tone::Octave };
            }
            if let Some(chg) = change {
                if chg.pos == next_on && s.at != 0 && tone != Tone::Dead && r.chance(c * 0.4) {
                    tone = Tone::Approach;
                }
            }
            let pitch = match tone {
                Tone::Root | Tone::Dead => root,
                Tone::Fifth => {
                    let p = root + chord.fifth();
                    if p > lowest + 16 { p - 12 } else { p }
                }
                Tone::Octave => (root + 12).min(lowest + 24),
                Tone::Third => root + chord.third(),
                Tone::Approach => {
                    let target = change.map_or(root, |x| place(x.chord.root, lowest));
                    let step = if r.chance(0.6) { 1 } else { 2 };
                    if r.chance(0.5) || target < lowest + step { target + step } else { target - step }
                }
            };
            if tone == Tone::Dead {
                notes.push(Note { channel: ch, pitch, velocity: vel(28.0 + 0.15 * i), pos: tick, len: 50 });
                continue;
            }
            // length: long notes released a little before the next beat, short ones nearly
            // legato; Dynamic Mute shortens soft playing; Mute Offset scales
            let span = next_on.saturating_sub(tick).max(1);
            let mut len = if span >= beat_t { span.saturating_sub(beat_t * 42 / 100) } else { span * 9 / 10 };
            if params.dynamic_mute {
                len = (len as f32 * (0.55 + 0.45 * (i / 100.0))) as u32;
            }
            len = (len as f32 * (1.0 - params.mute_offset.clamp(-1.0, 1.0) * 0.35)).max(30.0) as u32;
            let mut v = base_vel + s.accent;
            if s.at == 0 && kind == SectionKind::Chorus {
                v += 4.0;
            }
            // slides into a new chord's root
            let on_change = s.at == 0 || chords.iter().any(|x| x.pos == tick);
            if let (true, Some(pp)) = (on_change, prev_pitch) {
                if pp != pitch && tick >= region.start + 70 && r.chance(params.slides.clamp(0.0, 1.0) * 0.2) {
                    let from = if pp < pitch { pitch - 2 } else { pitch + 2 };
                    notes.push(Note { channel: ch, pitch: from, velocity: vel(v - 12.0), pos: tick - 60, len: 60 });
                }
            }
            notes.push(Note { channel: ch, pitch, velocity: vel(v), pos: tick, len });
            // double stops: a tenth (or fifth) above long notes
            if params.double_stops > 0.0 && span >= beat_t && r.chance(params.double_stops.clamp(0.0, 1.0) * 0.5) {
                let up = if r.chance(0.6) { 12 + chord.third() } else { chord.fifth() };
                notes.push(Note { channel: ch, pitch: pitch + up, velocity: vel(v - 10.0), pos: tick, len });
            }
            prev_pitch = Some(pitch);
        }
        // pickup hits: a hard sixteenth on the next root just before a chord change
        for chg in chords.iter().filter(|x| x.pos > t0 && x.pos <= t0 + bar_t && x.pos < region.end()) {
            if chord_at(chords, chg.pos - 1) == Some(chg.chord) {
                continue;
            }
            let p = if params.pickup_hits { 1.0 } else { 0.25 + 0.3 * c };
            if chg.pos >= region.start + beat_t / 4 && r.chance(p) {
                let at = chg.pos - beat_t * 3 / 20;
                let pitch = place(chg.chord.root, lowest);
                notes.retain(|n| !(n.pos >= at && n.pos < chg.pos));
                notes.push(Note { channel: ch, pitch, velocity: vel(base_vel + 34.0), pos: at, len: beat_t * 3 / 20 });
            }
        }
        // dead notes between the notes
        let dead = params.dead_notes.clamp(0.0, 1.0);
        if dead > 0.0 {
            for _ in 0..r.round(dead * beats as f32 * 0.5) {
                let at = t0 + (r.below(beats * 4) * 2 + 1) * beat_t / 8;
                if at < region.end() && !notes.iter().any(|n| n.pos.abs_diff(at) < beat_t / 8) {
                    let p = place(chord_at(chords, at).map_or(0, |x| x.root), lowest);
                    notes.push(Note { channel: ch, pitch: p, velocity: vel(26.0 + 0.15 * i), pos: at, len: 50 });
                }
            }
        }
    }
    // one line: each note ends where the next begins (double stops sound together)
    notes.sort_by_key(|n| (n.pos, n.pitch));
    for k in 0..notes.len() {
        if let Some(next) = notes[k + 1..].iter().find(|n| n.pos > notes[k].pos).map(|n| n.pos) {
            if notes[k].pos + notes[k].len > next {
                notes[k].len = next - notes[k].pos;
            }
        }
    }
    humanize(notes, region, params.humanize, params.feel, params.swing, params.swing_type, params.dynamics, params.seed, beat_t)
}

/// Swing, feel, timing and velocity looseness, dynamics range; then made safe.
#[allow(clippy::too_many_arguments)]
pub(crate) fn humanize(notes: Vec<Note>, region: &RegionSpec, humanize: f32, feel: f32, swing: f32, swing_type: SwingType, dynamics: f32, seed: u64, beat_t: u32) -> Vec<Note> {
    let mut r = Rng::derive(seed, 99);
    let h = humanize.clamp(0.0, 1.0);
    let tsd = if h > 0.0 { 1.5 + 12.0 * h } else { 0.0 };
    let f = feel.clamp(-1.0, 1.0);
    let shift = -(f * if f < 0.0 { 42.0 } else { 32.0 });
    let beat_t = beat_t.max(4);
    let mut out = Vec::with_capacity(notes.len());
    for mut n in notes {
        let in_beat = (n.pos - region.start) % beat_t;
        let mut t = n.pos as f32;
        match swing_type {
            SwingType::Eighths if in_beat == beat_t / 2 => t += swing.clamp(0.0, 1.0) * beat_t as f32 / 6.0,
            SwingType::Sixteenths if in_beat % (beat_t / 2) == beat_t / 4 => t += swing.clamp(0.0, 1.0) * beat_t as f32 / 12.0,
            _ => {}
        }
        t += shift + r.normal() * tsd;
        n.pos = (t.round() as i64).clamp(region.start as i64, region.end() as i64 - 1) as u32;
        let v = 80.0 + (n.velocity as f32 - 80.0) * dynamics.max(0.0) + r.normal() * (1.0 + 5.0 * h);
        n.velocity = vel(v);
        out.push(n);
    }
    finish(out, region)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chords::parse_bars;

    #[test]
    fn roots_follow_the_chords() {
        let region = RegionSpec::new(0, 2880 * 8, (3, 4));
        let chords = parse_bars("C Am F G C Am F G", 0, 2880);
        let n = generate(&BassParams { complexity: 0.1, slides: 0.0, ..Default::default() }, &region, &chords);
        for b in 0..8u32 {
            let first = n.iter().find(|x| x.pos + 30 >= b * 2880 && x.pos < b * 2880 + 200 && x.len > 200).unwrap();
            assert_eq!(first.pitch % 12, chords[b as usize].chord.root, "bar {b}");
            assert!((40..52).contains(&first.pitch));
        }
    }
}
