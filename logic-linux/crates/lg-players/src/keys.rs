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
    SynthRhythmic,
    SynthModulatedPad,
    SynthSimplePad,
}

impl KeysStyle {
    pub const ALL: [KeysStyle; 8] = [KeysStyle::Strummed, KeysStyle::FullChords, KeysStyle::BrokenChords, KeysStyle::Arpeggiated, KeysStyle::SupportingPad, KeysStyle::SynthRhythmic, KeysStyle::SynthModulatedPad, KeysStyle::SynthSimplePad];

    /// A Synth Keyboard Player style (played by [`crate::synth::keys`]).
    pub fn synth(self) -> Option<crate::synth::SynthKeysStyle> {
        use crate::synth::SynthKeysStyle as S;
        match self {
            KeysStyle::SynthRhythmic => Some(S::RhythmicChords),
            KeysStyle::SynthModulatedPad => Some(S::ModulatedPad),
            KeysStyle::SynthSimplePad => Some(S::SimplePad),
            _ => None,
        }
    }

    pub fn group(self) -> &'static str {
        self.id().split(" - ").next().unwrap_or("")
    }

    pub fn name(self) -> &'static str {
        self.id().split(" - ").nth(1).unwrap_or("")
    }

    pub fn default_bpm(self) -> f32 {
        match self {
            KeysStyle::SupportingPad | KeysStyle::SynthModulatedPad | KeysStyle::SynthSimplePad => 90.0,
            KeysStyle::SynthRhythmic => 122.0,
            KeysStyle::FullChords => 110.0,
            _ => 100.0,
        }
    }

    /// The right-hand voicings the style offers (Synth styles add their own).
    pub fn voicings(self) -> &'static [RhVoicing] {
        if self.synth().is_some() { &RhVoicing::ALL } else { &RhVoicing::ALL[..9] }
    }

    pub fn id(self) -> &'static str {
        match self {
            KeysStyle::Strummed => "Acoustic Piano - Strummed",
            KeysStyle::FullChords => "Acoustic Piano - Full Chords",
            KeysStyle::BrokenChords => "Acoustic Piano - Broken Chords",
            KeysStyle::Arpeggiated => "Acoustic Piano - Arpeggiated",
            KeysStyle::SupportingPad => "Keyboard - Supporting Pad",
            KeysStyle::SynthRhythmic => "Synth Keyboard - Rhythmic Chords",
            KeysStyle::SynthModulatedPad => "Synth Keyboard - Modulated Pad",
            KeysStyle::SynthSimplePad => "Synth Keyboard - Simple Pad",
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
    /// root, fifth and octave together
    RootFifthOctave,
    /// root, fifth and octave one after the other (Rhythmic Chords)
    Broken,
}

impl LeftHand {
    /// The Left-Hand Voicing pop-up (Off is the muted left hand).
    pub const VOICINGS: [(LeftHand, &'static str); 5] = [(LeftHand::Root, "Root Only"), (LeftHand::Octaves, "Root & Octave"), (LeftHand::RootFifth, "Root & Fifth"), (LeftHand::RootFifthOctave, "Root, Fifth & Octave"), (LeftHand::Broken, "Broken")];
}

/// The Left-Hand Style pop-up (index = [`KeysParams::lh_style`]); Steady 4ths is for Rhythmic
/// Chords only.
pub const LH_STYLES: [&str; 7] = ["Sustain Only", "Simple", "Moderate", "Complex", "Follow Right Hand", "Steady 8ths", "Steady 4ths"];
/// The Right-Hand Movement pop-up: how far the right hand may move up and down the keys.
pub const RH_MOVEMENT: [&str; 3] = ["Narrow", "Medium", "Wide"];
/// Block Chords (Full Chords) Phrasing / Rhythmic Chords Phrasing.
pub const PHRASING: [&str; 3] = ["Staccato", "Mixed Long and Short", "Long (Sustain Pedal)"];
/// Arpeggio Mode (Arpeggiated): index = arp_direction - 1.
pub const ARP_MODES: [&str; 7] = ["Up", "Down", "Up and Down", "Down and Up", "Random", "As Played", "Chords on Accents"];

/// The Right-Hand Voicing pop-up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RhVoicing {
    TwoVoice,
    TwoFixedInterval,
    TwoCommonTone,
    ThreeVoice,
    ThreeCommonTone,
    FullChord,
    FullFixedInversion,
    FourPlus,
    FourPlusFixed,
    // the Synth Keyboard Player's extra voicings
    OneVoice,
    ThreeWide,
    Stacked,
    StackedFixed,
    Shape1,
    Shape2,
    Shape3,
    RootOnly,
    RootFifth,
    RootFifthOctave,
}

impl RhVoicing {
    pub const ALL: [RhVoicing; 19] = [
        RhVoicing::TwoVoice,
        RhVoicing::TwoFixedInterval,
        RhVoicing::TwoCommonTone,
        RhVoicing::ThreeVoice,
        RhVoicing::ThreeCommonTone,
        RhVoicing::FullChord,
        RhVoicing::FullFixedInversion,
        RhVoicing::FourPlus,
        RhVoicing::FourPlusFixed,
        RhVoicing::OneVoice,
        RhVoicing::ThreeWide,
        RhVoicing::Stacked,
        RhVoicing::StackedFixed,
        RhVoicing::Shape1,
        RhVoicing::Shape2,
        RhVoicing::Shape3,
        RhVoicing::RootOnly,
        RhVoicing::RootFifth,
        RhVoicing::RootFifthOctave,
    ];

    pub fn name(self) -> &'static str {
        match self {
            RhVoicing::TwoVoice => "2 Voice",
            RhVoicing::TwoFixedInterval => "2 Voice (Fixed Interval)",
            RhVoicing::TwoCommonTone => "2 Voice (Common Tone)",
            RhVoicing::ThreeVoice => "3 Voice",
            RhVoicing::ThreeCommonTone => "3 Voice (Common Tone)",
            RhVoicing::FullChord => "Full Chord",
            RhVoicing::FullFixedInversion => "Full Chord (Fixed Inversion)",
            RhVoicing::FourPlus => "4+ Voice",
            RhVoicing::FourPlusFixed => "4+ Voice (Fixed Inversion)",
            RhVoicing::OneVoice => "1 Voice",
            RhVoicing::ThreeWide => "3 Voice Wide",
            RhVoicing::Stacked => "Stacked",
            RhVoicing::StackedFixed => "Stacked (Fixed Inversion)",
            RhVoicing::Shape1 => "Shape 1",
            RhVoicing::Shape2 => "Shape 2",
            RhVoicing::Shape3 => "Shape 3",
            RhVoicing::RootOnly => "Root Only",
            RhVoicing::RootFifth => "Root & Fifth",
            RhVoicing::RootFifthOctave => "Root, Fifth & Octave",
        }
    }
}

/// A keyboard region's settings.
#[derive(Clone, Debug, PartialEq)]
pub struct KeysParams {
    pub style: KeysStyle,
    pub complexity: f32,
    pub intensity: f32,
    pub fills: f32,
    /// how busy a fill is (Fill Complexity), 0..1
    pub fill_complexity: f32,
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
    /// Left-Hand Style (index into [`LH_STYLES`])
    pub lh_style: u8,
    /// Right-Hand Voicing (Full Chord uses `upper_voicing`, Logic's stored voicing style)
    pub rh_voicing: RhVoicing,
    /// Right-Hand Movement (index into [`RH_MOVEMENT`])
    pub rh_movement: u8,
    /// Block Chords Phrasing (index into [`PHRASING`])
    pub phrasing: u8,
    /// Block Chords: Steady 8ths fills the pattern with eighth notes
    pub steady_8ths: bool,
    /// Follow Rhythm Of ▸ Chords
    pub follow_chords: bool,
    /// The chosen rhythmic pattern (Patterns dialog; None: the style's own)
    pub pattern: Option<usize>,
    /// The Synth Keyboard Player's settings
    pub synth: crate::synth::SynthKeys,
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
            fill_complexity: 0.5,
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
            lh_style: 1,
            rh_voicing: RhVoicing::FullChord,
            rh_movement: 1,
            phrasing: 1,
            steady_8ths: false,
            follow_chords: true,
            pattern: None,
            synth: crate::synth::SynthKeys::default(),
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
    voice_ints(&colour(chord, style), chord.root, centre, prev, 0.6, false, style == 2)
}

/// Pitch classes common to most chords of a progression, most shared first.
pub fn common_tones(chords: &[ChordAt]) -> Vec<u8> {
    let mut count = [0u32; 12];
    let mut seen: Vec<Chord> = Vec::new();
    for c in chords {
        if !seen.contains(&c.chord) {
            seen.push(c.chord);
            for p in c.chord.pitch_classes() {
                count[p as usize] += 1;
            }
        }
    }
    let mut v: Vec<u8> = (0..12).filter(|p| count[*p as usize] > 0).collect();
    v.sort_by_key(|p| std::cmp::Reverse(count[*p as usize]));
    v
}

/// The right hand's notes for a chord in a Right-Hand Voicing.
pub fn voice_rh(kind: RhVoicing, chord: &Chord, upper_voicing: u8, centre: u8, prev: Option<&[u8]>, common: &[u8], movement: u8) -> Vec<u8> {
    let w = [1.4, 0.6, 0.2][movement.min(2) as usize];
    let third = chord.third();
    let fifth = chord.fifth();
    let sev = chord.seventh();
    let full: Vec<u8> = match sev {
        Some(s) => vec![0, third, fifth, s],
        None => vec![0, third, fifth],
    };
    let rel = |pcs: &[u8]| pcs.iter().map(|p| (p + 12 - chord.root) % 12).collect::<Vec<u8>>();
    match kind {
        RhVoicing::FullChord => voice_ints(&colour(chord, upper_voicing.clamp(1, 8)), chord.root, centre, prev, w, false, upper_voicing == 2),
        RhVoicing::FullFixedInversion => voice_ints(&full, chord.root, centre, None, w, true, false),
        RhVoicing::ThreeVoice => voice_ints(&[0, third, fifth], chord.root, centre, prev, w, false, false),
        RhVoicing::TwoVoice => {
            let v = voice_ints(&[0, third, fifth], chord.root, centre, prev, w, false, false);
            v[v.len().saturating_sub(2)..].to_vec()
        }
        RhVoicing::TwoFixedInterval => {
            // the root on top, a fourth below (the fifth): the same interval on every chord
            let top = voice_ints(&[0], chord.root, centre + 2, None, w, true, false)[0];
            vec![top - 5, top]
        }
        RhVoicing::TwoCommonTone | RhVoicing::ThreeCommonTone => {
            let n = if kind == RhVoicing::TwoCommonTone { 2 } else { 3 };
            let pcs: Vec<u8> = common.iter().take(n).copied().collect();
            if pcs.len() < n {
                return voice_ints(&full[..n.min(full.len())], chord.root, centre, prev, w, false, false);
            }
            voice_ints(&rel(&pcs), chord.root, centre, prev, w, false, false)
        }
        RhVoicing::FourPlus => voice_ints(&colour(chord, 7), chord.root, centre, prev, w, false, false),
        RhVoicing::FourPlusFixed => voice_ints(&colour(chord, 7), chord.root, centre, None, w, true, false),
        RhVoicing::OneVoice => {
            let v = voice_ints(&[0, third, fifth], chord.root, centre, prev, w, false, false);
            vec![*v.last().unwrap_or(&centre)]
        }
        RhVoicing::ThreeWide => {
            let r = voice_ints(&[0], chord.root, centre.saturating_sub(10), None, w, true, false)[0];
            vec![r, r + fifth + 12, (r + third + 24).min(108)]
        }
        RhVoicing::Stacked => voice_ints(&[0, 7, 14, third + 12], chord.root, centre, prev, w, false, false),
        RhVoicing::StackedFixed => voice_ints(&[0, 7, 14, third + 12], chord.root, centre, None, w, true, false),
        RhVoicing::Shape1 => voice_ints(&[0, 2, third, fifth], chord.root, centre, prev, w, false, false),
        RhVoicing::Shape2 => voice_ints(&[0, fifth, 14, 17], chord.root, centre, prev, w, false, false),
        RhVoicing::Shape3 => voice_ints(&[0, third, sev.unwrap_or(if chord.is_minor() { 10 } else { 11 }), 14, 21], chord.root, centre, prev, w, false, false),
        RhVoicing::RootOnly => voice_ints(&[0], chord.root, centre, None, w, true, false),
        RhVoicing::RootFifth => voice_ints(&[0, 7], chord.root, centre, None, w, true, false),
        RhVoicing::RootFifthOctave => voice_ints(&[0, 7, 12], chord.root, centre, None, w, true, false),
    }
}

/// A voicing of intervals over a root near `centre`, leading from `prev`; `fixed`: root
/// position only (the same inversion for every chord); `weight`: how strongly the hand stays
/// near the centre (the Right-Hand Movement).
pub fn voice_ints(ints: &[u8], root: u8, centre: u8, prev: Option<&[u8]>, weight: f32, fixed: bool, double_top: bool) -> Vec<u8> {
    let style = if double_top { 2 } else { 0 };
    let chord = Chord { root, mask: 1, bass: None };
    let mut best: Option<(f32, Vec<i32>)> = None;
    if ints.is_empty() {
        return Vec::new();
    }
    // try every inversion at the octaves around the centre
    for rot in 0..if fixed { 1 } else { ints.len() } {
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
            let mut cost = (mean - centre as f32 - 3.0).abs() * weight + (v[v.len() - 1] - v[0]) as f32 * 0.05;
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
    generate_perf(params, region, chords).notes
}

/// The performance with its controller events (sustain pedal, the Synth Player LFO and
/// envelope).
pub fn generate_perf(params: &KeysParams, region: &RegionSpec, chords: &[ChordAt]) -> crate::synth::Perf {
    let common = common_tones(chords);
    if let Some(ss) = params.style.synth() {
        let mut prev: Option<Vec<u8>> = None;
        let mut f = |c: &Chord| {
            let v = voice_rh(params.rh_voicing, c, params.upper_voicing, params.upper_position, prev.as_deref(), &common, params.rh_movement);
            prev = Some(v.clone());
            v
        };
        let mut perf = crate::synth::keys(ss, &params.synth, params.complexity, params.intensity, region, chords, params.seed, params.channel, &mut f);
        // the left hand: the root under each chord (Left-Hand Voicing / Style as below)
        if params.left_hand != LeftHand::Off {
            let fallback = [ChordAt { pos: region.start, chord: Chord::major(0) }];
            let chs = if chords.is_empty() { &fallback[..] } else { chords };
            let (_, beat_t) = beats_of(region.signature);
            for (k, c) in chs.iter().enumerate() {
                let a = c.pos.max(region.start);
                let b = chs.get(k + 1).map_or(region.end(), |x| x.pos).min(region.end());
                if b <= a {
                    continue;
                }
                let top_lh = params.lower_position.clamp(30, 90);
                let root = top_lh - 12 + (c.chord.bass_pc() + 12 - (top_lh - 12) % 12) % 12;
                let step = match params.lh_style {
                    5 => beat_t / 2,
                    6 => beat_t,
                    _ => b - a,
                };
                let mut t = a;
                let mut j = 0;
                while t < b {
                    let len = step.min(b - t).saturating_sub(10).max(30);
                    for p in lh_notes(params.left_hand, &c.chord, root, j) {
                        perf.notes.push(Note { channel: params.channel, pitch: p, velocity: vel(50.0 + 40.0 * params.intensity), pos: t, len, muted: false });
                    }
                    t += step;
                    j += 1;
                }
            }
            perf.notes = crate::finish(perf.notes, region);
        }
        return perf;
    }
    let mut controls: Vec<crate::synth::Control> = Vec::new();
    let bar_t = region.bar_ticks();
    let (_, beat_t) = beats_of(region.signature);
    let beat_t = match params.tempo.resolve(region.bpm, params.style.default_bpm()) {
        TempoFeel::Half => beat_t * 2,
        TempoFeel::Double => beat_t / 2,
        _ => beat_t,
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
    let pattern = params.pattern.and_then(|i| crate::synth::patterns(crate::synth::PatternKind::Keys).get(i));
    let rhythm: Vec<(u32, f32)> = match params.style {
        KeysStyle::FullChords if params.steady_8ths => (0..bar_t / half).map(|k| (k * half, if (k * half) % beat_t == 0 { 1.0 } else { 0.8 })).collect(),
        // a pattern from the Patterns dialog
        _ if pattern.is_some() && !matches!(params.style, KeysStyle::BrokenChords | KeysStyle::Arpeggiated) => {
            crate::synth::onsets(&pattern.unwrap().1, c, bar_t, &mut Rng::derive(params.seed, 23)).into_iter().map(|(at, main)| (at, if main { 1.0 } else { 0.5 })).collect()
        }
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
        _ => vec![(0, 1.0)],
    };
    for b in 0..region.bars() {
        let t0 = region.start + b * bar_t;
        let (kind, _, _) = region.section_at(t0);
        let i = (params.intensity + kind.energy()).clamp(0.0, 1.0) * 100.0;
        let base_v = 40.0 + 0.55 * i;
        // onsets: the rhythm plus every chord change
        let mut onsets: Vec<(u32, f32)> = rhythm.clone();
        if params.style == KeysStyle::Strummed && pattern.is_none() {
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
        // Follow Rhythm Of ▸ Track
        for &f in region.follow.iter().filter(|f| **f >= t0 && **f < t0 + bar_t) {
            let at = ((f - t0) + beat_t / 8) / (beat_t / 4) * (beat_t / 4);
            if at < bar_t && !onsets.iter().any(|o| o.0 == at) {
                onsets.push((at, 0.75));
            }
        }
        for chg in chords.iter().filter(|x| params.follow_chords && x.pos > t0 && x.pos < t0 + bar_t) {
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
                    let v = voice_rh(params.rh_voicing, &chord, params.upper_voicing.clamp(1, 8), params.upper_position, prev.as_deref(), &common, params.rh_movement);
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
                // Block Chords Phrasing: staccato, mixed long (main beats) and short, all long
                KeysStyle::FullChords => match params.phrasing {
                    0 => (span as f32 * 0.35) as u32,
                    1 if at % beat_t != 0 => (span as f32 * 0.45) as u32,
                    2 => span,
                    _ => (span as f32 * (0.85 - 0.25 * c)) as u32,
                },
                _ => (span as f32 * (0.85 - 0.25 * c)) as u32,
            };
            // the sustain pedal (Long): down after each chord change, up just before the next
            if params.style == KeysStyle::FullChords && params.phrasing == 2 && (at == 0 || chords.iter().any(|x| x.pos == tick)) {
                controls.push(crate::synth::Control { kind: 0xB0, channel: ch, pos: tick + 10, number: 64, value: 127 });
                controls.push(crate::synth::Control { kind: 0xB0, channel: ch, pos: to_change.saturating_sub(5).max(tick + 11), number: 64, value: 0 });
            }
            let top = voicing.len().saturating_sub(1);
            match params.style {
                KeysStyle::BrokenChords | KeysStyle::Arpeggiated => {
                    let n = voicing.len().max(1);
                    if params.style == KeysStyle::Arpeggiated && params.arp_direction == 7 && at % beat_t == 0 {
                        // Chords on Accents: the whole chord on the beat
                        for &p in &voicing {
                            notes.push(Note { channel: ch, pitch: p, velocity: vel(base_v), pos: tick, len: span.max(30), muted: false });
                        }
                        arp_k = 0;
                    } else {
                        let idx = arp_index(params.arp_direction, arp_k, n, &mut r, params.style == KeysStyle::BrokenChords);
                        arp_k += 1;
                        let p = voicing[idx];
                        let accent = if at % beat_t == 0 { 6.0 } else { 0.0 };
                        let len = if params.style == KeysStyle::Arpeggiated { (span * 2).min(to_change - tick) } else { legato.max(span) };
                        notes.push(Note { channel: ch, pitch: p, velocity: vel(base_v - 6.0 + accent + if idx == top { 6.0 } else { 0.0 }), pos: tick, len, muted: false });
                    }
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
                        notes.push(Note { channel: ch, pitch: voicing[vi], velocity: vel(base_v + accent), pos, len: legato.saturating_sub(off).max(30), muted: false });
                    }
                    // grace notes: a quick step into the top voice
                    if params.grace_notes > 0.0 && part >= 0.75 && tick >= region.start + 50 && r.chance(params.grace_notes * 0.35) {
                        let g = voicing[top].saturating_sub(if r.chance(0.5) { 1 } else { 2 });
                        notes.push(Note { channel: ch, pitch: g, velocity: vel(base_v - 16.0), pos: tick - 40, len: 40, muted: false });
                    }
                }
            }
            // left hand: on the chord's start and the bar's downbeat (moving: with the rhythm)
            let change_hit = at == 0 || chords.iter().any(|x| x.pos == tick);
            // the Left-Hand Style: when the left hand plays
            let lh_hit = match params.lh_style {
                0 => change_hit,
                2 => change_hit || at % beat_t == 0 && part >= 0.5,
                3 => change_hit || part >= 0.35,
                4 => true,
                5 | 6 => false,
                _ => change_hit || (params.left_hand == LeftHand::Moving && part >= 0.75) || (params.style == KeysStyle::Strummed && c > 0.3 && at == beats / 2 * beat_t + if beats % 2 == 1 { half } else { 0 }),
            };
            if lh_hit && params.left_hand != LeftHand::Off {
                let top_lh = params.lower_position.clamp(30, 90);
                let root = top_lh - 12 + (chord.bass_pc() + 12 - (top_lh - 12) % 12) % 12;
                let len = if params.lh_style == 0 { to_change - tick } else if params.lh_style == 4 { legato.max(30) } else { (to_change.min(next_on.max(tick + beat_t)) - tick).max(beat_t / 2).min(to_change - tick).max(30) };
                let lv = vel(base_v - 2.0 + if at == 0 { 4.0 } else { -8.0 });
                let mut lh = vec![root];
                match params.left_hand {
                    LeftHand::RootFifthOctave => {
                        lh.push(root + chord.fifth());
                        lh.push(root + 12);
                    }
                    LeftHand::Broken => {
                        // root, fifth and octave apart: root now, the others after
                        for (j, iv) in [chord.fifth(), 12].into_iter().enumerate() {
                            let at2 = tick + (j as u32 + 1) * half;
                            if at2 < to_change {
                                notes.push(Note { channel: ch, pitch: root + iv, velocity: lv, pos: at2, len: half.saturating_sub(10), muted: false });
                            }
                        }
                    }
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
                        notes.push(Note { channel: ch, pitch: p, velocity: lv, pos: tick, len, muted: false });
                    }
                }
            }
        }
        // a fill: a run up to the next chord's top voice at the end of a section
        let next = t0 + bar_t;
        if params.fills > 0.05 && next < region.end() && region.section_at(next).1 != region.section_at(t0).1 {
            let target = chord_at(chords, next).unwrap_or(Chord::major(0));
            let tv = voice(&target, params.upper_voicing.clamp(1, 8), params.upper_position, prev.as_deref());
            let run = ((2.0 + params.fills * 4.0) * (0.5 + params.fill_complexity.clamp(0.0, 1.0))).max(1.0) as u32;
            let step = beat_t / 4;
            let top = *tv.last().unwrap_or(&72);
            for k in 0..run {
                let at = next - (run - k) * step;
                let pcs = target.pitch_classes();
                let mut p = top as i32 - ((run - k) as i32) * 2;
                while !pcs.contains(&((p.rem_euclid(12)) as u8)) && p > 30 {
                    p -= 1;
                }
                notes.push(Note { channel: ch, pitch: p.clamp(21, 108) as u8, velocity: vel(base_v - 8.0 + k as f32 * 2.0), pos: at, len: step, muted: false });
            }
        }
    }
    // Steady 8ths / Steady 4ths: the left hand on an even grid
    if params.lh_style >= 5 && params.left_hand != LeftHand::Off {
        let step = if params.lh_style == 5 { half } else { beat_t };
        let mut t = region.start;
        let mut j = 0;
        while t < region.end() {
            let chord = chord_at(chords, t).unwrap_or(Chord::major(0));
            let top_lh = params.lower_position.clamp(30, 90);
            let root = top_lh - 12 + (chord.bass_pc() + 12 - (top_lh - 12) % 12) % 12;
            for p in lh_notes(params.left_hand, &chord, root, j) {
                notes.push(Note { channel: ch, pitch: p, velocity: vel(40.0 + 0.5 * params.intensity * 100.0 + if (t - region.start) % beat_t == 0 { 4.0 } else { -4.0 }), pos: t, len: step - 20, muted: false });
            }
            t += step;
            j += 1;
        }
    }
    let notes = crate::bass::humanize(notes, region, params.humanize * 0.6, params.feel, params.swing, params.swing_type, params.dynamics, params.seed ^ 0x55, beat_t);
    controls.retain(|c| c.pos >= region.start && c.pos < region.end());
    controls.sort_by_key(|c| c.pos);
    crate::synth::Perf { notes, controls }
}

/// The left hand's notes for a voicing (`j` counts the hits, for Broken).
fn lh_notes(lh: LeftHand, chord: &Chord, root: u8, j: usize) -> Vec<u8> {
    match lh {
        LeftHand::Off => vec![],
        LeftHand::RootFifth => vec![root, root + chord.fifth()],
        LeftHand::Octaves => vec![root, root.saturating_sub(12).max(24)],
        LeftHand::RootFifthOctave => vec![root, root + chord.fifth(), root + 12],
        LeftHand::Broken => vec![root + [0, chord.fifth(), 12][j % 3]],
        LeftHand::Chord => vec![root, root + chord.third(), root + chord.fifth()],
        _ => vec![root],
    }
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

    #[test]
    fn hands_and_voicings() {
        let c = Chord::parse("C").unwrap();
        let common = common_tones(&parse_bars("C Am F", 0, 3840));
        assert_eq!(common[0], 0, "C is in all three");
        assert_eq!(voice_rh(RhVoicing::TwoVoice, &c, 5, 64, None, &common, 1).len(), 2);
        let fixed = voice_rh(RhVoicing::TwoFixedInterval, &Chord::parse("F").unwrap(), 5, 64, None, &common, 1);
        assert_eq!(fixed[1] - fixed[0], 5);
        let wide = voice_rh(RhVoicing::ThreeWide, &c, 5, 64, None, &common, 1);
        assert!(wide[2] - wide[0] >= 24, "over two octaves");
        let inv = |ch: &str| voice_rh(RhVoicing::FullFixedInversion, &Chord::parse(ch).unwrap(), 5, 64, None, &common, 1);
        assert_eq!(inv("C")[0] % 12, 0);
        assert_eq!(inv("F")[0] % 12, 5, "root position every time");
        // Left-Hand Style Steady 8ths: the left hand on every eighth
        let region = RegionSpec::new(0, 3840, (4, 4));
        let chords = parse_bars("C", 0, 3840);
        let p = KeysParams { lh_style: 5, humanize: 0.0, feel: 0.0, lower_position: 55, ..KeysParams::default() };
        let n = generate(&p, &region, &chords);
        assert_eq!(n.iter().filter(|x| x.pitch <= 48 && x.pitch % 12 == 0).count(), 8);
        // Block Chords Long phrasing: the sustain pedal
        let perf = generate_perf(&KeysParams { style: KeysStyle::FullChords, phrasing: 2, ..KeysParams::default() }, &region, &chords);
        assert!(perf.controls.iter().any(|c| c.number == 64 && c.value == 127));
        // a Synth Keyboard style with a left hand
        let s = generate(&KeysParams { style: KeysStyle::SynthSimplePad, ..KeysParams::default() }, &region, &chords);
        assert!(s.iter().any(|x| x.pitch < 55));
    }
}
