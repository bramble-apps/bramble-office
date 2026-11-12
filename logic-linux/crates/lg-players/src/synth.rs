//! Synth Players: the Synth Bass Player styles (808 Bass, Pump Bass, Sequenced Bass) and the
//! Synth Keyboard Player styles (Rhythmic Chords, Modulated Pad, Simple Pad), with the extras
//! Logic gives them: an LFO sent as a MIDI controller (CC 74, the filter cutoff of most synths,
//! by default), slides sent as pitch bend (or as overlapping notes), the Modulated Pad's own
//! envelope generator (a controller too) and the Pump Bass's ducking.
//!
//! The patterns are Studio's own (rules over a sixteenth-note grid), not Apple's.
//!
//! A performance is notes plus controller / pitch-bend events ([`Perf`]), both at song ticks.

use crate::chords::{chord_at, Chord, ChordAt};
use crate::{vel, Note, RegionSpec, Rng, PPQ};
pub use lg_core::events::Control;

/// Notes and controller events (song ticks).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Perf {
    pub notes: Vec<Note>,
    pub controls: Vec<Control>,
}

/// A sixteenth note.
pub const SIXTEENTH: u32 = PPQ / 4;

// ---------------------------------------------------------------- LFO

/// The LFO's waveform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    Triangle,
    Sine,
    Square,
    Random,
}

impl Wave {
    pub const ALL: [Wave; 4] = [Wave::Triangle, Wave::Sine, Wave::Square, Wave::Random];
    pub fn name(self) -> &'static str {
        match self {
            Wave::Triangle => "Triangle",
            Wave::Sine => "Sine",
            Wave::Square => "Square",
            Wave::Random => "Random",
        }
    }
}

/// What the LFO's rate is relative to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sync {
    /// cycles per region
    Region,
    /// cycles per chord
    Chord,
    /// cycles per beat (in time with the beat)
    Beats,
    /// Hz, free running
    Free,
    /// cycles per beat, each cycle's rate scaled at random
    Random,
}

impl Sync {
    pub const ALL: [Sync; 5] = [Sync::Region, Sync::Chord, Sync::Beats, Sync::Free, Sync::Random];
    pub fn name(self) -> &'static str {
        match self {
            Sync::Region => "Region",
            Sync::Chord => "Chord",
            Sync::Beats => "Beats",
            Sync::Free => "Free",
            Sync::Random => "Random",
        }
    }
    /// The Rate knob's unit.
    pub fn unit(self) -> &'static str {
        match self {
            Sync::Region => "per region",
            Sync::Chord => "per chord",
            Sync::Free => "Hz",
            _ => "per beat",
        }
    }
}

/// The Synth Player LFO (every style but Simple Pad).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lfo {
    /// 0..1 (0: off)
    pub amount: f32,
    pub wave: Wave,
    /// the waveform's starting point, 0..1 of a cycle
    pub phase: f32,
    /// -1..1: Triangle → down / up sawtooth, Sine leaned, Square pulse width (0: the pure shape)
    pub symmetry: f32,
    /// More: -1..1 curvature
    pub curve: f32,
    /// More: S-shaped curves (no effect when Curve is 0)
    pub s_curve: bool,
    /// More: -1..1 shifts the curve up or down
    pub offset: f32,
    /// cycles per unit of `sync`
    pub rate: f32,
    pub sync: Sync,
    /// More: the LFO applied to note velocity / note length (-1..1; not on the pads)
    pub to_velocity: f32,
    pub to_length: f32,
    /// More: CC Target (74 = cutoff)
    pub cc: u8,
}

impl Default for Lfo {
    fn default() -> Self {
        Lfo { amount: 0.0, wave: Wave::Triangle, phase: 0.0, symmetry: 0.0, curve: 0.0, s_curve: false, offset: 0.0, rate: 1.0, sync: Sync::Beats, to_velocity: 0.0, to_length: 0.0, cc: 74 }
    }
}

/// Bends a 0..1 ramp by `curve` (-1..1); S Curve bends both halves (no effect at 0).
pub fn shape(x: f32, curve: f32, s: bool) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if curve.abs() < 1e-4 {
        return x;
    }
    let k = 1.0 + curve.abs() * 4.0;
    let bend = |x: f32| if curve > 0.0 { x.powf(k) } else { 1.0 - (1.0 - x).powf(k) };
    if s {
        if x < 0.5 { bend(x * 2.0) * 0.5 } else { 0.5 + (1.0 - bend((1.0 - x) * 2.0)) * 0.5 }
    } else {
        bend(x)
    }
}

impl Lfo {
    /// The waveform at a phase (0..1 of a cycle), -1..1 before Amount and Offset; `rnd` is the
    /// cycle's random value (Random).
    pub fn wave_at(&self, ph: f32, rnd: f32) -> f32 {
        let ph = ph.rem_euclid(1.0);
        let sym = self.symmetry.clamp(-0.99, 0.99);
        let v = match self.wave {
            Wave::Triangle => {
                // the peak moves from the middle: -1 → at the start (down saw), 1 → at the end
                let peak = 0.5 + sym * 0.5;
                let up = if ph < peak { ph / peak.max(1e-4) } else { 1.0 - (ph - peak) / (1.0 - peak).max(1e-4) };
                let up = shape(up, self.curve, self.s_curve);
                up * 2.0 - 1.0
            }
            Wave::Sine => {
                // the phase warped so the crest leans left or right
                let peak = 0.25 + sym * 0.2;
                let w = if ph < peak * 2.0 { ph / (peak * 2.0) * 0.5 } else { 0.5 + (ph - peak * 2.0) / (1.0 - peak * 2.0).max(1e-4) * 0.5 };
                (w * std::f32::consts::TAU).sin()
            }
            Wave::Square => {
                let width = 0.5 + sym * 0.5;
                if ph < width { 1.0 } else { -1.0 }
            }
            Wave::Random => rnd * 2.0 - 1.0,
        };
        v
    }

    /// The controller value (0..127) at a tick: `cycle_ticks` is how long a cycle lasts there.
    fn value(&self, pos_in: f32, cycle_ticks: f32, r: &mut Vec<f32>, rng: &mut Rng) -> u8 {
        let cyc = pos_in / cycle_ticks.max(1.0) + self.phase;
        let n = cyc.floor().max(0.0) as usize;
        while r.len() <= n {
            r.push(rng.f());
        }
        let w = self.wave_at(cyc.fract(), r[n]);
        let v = 0.5 + 0.5 * (w * self.amount.clamp(0.0, 1.0)) + self.offset.clamp(-1.0, 1.0) * 0.5;
        (v.clamp(0.0, 1.0) * 127.0).round() as u8
    }

    /// The LFO as CC events every 32nd note over a region (song ticks), chords for Sync Chord.
    pub fn events(&self, region: &RegionSpec, chords: &[ChordAt], channel: u8, seed: u64) -> Vec<Control> {
        if self.amount <= 0.0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut rnd = Vec::new();
        let mut rng = Rng::derive(seed, 0x1F0);
        let step = PPQ / 8;
        let mut last: Option<u8> = None;
        let mut t = region.start;
        // Random sync: per-beat cycles at a random rate each cycle (0.5x .. 2x)
        let mut rand_scale = 1.0f32;
        let mut rand_until = region.start;
        while t < region.end() {
            let rate = self.rate.max(0.01);
            let (pos_in, cycle) = match self.sync {
                Sync::Region => ((t - region.start) as f32, region.length as f32 / rate),
                Sync::Chord => {
                    let c0 = chords.iter().rev().find(|c| c.pos <= t).map_or(region.start, |c| c.pos.max(region.start));
                    let c1 = chords.iter().find(|c| c.pos > t).map_or(region.end(), |c| c.pos.min(region.end()));
                    ((t - c0) as f32, (c1 - c0).max(1) as f32 / rate)
                }
                Sync::Beats => ((t - region.start) as f32, PPQ as f32 / rate),
                Sync::Free => {
                    // Hz at the project tempo: a beat lasts 60 / bpm seconds
                    let beat_s = 60.0 / region.bpm.max(1.0);
                    ((t - region.start) as f32, PPQ as f32 / (rate * beat_s))
                }
                Sync::Random => {
                    if t >= rand_until {
                        rand_scale = 0.5 + 1.5 * rng.f();
                        rand_until = t + (PPQ as f32 / (rate * rand_scale)) as u32;
                    }
                    ((t - region.start) as f32, PPQ as f32 / (rate * rand_scale))
                }
            };
            let v = self.value(pos_in, cycle, &mut rnd, &mut rng);
            if last != Some(v) {
                out.push(Control { kind: 0xB0, channel, pos: t, number: self.cc.min(127), value: v });
                last = Some(v);
            }
            t += step;
        }
        out
    }

    /// To Velocity / To Note Length on the notes (the LFO's value where each note starts).
    pub fn apply_to_notes(&self, notes: &mut [Note], region: &RegionSpec) {
        if self.amount <= 0.0 || (self.to_velocity == 0.0 && self.to_length == 0.0) {
            return;
        }
        let cycle = match self.sync {
            Sync::Region => region.length as f32 / self.rate.max(0.01),
            _ => PPQ as f32 / self.rate.max(0.01),
        };
        for n in notes {
            let ph = (n.pos.saturating_sub(region.start)) as f32 / cycle.max(1.0) + self.phase;
            let w = self.wave_at(ph.fract(), 0.5) * self.amount;
            n.velocity = vel(n.velocity as f32 * (1.0 + 0.5 * w * self.to_velocity.clamp(-1.0, 1.0)));
            n.len = ((n.len as f32) * (1.0 + 0.6 * w * self.to_length.clamp(-1.0, 1.0))).max(20.0) as u32;
        }
    }
}

// ---------------------------------------------------------------- slides

/// How slides are played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideMode {
    /// pitch-bend automation (the Synth Bass default)
    PitchBend,
    /// MIDI notes and articulations for Studio Bass (the electric and acoustic default)
    StudioBass,
    /// overlapping notes, for synths in mono legato with glide or portamento
    Overlap,
}

impl SlideMode {
    pub const ALL: [SlideMode; 3] = [SlideMode::PitchBend, SlideMode::StudioBass, SlideMode::Overlap];
    pub fn name(self) -> &'static str {
        match self {
            SlideMode::PitchBend => "Pitch Bend",
            SlideMode::StudioBass => "Studio Bass",
            SlideMode::Overlap => "Overlap",
        }
    }
}

/// Where a slide starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideStart {
    /// it ends on the target note's start
    BeforeNote,
    /// it starts with the target note
    OnNote,
    /// it starts at the end of the previous note
    EndOfPrevious,
}

impl SlideStart {
    pub const ALL: [SlideStart; 3] = [SlideStart::BeforeNote, SlideStart::OnNote, SlideStart::EndOfPrevious];
    pub fn name(self) -> &'static str {
        match self {
            SlideStart::BeforeNote => "Before Note",
            SlideStart::OnNote => "On Note",
            SlideStart::EndOfPrevious => "End of Previous",
        }
    }
}

/// The Synth Bass Slides settings (and the Bass Player's Slide Mode).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slides {
    /// how many slides, 0..1
    pub amount: f32,
    /// slide length in ticks
    pub time: u32,
    /// Auto: the slide lasts the note
    pub auto: bool,
    /// 0: every note retriggered at the slide .. 100: none
    pub legato: u8,
    pub start: SlideStart,
    /// -1..1 (dragging the drawn curve vertically sets it too)
    pub curve: f32,
    pub s_curve: bool,
    pub mode: SlideMode,
    /// MIDI Mono Mode for MPE instruments: 0 Off, 1 On (Base Channel 1), 2 On (Base Channel 16)
    pub mono_mode: u8,
    /// Pitch Bend Range in semitones (0..96)
    pub bend_range: u8,
}

impl Default for Slides {
    fn default() -> Self {
        Slides { amount: 0.3, time: 120, auto: false, legato: 100, start: SlideStart::BeforeNote, curve: 0.0, s_curve: false, mode: SlideMode::PitchBend, mono_mode: 0, bend_range: 12 }
    }
}

impl Slides {
    /// Turns some note-to-note moves into slides. Pitch Bend: the target note starts at the old
    /// pitch's bend and bends home (or, with Legato, isn't retriggered: the old note bends to
    /// the new pitch and lasts); Studio Bass: the plain bend without curve, as MIDI; Overlap:
    /// the notes overlap (only Amount counts).
    pub fn apply(&self, perf: &mut Perf, seed: u64) {
        let amount = self.amount.clamp(0.0, 1.0);
        if amount <= 0.0 || perf.notes.len() < 2 {
            return;
        }
        let mut r = Rng::derive(seed, 0x51D);
        perf.notes.sort_by_key(|n| (n.pos, n.pitch));
        let range = self.bend_range.clamp(1, 96) as f32;
        let mut k = 1;
        while k < perf.notes.len() {
            let (a, b) = (perf.notes[k - 1], perf.notes[k]);
            let jump = b.pitch as i32 - a.pitch as i32;
            if b.pos <= a.pos || jump == 0 || !r.chance(amount) {
                k += 1;
                continue;
            }
            match self.mode {
                SlideMode::Overlap => {
                    let e = (b.pos + (b.len / 3).clamp(20, 120)).saturating_sub(a.pos);
                    perf.notes[k - 1].len = e.max(perf.notes[k - 1].len);
                }
                _ if jump.unsigned_abs() as f32 > range => {}
                mode => {
                    let time = if self.auto { b.len.max(20) } else { self.time.clamp(10, 3840) };
                    let ch = b.channel;
                    let (from, to) = (-(jump as f32) / range, 0.0f32);
                    let (t0, t1) = match self.start {
                        SlideStart::BeforeNote => (b.pos.saturating_sub(time).max(a.pos), b.pos),
                        SlideStart::OnNote => (b.pos, b.pos + time),
                        SlideStart::EndOfPrevious => (a.pos + a.len.min(b.pos - a.pos), (a.pos + a.len.min(b.pos - a.pos) + time).max(b.pos)),
                    };
                    let retrigger = !r.chance(self.legato as f32 / 100.0);
                    let (curve, s) = if mode == SlideMode::StudioBass { (0.0, false) } else { (self.curve, self.s_curve) };
                    let steps = 8u32;
                    if retrigger {
                        // the new note starts bent at the old pitch and glides home
                        let span = (t1.max(b.pos + 1) - b.pos).max(1);
                        for i in 0..=steps {
                            let x = i as f32 / steps as f32;
                            let v = from + (to - from) * shape(x, curve, s);
                            perf.controls.push(lg_core::regions::bend_event(b.pos + span * i / steps, ch, (v * 8191.0) as i16));
                        }
                    } else {
                        // legato: the old note bends up / down to the new pitch and holds; the
                        // new note isn't struck
                        let span = (t1.saturating_sub(t0)).max(1);
                        for i in 0..=steps {
                            let x = i as f32 / steps as f32;
                            let v = (jump as f32 / range) * shape(x, curve, s);
                            perf.controls.push(lg_core::regions::bend_event(t0 + span * i / steps, a.channel, (v * 8191.0) as i16));
                        }
                        let end = b.pos + b.len;
                        perf.notes[k - 1].len = end - a.pos;
                        perf.controls.push(lg_core::regions::bend_event(end, a.channel, 0));
                        perf.notes.remove(k);
                        continue;
                    }
                }
            }
            k += 1;
        }
        // MIDI Mono Mode (MPE): every note on its own member channel, so bends stay per note
        if self.mono_mode > 0 {
            let members: Vec<u8> = if self.mono_mode == 1 { (1..16).collect() } else { (0..15).collect() };
            let mut i = 0usize;
            for n in perf.notes.iter_mut() {
                let ch = members[i % members.len()];
                for c in perf.controls.iter_mut().filter(|c| c.kind == 0xE0 && c.pos >= n.pos && c.pos <= n.pos + n.len && c.channel == n.channel) {
                    c.channel = ch;
                }
                n.channel = ch;
                i += 1;
            }
        }
        perf.controls.sort_by_key(|c| c.pos);
    }
}

// ---------------------------------------------------------------- envelope

/// How the Modulated Pad envelope fits between accents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvLength {
    /// scaled to the gap between accents
    Relative,
    /// the set length if it fits, otherwise scaled
    AbsoluteScaled,
    /// the set length if it fits, otherwise the end clipped
    AbsoluteClipped,
    /// attack and decay kept, hold scaled (attack and decay shrink only if there's no room)
    FixedAd,
}

impl EnvLength {
    pub const ALL: [EnvLength; 4] = [EnvLength::Relative, EnvLength::AbsoluteScaled, EnvLength::AbsoluteClipped, EnvLength::FixedAd];
    pub fn name(self) -> &'static str {
        match self {
            EnvLength::Relative => "Relative",
            EnvLength::AbsoluteScaled => "Absolute Scaled",
            EnvLength::AbsoluteClipped => "Absolute Clipped",
            EnvLength::FixedAd => "Absolute / Fixed AD",
        }
    }
}

/// The Modulated Pad's envelope generator (sent as a MIDI controller at every accent).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    /// Relative: fractions of the gap (sum ≤ 1); Absolute: ticks / 960 (beats)
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    /// the hollow nodes on the attack and decay lines: -1..1
    pub attack_curve: f32,
    pub decay_curve: f32,
    /// Shift Start: steps of a 64th note, -8..8
    pub shift: i8,
    /// Align: false Start on Step, true Peak on Step
    pub peak_on_step: bool,
    pub invert: bool,
    pub length: EnvLength,
    pub cc: u8,
    /// -1..1
    pub offset: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Envelope { attack: 0.1, hold: 0.2, decay: 0.5, attack_curve: 0.0, decay_curve: 0.3, shift: 0, peak_on_step: false, invert: false, length: EnvLength::Relative, cc: 1, offset: 0.0 }
    }
}

impl Envelope {
    /// (attack, hold, decay) in ticks for a gap between accents.
    pub fn segments(&self, gap: u32) -> (u32, u32, u32) {
        let gap = gap.max(1) as f32;
        let (a, h, d) = (self.attack.max(0.0), self.hold.max(0.0), self.decay.max(0.0));
        let (a, h, d) = match self.length {
            EnvLength::Relative => {
                let s = (a + h + d).max(1e-4);
                (a / s * gap, h / s * gap, d / s * gap)
            }
            _ => {
                let (a, h, d) = (a * PPQ as f32, h * PPQ as f32, d * PPQ as f32);
                let sum = a + h + d;
                if sum <= gap {
                    (a, h, d)
                } else {
                    match self.length {
                        EnvLength::AbsoluteClipped => {
                            let a2 = a.min(gap);
                            let h2 = h.min(gap - a2);
                            (a2, h2, (gap - a2 - h2).min(d))
                        }
                        EnvLength::FixedAd if a + d <= gap => (a, gap - a - d, d),
                        _ => (a * gap / sum, h * gap / sum, d * gap / sum),
                    }
                }
            }
        };
        (a as u32, h as u32, d as u32)
    }

    /// The envelope's level (0..1) `x` ticks after it starts.
    pub fn level(&self, x: u32, gap: u32) -> f32 {
        let (a, h, d) = self.segments(gap);
        let v = if x < a {
            shape(x as f32 / a.max(1) as f32, self.attack_curve, false)
        } else if x < a + h {
            1.0
        } else if x < a + h + d {
            1.0 - shape((x - a - h) as f32 / d.max(1) as f32, self.decay_curve, false)
        } else {
            0.0
        };
        let v = if self.invert { 1.0 - v } else { v };
        (v + self.offset * 0.5).clamp(0.0, 1.0)
    }

    /// CC events for envelopes starting at each accent (song ticks) up to `end`.
    pub fn events(&self, accents: &[u32], end: u32, channel: u8) -> Vec<Control> {
        let mut out = Vec::new();
        let step = PPQ / 16;
        let shift = self.shift as i64 * (PPQ / 16) as i64;
        for (k, &acc) in accents.iter().enumerate() {
            let next = accents.get(k + 1).copied().unwrap_or(end).min(end);
            let gap = next.saturating_sub(acc).max(1);
            let (a, _, _) = self.segments(gap);
            let start = acc as i64 + shift - if self.peak_on_step { a as i64 } else { 0 };
            let mut x = 0u32;
            let mut last = None;
            while x < gap {
                let pos = start + x as i64;
                if pos >= 0 && (pos as u32) < end {
                    let v = (self.level(x, gap) * 127.0).round() as u8;
                    if last != Some(v) {
                        out.push(Control { kind: 0xB0, channel, pos: pos as u32, number: self.cc.min(127), value: v });
                        last = Some(v);
                    }
                }
                x += step;
            }
        }
        out.sort_by_key(|c| c.pos);
        out.dedup_by_key(|c| c.pos);
        out
    }
}

// ---------------------------------------------------------------- patterns

/// A one-bar pattern of sixteenth-note dots: 0 no note, 1 a probable accent (played at higher
/// complexity), 2 a main accent.
pub type Dots = [u8; 16];

/// Studio's own rhythmic patterns for a player (name, dots).
pub fn patterns(kind: PatternKind) -> &'static [(&'static str, Dots)] {
    match kind {
        PatternKind::Bass => &[
            ("Roots", [2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0]),
            ("Push", [2, 0, 0, 1, 0, 0, 2, 0, 0, 0, 1, 0, 2, 0, 0, 0]),
            ("Eighths", [2, 0, 1, 0, 2, 0, 1, 0, 2, 0, 1, 0, 2, 0, 1, 0]),
            ("Offbeat", [0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 1]),
            ("Gallop", [2, 0, 1, 1, 2, 0, 1, 1, 2, 0, 1, 1, 2, 0, 1, 1]),
            ("Anticipate", [2, 0, 0, 0, 0, 0, 1, 0, 2, 0, 0, 1, 0, 0, 2, 0]),
        ],
        PatternKind::Keys => &[
            ("Whole", [2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]),
            ("Quarters", [2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0]),
            ("Backbeat", [1, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0]),
            ("Syncopated", [2, 0, 0, 2, 0, 0, 2, 0, 0, 0, 2, 0, 0, 1, 0, 0]),
            ("Eighths", [2, 0, 1, 0, 2, 0, 1, 0, 2, 0, 1, 0, 2, 0, 1, 0]),
            ("Skip", [2, 0, 0, 1, 0, 0, 2, 0, 1, 0, 2, 0, 0, 1, 0, 0]),
        ],
        PatternKind::Sequence => &[
            ("Rolling", [2, 1, 1, 1, 2, 1, 1, 1, 2, 1, 1, 1, 2, 1, 1, 1]),
            ("Octave Jumper", [2, 0, 1, 0, 2, 0, 1, 0, 2, 0, 1, 0, 2, 0, 1, 1]),
            ("Pulse", [2, 0, 2, 0, 2, 0, 2, 0, 2, 0, 2, 0, 2, 0, 2, 0]),
            ("Broken", [2, 0, 0, 1, 2, 0, 1, 0, 0, 1, 2, 0, 1, 0, 2, 0]),
        ],
        PatternKind::Envelope => &[
            ("Half Notes", [2, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0]),
            ("Quarters", [2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0]),
            ("Dotted", [2, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 2, 0, 0, 1]),
            ("Gate", [2, 0, 2, 0, 2, 0, 2, 0, 2, 0, 2, 0, 2, 0, 2, 0]),
        ],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternKind {
    Bass,
    Keys,
    Sequence,
    Envelope,
}

/// The onsets (ticks in the bar) a pattern plays at a complexity: the main accents, and the
/// probable ones once the complexity is over a half (more of them the higher it is).
pub fn onsets(dots: &Dots, complexity: f32, bar_t: u32, r: &mut Rng) -> Vec<(u32, bool)> {
    let mut v = Vec::new();
    for (i, d) in dots.iter().enumerate() {
        let at = i as u32 * SIXTEENTH;
        if at >= bar_t {
            break;
        }
        match d {
            2 => v.push((at, true)),
            1 if r.chance(((complexity - 0.35) * 1.6).clamp(0.0, 1.0)) => v.push((at, false)),
            _ => {}
        }
    }
    v
}

// ---------------------------------------------------------------- Synth Bass

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SynthBassStyle {
    Bass808,
    Pump,
    Sequenced,
}

/// Sequenced Bass Chord Response: how the sequence moves to a new chord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChordResponse {
    All,
    ShiftBassNotes,
    ShiftFirstNote,
    ShiftOnPhraseStart,
    ShiftNone,
}

impl ChordResponse {
    pub const ALL: [ChordResponse; 5] = [ChordResponse::All, ChordResponse::ShiftBassNotes, ChordResponse::ShiftFirstNote, ChordResponse::ShiftOnPhraseStart, ChordResponse::ShiftNone];
    pub fn name(self) -> &'static str {
        match self {
            ChordResponse::All => "All",
            ChordResponse::ShiftBassNotes => "Shift Bass Notes",
            ChordResponse::ShiftFirstNote => "Shift First Note in Chord",
            ChordResponse::ShiftOnPhraseStart => "Shift on Phrase Start",
            ChordResponse::ShiftNone => "Shift None",
        }
    }
}

/// The Synth Bass Player's own settings.
#[derive(Clone, Debug, PartialEq)]
pub struct SynthBass {
    /// the Main view XY pad: right adds notes beyond the root, up more octave moves
    pub xy_notes: f32,
    pub xy_octaves: f32,
    /// the note range (MIDI notes; drag between the handles to move both)
    pub range_low: u8,
    pub range_high: u8,
    /// Phrase Length: 0 Chord length, 1 one bar, 2 two, 3 four, 4 eight
    pub phrase_length: u8,
    /// Details: probabilities of rhythmic / octave / melodic variations over time
    pub rhythm: f32,
    pub octave: f32,
    pub melody: f32,
    /// 808 Bass: Repeats and Triplets
    pub repeats: f32,
    pub triplets: f32,
    /// Pump Bass: ducking Amount and Shape (sent as CC 11, Expression)
    pub pump_amount: f32,
    pub pump_shape: f32,
    /// Sequenced Bass: Add Notes (0 off, 1 eighth roots, 2 sixteenth roots), the accents
    pub add_notes: u8,
    pub acc_velocity: f32,
    pub acc_length: f32,
    pub acc_cc: f32,
    pub acc_cc_target: u8,
    pub chord_response: ChordResponse,
    /// the chosen rhythmic pattern (index into [`patterns`]); None: the style's own
    pub pattern: Option<usize>,
    pub slides: Slides,
    pub lfo: Lfo,
}

impl Default for SynthBass {
    fn default() -> Self {
        SynthBass {
            xy_notes: 0.3,
            xy_octaves: 0.3,
            range_low: 28,
            range_high: 52,
            phrase_length: 2,
            rhythm: 0.3,
            octave: 0.3,
            melody: 0.3,
            repeats: 0.2,
            triplets: 0.0,
            pump_amount: 0.6,
            pump_shape: 0.0,
            add_notes: 0,
            acc_velocity: 0.5,
            acc_length: 0.3,
            acc_cc: 0.0,
            acc_cc_target: 71,
            chord_response: ChordResponse::All,
            pattern: None,
            slides: Slides::default(),
            lfo: Lfo::default(),
        }
    }
}

fn phrase_ticks(code: u8, bar_t: u32) -> Option<u32> {
    match code {
        0 => None,
        1 => Some(bar_t),
        2 => Some(bar_t * 2),
        3 => Some(bar_t * 4),
        _ => Some(bar_t * 8),
    }
}

/// The pitch of a pitch class inside [low, high] nearest `near`.
fn in_range(pc: u8, low: u8, high: u8, near: i32) -> u8 {
    let (low, high) = (low.min(high.saturating_sub(11)), high.max(low + 11));
    let mut best = low as i32 + (pc as i32 - low as i32).rem_euclid(12);
    let mut p = best;
    while p <= high as i32 {
        if (p - near).abs() < (best - near).abs() {
            best = p;
        }
        p += 12;
    }
    best.clamp(0, 127) as u8
}

/// A Synth Bass performance.
#[allow(clippy::too_many_arguments)]
pub fn bass(style: SynthBassStyle, s: &SynthBass, complexity: f32, intensity: f32, region: &RegionSpec, chords: &[ChordAt], seed: u64, channel: u8) -> Perf {
    let bar_t = region.bar_ticks();
    let c = complexity.clamp(0.0, 1.0);
    let fallback = [ChordAt { pos: region.start, chord: Chord::major(0) }];
    let chords = if chords.is_empty() { &fallback[..] } else { chords };
    let kind = if style == SynthBassStyle::Sequenced { PatternKind::Sequence } else { PatternKind::Bass };
    let dots: Dots = match s.pattern {
        Some(i) => patterns(kind).get(i).map_or(patterns(kind)[0].1, |p| p.1),
        None => match style {
            SynthBassStyle::Bass808 => patterns(PatternKind::Bass)[5].1,
            SynthBassStyle::Pump => patterns(PatternKind::Bass)[3].1,
            SynthBassStyle::Sequenced => patterns(PatternKind::Sequence)[0].1,
        },
    };
    let mut perf = Perf::default();
    let phrase = phrase_ticks(s.phrase_length, bar_t);
    let mut r = Rng::derive(seed, 0x5B);
    let (low, high) = (s.range_low.min(s.range_high), s.range_high.max(s.range_low));
    // the phrase's figure: (onset in phrase, interval choice, accent)
    let base_v = 50.0 + 60.0 * intensity.clamp(0.0, 1.0);
    let mut prev_pitch: i32 = (low as i32 + high as i32) / 2 - 6;
    // Sequenced Bass: the sequence's intervals over the root, fixed per phrase
    let seq_ints: Vec<i32> = (0..16).map(|i| if i % 4 == 0 { 0 } else { *r.pick(&[0, 0, 12, 7, 0, 12, 10, 5]) }).collect();
    let mut phrase_root: Option<u8> = None;
    let mut last_chord: Option<Chord> = None;
    let mut first_after_change = false;
    let mut t_bar = region.start;
    let mut bar_idx = 0u32;
    while t_bar < region.end() {
        let ph_start = match phrase {
            Some(pl) => region.start + (t_bar - region.start) / pl * pl,
            None => chords.iter().rev().find(|x| x.pos <= t_bar).map_or(region.start, |x| x.pos.max(region.start)),
        };
        let at_phrase_start = t_bar == ph_start;
        let mut vr = Rng::derive(seed ^ ph_start as u64, 0xB1);
        // variations at the phrase start: rhythm, octave, melody
        let vary_rhythm = vr.chance(s.rhythm.clamp(0.0, 1.0) * 0.6);
        let vary_oct = vr.chance(s.octave.clamp(0.0, 1.0) * 0.6);
        let vary_mel = vr.chance(s.melody.clamp(0.0, 1.0) * 0.6);
        let mut steps = onsets(&dots, c, bar_t, &mut Rng::derive(seed ^ (ph_start as u64) << 1, 0x77));
        if vary_rhythm && !steps.is_empty() {
            let k = vr.below(steps.len() as u32) as usize;
            if !steps[k].1 {
                steps.remove(k);
            } else {
                let at = steps[k].0 + 2 * SIXTEENTH;
                if at < bar_t && !steps.iter().any(|x| x.0 == at) {
                    steps.push((at, false));
                }
            }
        }
        // Add Notes (Sequenced): extra roots on the 8th or 16th grid
        if style == SynthBassStyle::Sequenced && s.add_notes > 0 {
            let g = if s.add_notes == 1 { 2 * SIXTEENTH } else { SIXTEENTH };
            let mut at = 0;
            while at < bar_t {
                if !steps.iter().any(|x| x.0 == at) {
                    steps.push((at, false));
                }
                at += g;
            }
        }
        // a chord change inside the bar gets a note
        for chg in chords.iter().filter(|x| x.pos > t_bar && x.pos < t_bar + bar_t) {
            let at = chg.pos - t_bar;
            if !steps.iter().any(|x| x.0 == at) {
                steps.push((at, true));
            }
        }
        steps.sort_by_key(|x| x.0);
        for (k, &(at, main)) in steps.iter().enumerate() {
            let tick = t_bar + at;
            if tick >= region.end() {
                break;
            }
            let chord = chord_at(chords, tick).unwrap_or(Chord::major(0));
            if last_chord != Some(chord) {
                first_after_change = true;
                last_chord = Some(chord);
            }
            if at_phrase_start && at == 0 {
                phrase_root = Some(chord.root);
            }
            let next = steps.get(k + 1).map_or(t_bar + bar_t, |x| t_bar + x.0).min(region.end());
            // the root the note is built on
            let root_pc = match (style, s.chord_response) {
                (SynthBassStyle::Sequenced, ChordResponse::ShiftOnPhraseStart) => phrase_root.unwrap_or(chord.root),
                (SynthBassStyle::Sequenced, ChordResponse::ShiftFirstNote) if !first_after_change => phrase_root.unwrap_or(chord.root),
                (SynthBassStyle::Sequenced, ChordResponse::ShiftNone) => chords[0].chord.root,
                _ => chord.root,
            };
            if first_after_change && s.chord_response == ChordResponse::ShiftFirstNote {
                phrase_root = Some(chord.root);
            }
            first_after_change = false;
            let mut interval: i32 = 0;
            if style == SynthBassStyle::Sequenced {
                interval = seq_ints[(at / SIXTEENTH) as usize % 16];
                if s.chord_response == ChordResponse::ShiftBassNotes && interval != 0 {
                    // only the bass (root) notes follow the chord; the others keep the phrase's
                    interval += phrase_root.unwrap_or(chord.root) as i32 - root_pc as i32;
                }
            } else if !main || at != 0 {
                // XY across: notes beyond the root
                let x = (s.xy_notes + if vary_mel { 0.25 } else { 0.0 }).clamp(0.0, 1.0);
                if r.chance(x * 0.8) {
                    interval = *r.pick(&[chord.fifth() as i32, chord.third() as i32, 10, chord.fifth() as i32 - 12, 2]);
                }
            }
            // XY up: octave moves
            let oy = (s.xy_octaves + if vary_oct { 0.25 } else { 0.0 }).clamp(0.0, 1.0);
            if style != SynthBassStyle::Sequenced && !main && r.chance(oy * 0.7) {
                interval += 12;
            }
            let mut pc = ((root_pc as i32 + interval).rem_euclid(12)) as u8;
            if style == SynthBassStyle::Sequenced && s.chord_response == ChordResponse::ShiftNone {
                // unchanged, except notes that clash with the chord move to its nearest tone
                let pcs = chord.pitch_classes();
                if !pcs.contains(&pc) && !pcs.contains(&((pc + 1) % 12)) && !pcs.contains(&((pc + 11) % 12)) {
                    pc = chord.root;
                }
            }
            let near = prev_pitch + if interval >= 12 { 12 } else { 0 };
            let pitch = in_range(pc, low, high, near);
            prev_pitch = if interval >= 12 { pitch as i32 - 12 } else { pitch as i32 };
            let span = next - tick;
            let mut len = match style {
                SynthBassStyle::Bass808 => span.saturating_sub(20).max(60),
                SynthBassStyle::Pump => (span * 3 / 4).max(60),
                SynthBassStyle::Sequenced => (span * 4 / 5).max(40),
            };
            let mut v = base_v + if main { 8.0 } else { -6.0 } + if at == 0 { 4.0 } else { 0.0 };
            if style == SynthBassStyle::Sequenced {
                // the pattern's accents: velocity, note length, a CC
                let accent = if main { 1.0 } else { -1.0 };
                v += accent * 18.0 * s.acc_velocity.clamp(0.0, 1.0);
                len = ((len as f32) * (1.0 + 0.4 * accent * s.acc_length.clamp(0.0, 1.0))).max(30.0) as u32;
                if s.acc_cc > 0.0 {
                    let cv = 64.0 + accent * 63.0 * s.acc_cc.clamp(0.0, 1.0);
                    perf.controls.push(Control { kind: 0xB0, channel, pos: tick, number: s.acc_cc_target.min(127), value: cv.clamp(0.0, 127.0) as u8 });
                }
            }
            // 808: triplets replace a beat now and then; repeats echo a note a 16th later
            if style == SynthBassStyle::Bass808 && at % PPQ == 0 && span >= PPQ && r.chance(s.triplets.clamp(0.0, 1.0) * 0.5) {
                for j in 0..3 {
                    perf.notes.push(Note { channel, pitch, velocity: vel(v - j as f32 * 6.0), pos: tick + j * PPQ / 3, len: PPQ / 3 - 20, muted: false });
                }
                continue;
            }
            perf.notes.push(Note { channel, pitch, velocity: vel(v), pos: tick, len, muted: false });
            if style == SynthBassStyle::Bass808 && span >= 2 * SIXTEENTH && r.chance(s.repeats.clamp(0.0, 1.0) * 0.6) {
                let at2 = tick + SIXTEENTH;
                perf.notes.last_mut().unwrap().len = SIXTEENTH - 20;
                perf.notes.push(Note { channel, pitch, velocity: vel(v - 10.0), pos: at2, len: (next - at2).saturating_sub(20).max(40), muted: false });
            }
        }
        // Pump Bass: the ducking, a dip on every beat that recovers by the next
        if style == SynthBassStyle::Pump && s.pump_amount > 0.0 {
            let floor = 1.0 - s.pump_amount.clamp(0.0, 1.0);
            let mut b = 0;
            while b < bar_t && t_bar + b < region.end() {
                for i in 0..8 {
                    let x = i as f32 / 8.0;
                    let v = floor + (1.0 - floor) * shape(x, s.pump_shape, false);
                    perf.controls.push(Control { kind: 0xB0, channel, pos: t_bar + b + i * PPQ / 8, number: 11, value: (v * 127.0).round() as u8 });
                }
                b += PPQ;
            }
        }
        t_bar += bar_t;
        bar_idx += 1;
    }
    let _ = bar_idx;
    perf.notes = crate::finish(perf.notes, region);
    s.slides.apply(&mut perf, seed);
    s.lfo.apply_to_notes(&mut perf.notes, region);
    perf.controls.extend(s.lfo.events(region, chords, channel, seed));
    perf.controls.retain(|c| c.pos >= region.start && c.pos < region.end());
    perf.controls.sort_by_key(|c| c.pos);
    perf
}

// ---------------------------------------------------------------- Synth Keyboard

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SynthKeysStyle {
    RhythmicChords,
    ModulatedPad,
    SimplePad,
}

/// The Synth Keyboard Player's own settings.
#[derive(Clone, Debug, PartialEq)]
pub struct SynthKeys {
    /// Simple / Modulated Pad: Tie Notes (sustain pitches common to consecutive chords)
    pub tie_notes: bool,
    /// Simple / Modulated Pad: Note Start (-1 ahead .. 1 behind the beat) and Note Length (0..1)
    pub note_start: f32,
    pub note_length: f32,
    /// Modulated Pad: rhythmic variation of the envelope pattern
    pub phrase_variation: f32,
    /// Rhythmic Chords: rhythm and fill variation over time
    pub rhythm: f32,
    pub fills: f32,
    /// Modulated Pad / Rhythmic Chords: strum (-1 high to low .. 1 low to high)
    pub strum: f32,
    /// Modulated Pad: note velocity
    pub velocity: f32,
    /// Rhythmic Chords: Phrasing 0 staccato, 1 mixed long and short, 2 all long
    pub phrasing: u8,
    /// Rhythmic Chords: Note Length fine-tuning (-1..1)
    pub length_fine: f32,
    pub pattern: Option<usize>,
    pub envelope: Envelope,
    pub lfo: Lfo,
}

impl Default for SynthKeys {
    fn default() -> Self {
        SynthKeys { tie_notes: true, note_start: 0.0, note_length: 1.0, phrase_variation: 0.2, rhythm: 0.3, fills: 0.2, strum: 0.0, velocity: 0.7, phrasing: 1, length_fine: 0.0, pattern: None, envelope: Envelope::default(), lfo: Lfo::default() }
    }
}

/// A Synth Keyboard performance; `voicing` makes the right hand's notes for a chord (the
/// Keyboard Player's voicing rules).
#[allow(clippy::too_many_arguments)]
pub fn keys(style: SynthKeysStyle, s: &SynthKeys, complexity: f32, intensity: f32, region: &RegionSpec, chords: &[ChordAt], seed: u64, channel: u8, voicing: &mut dyn FnMut(&Chord) -> Vec<u8>) -> Perf {
    let bar_t = region.bar_ticks();
    let c = complexity.clamp(0.0, 1.0);
    let fallback = [ChordAt { pos: region.start, chord: Chord::major(0) }];
    let chords = if chords.is_empty() { &fallback[..] } else { chords };
    let mut perf = Perf::default();
    let mut r = Rng::derive(seed, 0x5C);
    let base_v = match style {
        SynthKeysStyle::ModulatedPad => 30.0 + 90.0 * s.velocity.clamp(0.0, 1.0),
        _ => 45.0 + 55.0 * intensity.clamp(0.0, 1.0),
    };
    let strum_step = |j: usize, n: usize| -> i64 {
        let st = s.strum.clamp(-1.0, 1.0);
        let k = if st >= 0.0 { j } else { n - 1 - j };
        (k as f32 * st.abs() * 40.0) as i64
    };
    match style {
        SynthKeysStyle::SimplePad | SynthKeysStyle::ModulatedPad => {
            // a held chord per chord; tied pitches carry on across the change
            let mut segs: Vec<(u32, u32, Chord)> = Vec::new();
            for (k, ch) in chords.iter().enumerate() {
                let a = ch.pos.max(region.start);
                let b = chords.get(k + 1).map_or(region.end(), |x| x.pos).min(region.end());
                if b > a && ch.pos < region.end() && b > region.start {
                    segs.push((a, b, ch.chord));
                }
            }
            if segs.is_empty() {
                segs.push((region.start, region.end(), chords[0].chord));
            }
            let shift = (s.note_start.clamp(-1.0, 1.0) * PPQ as f32 / 4.0) as i64;
            let mut held: Vec<(u8, usize)> = Vec::new(); // (pitch, index in notes)
            for (a, b, chord) in segs {
                let v = voicing(&chord);
                let span = b - a;
                let len = ((span as f32) * s.note_length.clamp(0.1, 1.0)) as u32;
                let start = (a as i64 + shift).clamp(region.start as i64, region.end() as i64 - 1) as u32;
                let mut now: Vec<(u8, usize)> = Vec::new();
                for (j, &p) in v.iter().enumerate() {
                    if s.tie_notes {
                        if let Some(&(_, idx)) = held.iter().find(|h| h.0 == p) {
                            // tie: lengthen the note already sounding
                            let n: &mut Note = &mut perf.notes[idx];
                            n.len = (start + len).saturating_sub(n.pos);
                            now.push((p, idx));
                            continue;
                        }
                    }
                    let pos = (start as i64 + strum_step(j, v.len())).max(region.start as i64) as u32;
                    perf.notes.push(Note { channel, pitch: p, velocity: vel(base_v + if j + 1 == v.len() { 6.0 } else { 0.0 }), pos, len: len.saturating_sub(pos - start).max(30), muted: false });
                    now.push((p, perf.notes.len() - 1));
                }
                held = now;
            }
            if style == SynthKeysStyle::ModulatedPad {
                // the envelope at each accent of the envelope pattern
                let dots = s.pattern.and_then(|i| patterns(PatternKind::Envelope).get(i)).map_or(patterns(PatternKind::Envelope)[1].1, |p| p.1);
                let mut acc = Vec::new();
                let mut t = region.start;
                while t < region.end() {
                    let mut vr = Rng::derive(seed ^ t as u64, 0xE7);
                    let mut on = onsets(&dots, c, bar_t, &mut vr);
                    if vr.chance(s.phrase_variation.clamp(0.0, 1.0)) && on.len() > 1 {
                        let k = 1 + vr.below(on.len() as u32 - 1) as usize;
                        on.remove(k);
                    }
                    acc.extend(on.into_iter().map(|o| t + o.0).filter(|x| *x < region.end()));
                    t += bar_t;
                }
                perf.controls.extend(s.envelope.events(&acc, region.end(), channel));
            }
        }
        SynthKeysStyle::RhythmicChords => {
            let dots = s.pattern.and_then(|i| patterns(PatternKind::Keys).get(i)).map_or(patterns(PatternKind::Keys)[3].1, |p| p.1);
            let mut t = region.start;
            while t < region.end() {
                let mut vr = Rng::derive(seed ^ t as u64, 0x3C);
                let mut on = onsets(&dots, c, bar_t, &mut vr);
                if vr.chance(s.rhythm.clamp(0.0, 1.0) * 0.6) && !on.is_empty() {
                    let k = vr.below(on.len() as u32) as usize;
                    let at = on[k].0 + SIXTEENTH;
                    if at < bar_t {
                        on[k].0 = at;
                    }
                }
                // a fill at the end of a four-bar phrase
                let bar_no = (t - region.start) / bar_t;
                if bar_no % 4 == 3 && vr.chance(s.fills.clamp(0.0, 1.0)) {
                    for k in 0..4 {
                        let at = bar_t - (4 - k) * SIXTEENTH;
                        if !on.iter().any(|o| o.0 == at) {
                            on.push((at, false));
                        }
                    }
                }
                for chg in chords.iter().filter(|x| x.pos > t && x.pos < t + bar_t) {
                    let at = chg.pos - t;
                    on.retain(|o| o.0.abs_diff(at) >= SIXTEENTH * 2 || o.0 == at);
                    if !on.iter().any(|o| o.0 == at) {
                        on.push((at, true));
                    }
                }
                on.sort_by_key(|o| o.0);
                for (k, &(at, main)) in on.iter().enumerate() {
                    let tick = t + at;
                    if tick >= region.end() {
                        break;
                    }
                    let chord = chord_at(chords, tick).unwrap_or(Chord::major(0));
                    let next = on.get(k + 1).map_or(t + bar_t, |o| t + o.0).min(region.end());
                    let span = next - tick;
                    let long = match s.phrasing {
                        0 => false,
                        1 => main,
                        _ => true,
                    };
                    let mut len = if long { span.saturating_sub(15) } else { (span / 2).min(SIXTEENTH * 3 / 2) };
                    len = ((len as f32) * (1.0 + 0.5 * s.length_fine.clamp(-1.0, 1.0))).clamp(30.0, span as f32) as u32;
                    let v = voicing(&chord);
                    for (j, &p) in v.iter().enumerate() {
                        let pos = (tick as i64 + strum_step(j, v.len())).max(region.start as i64) as u32;
                        perf.notes.push(Note { channel, pitch: p, velocity: vel(base_v + if main { 8.0 } else { -4.0 } + r.normal() * 2.0), pos, len, muted: false });
                    }
                }
                t += bar_t;
            }
        }
    }
    perf.notes = crate::finish(perf.notes, region);
    if style != SynthKeysStyle::SimplePad {
        if style == SynthKeysStyle::RhythmicChords {
            s.lfo.apply_to_notes(&mut perf.notes, region);
        }
        perf.controls.extend(s.lfo.events(region, chords, channel, seed));
    }
    perf.controls.retain(|c| c.pos >= region.start && c.pos < region.end());
    perf.controls.sort_by_key(|c| c.pos);
    perf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chords::parse_bars;

    fn region() -> RegionSpec {
        RegionSpec::new(0, 3840 * 4, (4, 4))
    }

    #[test]
    fn lfo_shapes() {
        let l = Lfo { amount: 1.0, ..Lfo::default() };
        assert!((l.wave_at(0.5, 0.0) - 1.0).abs() < 1e-3, "a triangle peaks in the middle");
        let saw = Lfo { symmetry: 0.99, ..l };
        assert!(saw.wave_at(0.9, 0.0) > 0.7, "symmetry 1 is an up saw");
        let sq = Lfo { wave: Wave::Square, symmetry: 0.5, ..l };
        assert_eq!(sq.wave_at(0.6, 0.0), 1.0, "pulse width wider than half");
        assert_eq!(shape(0.3, 0.0, true), 0.3, "S Curve does nothing at Curve 0");
        let ev = Lfo { amount: 1.0, rate: 1.0, sync: Sync::Beats, ..Lfo::default() }.events(&region(), &[], 0, 1);
        assert!(ev.iter().all(|c| c.number == 74 && c.kind == 0xB0), "CC 74 by default");
        assert!(ev.iter().any(|c| c.value > 120) && ev.iter().any(|c| c.value < 8));
        let r1 = Lfo { amount: 1.0, rate: 1.0, sync: Sync::Region, ..Lfo::default() }.events(&region(), &[], 0, 1);
        let peaks = r1.iter().filter(|c| c.value == 127).count();
        assert!(peaks >= 1 && peaks < 4, "one cycle per region");
        assert!(Lfo::default().events(&region(), &[], 0, 1).is_empty(), "Amount 0: off");
    }

    #[test]
    fn synth_bass_styles() {
        let ch = parse_bars("C Am F G", 0, 3840);
        let s = SynthBass::default();
        for st in [SynthBassStyle::Bass808, SynthBassStyle::Pump, SynthBassStyle::Sequenced] {
            let p = bass(st, &s, 0.5, 0.6, &region(), &ch, 3, 0);
            assert!(!p.notes.is_empty());
            assert!(p.notes.iter().all(|n| n.pitch >= s.range_low && n.pitch <= s.range_high + 12), "{st:?} inside the range");
        }
        let p = bass(SynthBassStyle::Pump, &s, 0.5, 0.6, &region(), &ch, 3, 0);
        assert!(p.controls.iter().any(|c| c.number == 11 && c.value < 70), "pumping");
        // Sequenced: Chord Response None keeps the first chord's root
        let none = SynthBass { chord_response: ChordResponse::ShiftNone, slides: Slides { amount: 0.0, ..Slides::default() }, ..s.clone() };
        let p = bass(SynthBassStyle::Sequenced, &none, 0.5, 0.6, &region(), &ch, 3, 0);
        let bar2: Vec<u8> = p.notes.iter().filter(|n| n.pos >= 3840 && n.pos < 7680).map(|n| n.pitch % 12).collect();
        assert!(bar2.contains(&0) && !bar2.contains(&9), "the A minor bar keeps C: {bar2:?}");
        let all = SynthBass { slides: Slides { amount: 0.0, ..Slides::default() }, ..s.clone() };
        let p = bass(SynthBassStyle::Sequenced, &all, 0.5, 0.6, &region(), &ch, 3, 0);
        assert!(p.notes.iter().any(|n| n.pos >= 3840 && n.pos < 7680 && n.pitch % 12 == 9), "All shifts to A");
    }

    #[test]
    fn slides_bend() {
        let mut perf = Perf { notes: vec![Note { channel: 0, pitch: 36, velocity: 90, pos: 0, len: 900, muted: false }, Note { channel: 0, pitch: 43, velocity: 90, pos: 960, len: 900, muted: false }], controls: vec![] };
        Slides { amount: 1.0, legato: 0, ..Slides::default() }.apply(&mut perf, 1);
        assert!(perf.controls.iter().any(|c| c.kind == 0xE0), "a bend");
        assert_eq!(perf.notes.len(), 2);
        let mut p2 = Perf { notes: perf.notes.clone(), controls: vec![] };
        Slides { amount: 1.0, legato: 100, ..Slides::default() }.apply(&mut p2, 1);
        assert_eq!(p2.notes.len(), 1, "legato 100: the second note isn't retriggered");
        let mut p3 = Perf { notes: perf.notes.clone(), controls: vec![] };
        Slides { amount: 1.0, mode: SlideMode::Overlap, ..Slides::default() }.apply(&mut p3, 1);
        assert!(p3.controls.is_empty() && p3.notes[0].pos + p3.notes[0].len > p3.notes[1].pos, "overlap");
        let mut p4 = Perf { notes: perf.notes.clone(), controls: vec![] };
        Slides { amount: 1.0, legato: 0, mono_mode: 1, ..Slides::default() }.apply(&mut p4, 1);
        assert!(p4.notes.iter().all(|n| n.channel >= 1) && p4.notes[0].channel != p4.notes[1].channel);
    }

    #[test]
    fn envelope_lengths() {
        let e = Envelope { attack: 1.0, hold: 1.0, decay: 1.0, length: EnvLength::AbsoluteClipped, ..Envelope::default() };
        assert_eq!(e.segments(960 * 4), (960, 960, 960));
        assert_eq!(e.segments(1500), (960, 540, 0));
        let s = Envelope { length: EnvLength::AbsoluteScaled, ..e };
        assert_eq!(s.segments(1440), (480, 480, 480));
        let f = Envelope { length: EnvLength::FixedAd, ..e };
        assert_eq!(f.segments(2400), (960, 480, 960));
        let r = Envelope { length: EnvLength::Relative, ..e };
        assert_eq!(r.segments(300), (100, 100, 100));
        assert!(Envelope { invert: true, ..r }.level(0, 300) > 0.9);
    }

    #[test]
    fn pads_tie_notes() {
        let ch = parse_bars("C Am", 0, 3840);
        let reg = RegionSpec::new(0, 7680, (4, 4));
        let mut v = |c: &Chord| c.pitch_classes().iter().map(|p| 60 + p).collect::<Vec<u8>>();
        let tied = keys(SynthKeysStyle::SimplePad, &SynthKeys::default(), 0.5, 0.5, &reg, &ch, 1, 0, &mut v);
        // C and E are common to C and Am: held over the change
        assert!(tied.notes.iter().any(|n| n.pitch == 60 && n.len > 3840));
        assert!(tied.controls.is_empty(), "Simple Pad has no LFO");
        let untied = keys(SynthKeysStyle::SimplePad, &SynthKeys { tie_notes: false, ..SynthKeys::default() }, 0.5, 0.5, &reg, &ch, 1, 0, &mut v);
        assert!(untied.notes.len() > tied.notes.len());
        let m = keys(SynthKeysStyle::ModulatedPad, &SynthKeys::default(), 0.5, 0.5, &reg, &ch, 1, 0, &mut v);
        assert!(m.controls.iter().any(|c| c.number == 1), "the envelope's CC");
        let rc = keys(SynthKeysStyle::RhythmicChords, &SynthKeys::default(), 0.5, 0.5, &reg, &ch, 1, 0, &mut v);
        assert!(rc.notes.len() > 8);
    }
}
