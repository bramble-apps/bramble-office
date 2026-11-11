//! Studio Strings and Studio Horns stand-ins: solo and section instruments. A section is made of
//! its instruments (Violins 1, Violins 2, Violas, Cellos, Double Basses; saxophones, trumpets,
//! trombones), each one a part of its own: the bundled sampled instrument when the sample
//! library is installed, the modelled ensemble otherwise. On top of the parts:
//!
//! - articulations (sustain, staccato, tremolo, trills, falls, scoops, crescendos, doits…),
//!   chosen in the header or with keyswitches (C0 upward, the octave can be moved); a fall
//!   keyswitch pressed while notes are held ends them with a fall;
//! - Auto Voice Split: the notes of a chord go to different instruments of the section (start
//!   with the lead voice, the bass voice, or by a split key; 8va / Drop 2 / Drop 2+4 voicings;
//!   Allow Unison); without it strings split by key range and horns layer;
//! - Monophonic with legato transitions, Dynamics via CC (absolute, catch, relative), a vibrato
//!   controller, natural key ranges, and one MIDI channel per instrument of the section.

use std::sync::LazyLock;

use crate::dsp::{self, Biquad, CHUNK, Rng};
use crate::ensemble::StudioEnsemble;
use crate::sampled::SampledInstrument;
use lg_dsp::{Instrument, Param, Plugin, Unit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Strings,
    Horns,
}

/// One instrument of a section.
#[derive(Clone, Copy, Debug)]
pub struct PartDef {
    pub name: &'static str,
    /// The bundled sampled sound, and the one for pizzicato.
    sample: &'static str,
    pizz: &'static str,
    /// The modelled ensemble's model and section size (the fallback).
    model: f32,
    size: f32,
    /// Natural range.
    pub lo: u8,
    pub hi: u8,
    /// MIDI channel (1-based) that plays this instrument alone.
    pub channel: u8,
    sax: bool,
}

const fn part(name: &'static str, sample: &'static str, pizz: &'static str, model: f32, size: f32, lo: u8, hi: u8, channel: u8, sax: bool) -> PartDef {
    PartDef { name, sample, pizz, model, size, lo, hi, channel, sax }
}

const V1: PartDef = part("Violins 1", "violins", "violins-pizz", 0.0, 4.0, 55, 103, 2, false);
const V2: PartDef = part("Violins 2", "violins", "violins-pizz", 1.0, 3.0, 55, 96, 3, false);
const VA: PartDef = part("Violas", "violas", "violins-pizz", 1.0, 3.0, 48, 91, 4, false);
const VC: PartDef = part("Cellos", "cellos", "cellos-pizz", 0.0, 3.0, 36, 79, 5, false);
const DB: PartDef = part("Double Basses", "basses", "basses-pizz", 0.0, 2.0, 24, 60, 6, false);
const AS1: PartDef = part("Alto Sax 1", "tenor-sax", "", 10.0, 1.0, 49, 84, 2, true);
const AS2: PartDef = part("Alto Sax 2", "tenor-sax", "", 10.0, 1.0, 49, 84, 3, true);
const TS1: PartDef = part("Tenor Sax 1", "tenor-sax", "", 10.0, 1.0, 44, 77, 4, true);
const TS2: PartDef = part("Tenor Sax 2", "tenor-sax", "", 10.0, 1.0, 44, 77, 5, true);
const BS: PartDef = part("Baritone Sax", "tenor-sax", "", 10.0, 1.0, 36, 69, 6, true);
const TP1: PartDef = part("Trumpet 1", "trumpet", "", 3.0, 1.0, 54, 94, 7, false);
const TP2: PartDef = part("Trumpet 2", "trumpet", "", 3.0, 1.0, 54, 91, 8, false);
const TP3: PartDef = part("Trumpet 3", "trumpet", "", 3.0, 1.0, 54, 89, 9, false);
const TP4: PartDef = part("Trumpet 4", "trumpet", "", 3.0, 1.0, 54, 86, 10, false);
const TB1: PartDef = part("Trombone 1", "trombone", "", 3.0, 1.0, 40, 77, 11, false);
const TB2: PartDef = part("Trombone 2", "trombone", "", 3.0, 1.0, 40, 74, 12, false);
const TB3: PartDef = part("Trombone 3", "trombone", "", 3.0, 1.0, 40, 72, 13, false);
const BTB: PartDef = part("Bass Trombone", "trombone", "", 3.0, 1.0, 34, 67, 14, false);

/// A preset: its name, its instruments from the lead down to the bass, whether it is a solo
/// (lead) instrument, and whether the solo-only articulations are available.
pub struct Preset {
    pub name: &'static str,
    pub parts: &'static [PartDef],
    pub solo: bool,
    full: bool,
}

pub const STRING_PRESETS: &[Preset] = &[
    Preset { name: "String Ensemble", parts: &[V1, V2, VA, VC, DB], solo: false, full: false },
    Preset { name: "Violins 1", parts: &[V1], solo: false, full: true },
    Preset { name: "Violins 2", parts: &[V2], solo: false, full: true },
    Preset { name: "Violas", parts: &[VA], solo: false, full: true },
    Preset { name: "Cellos", parts: &[VC], solo: false, full: true },
    Preset { name: "Double Basses", parts: &[DB], solo: false, full: false },
    Preset { name: "Solo Violin", parts: &[part("Violin", "violin-solo", "violins-pizz", 15.0, 1.0, 55, 103, 2, false)], solo: true, full: true },
    Preset { name: "Solo Viola", parts: &[part("Viola", "violas", "violins-pizz", 15.0, 1.0, 48, 91, 4, false)], solo: true, full: true },
    Preset { name: "Solo Cello", parts: &[part("Cello", "cellos", "cellos-pizz", 15.0, 1.0, 36, 79, 5, false)], solo: true, full: true },
    Preset { name: "Solo Double Bass", parts: &[part("Double Bass", "basses", "basses-pizz", 15.0, 1.0, 24, 60, 6, false)], solo: true, full: false },
];

pub const HORN_PRESETS: &[Preset] = &[
    Preset { name: "Horn Section", parts: &[TP1, TP2, AS1, TS1, TB1, BS], solo: false, full: false },
    Preset { name: "Saxophones", parts: &[AS1, AS2, TS1, TS2, BS], solo: false, full: false },
    Preset { name: "Trumpets", parts: &[TP1, TP2, TP3, TP4], solo: false, full: false },
    Preset { name: "Trombones", parts: &[TB1, TB2, TB3, BTB], solo: false, full: false },
    Preset { name: "Trumpet", parts: &[TP1], solo: true, full: true },
    Preset { name: "Trombone", parts: &[TB1], solo: true, full: true },
    Preset { name: "Alto Sax", parts: &[AS1], solo: true, full: true },
    Preset { name: "Tenor Sax", parts: &[TS1], solo: true, full: true },
    Preset { name: "Baritone Sax", parts: &[BS], solo: true, full: true },
];

/// An articulation: name, Logic's articulation ID, and whether only the solo / single
/// instruments have it. The keyswitches follow this order from C0.
pub struct Art {
    pub name: &'static str,
    pub id: u8,
    pub limited: bool,
}

const fn art(name: &'static str, id: u8, limited: bool) -> Art {
    Art { name, id, limited }
}

pub const STRING_ARTS: &[Art] = &[
    art("Sustain", 1, false),
    art("Tremolo", 31, false),
    art("Accented Sustain", 19, false),
    art("Fortepiano Slow", 25, false),
    art("Fortepiano Fast", 23, false),
    art("Spiccato", 3, false),
    art("Staccato", 2, false),
    art("Pizzicato", 28, false),
    art("Trill Semi", 29, false),
    art("Trill Whole", 30, false),
    art("Fall Long", 10, true),
    art("Fall Short", 12, true),
    art("Scoop Slow", 17, true),
    art("Scoop Fast", 16, true),
    art("Crescendo Slow", 21, true),
    art("Crescendo Fast", 20, true),
    art("Glissando Up", 27, true),
    art("Glissando Down", 26, true),
];

pub const HORN_ARTS: &[Art] = &[
    art("Sustain", 1, false),
    art("Growl", 14, true),
    art("Expressive Medium", 6, false),
    art("Expressive Long", 7, true),
    art("Expressive Short", 8, false),
    art("Staccato", 2, false),
    art("Marcato Short", 4, true),
    art("Fall Long", 10, false),
    art("Marcato Long", 5, true),
    art("Fall Medium", 11, false),
    art("Passionate", 9, true),
    art("Fall Short", 12, false),
    art("Doit", 13, false),
    art("Scoop", 16, true),
    art("Shake", 15, false),
];

pub const VOICINGS: &[&str] = &["Off", "8va", "Drop 2", "Drop 2+4"];
pub const SPLIT_MODES: &[&str] = &["Start with Lead Voice", "Start with Bass Voice", "Start by Key Split"];
pub const DYN_MODES: &[&str] = &["Controller (Absolute)", "Velocity & Controller (Catch)", "Velocity & Controller (Relative)"];
pub const VIB_MODES: &[&str] = &["Off", "Normal", "Slow", "Fast", "Gentle", "Expressive"];

fn names(list: Vec<&'static str>) -> &'static [&'static str] {
    Box::leak(list.into_boxed_slice())
}

fn build_params(kind: Kind) -> Vec<Param> {
    let (presets, arts) = match kind {
        Kind::Strings => (STRING_PRESETS, STRING_ARTS),
        Kind::Horns => (HORN_PRESETS, HORN_ARTS),
    };
    let mut v = vec![
        Param::menu(0, "Preset", names(presets.iter().map(|p| p.name).collect()), 0.0),
        Param::new(1, "Attack", 0.0, 1.0, 0.15, Unit::Ratio),
        Param::new(2, "Release", 0.0, 1.0, 0.3, Unit::Ratio),
        Param::new(10, "Volume", -40.0, 12.0, 0.0, Unit::Db),
        Param::new(11, "Monophonic", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(12, "Auto Voice Split", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::menu(13, "Articulation", names(arts.iter().map(|a| a.name).collect()), 0.0),
        Param::new(14, "Keyswitch Octave", -2.0, 2.0, 0.0, Unit::Octaves),
        Param::new(15, "Keyswitches", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(16, "Dynamics via CC", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(20, "Vibrato Controller", 0.0, 120.0, 1.0, Unit::None),
        Param::new(21, "Dynamic Controller", 0.0, 120.0, 11.0, Unit::None),
        Param::menu(22, "Dynamic Controller Mode", DYN_MODES, 0.0),
        Param::new(23, "Legato Transitions", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(24, "Release Samples", 0.0, 1.0, 0.7, Unit::Ratio),
        Param::new(25, "Pitch Bend Range", -12.0, 12.0, 2.0, Unit::Semitones),
        Param::new(26, "Extended Key Range", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(27, "Allow Unison", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(28, "Split Voicing", VOICINGS, 0.0),
        Param::menu(29, "Split Mode", SPLIT_MODES, 0.0),
        Param::new(30, "Split Key", 24.0, 96.0, 60.0, Unit::None),
        Param::new(31, "Inst via MIDI Channel", 0.0, 1.0, 1.0, Unit::OnOff),
    ];
    match kind {
        Kind::Strings => {
            v.push(Param::new(3, "Cutoff", 200.0, 20000.0, 20000.0, Unit::Hz));
            v.push(Param::new(4, "Resonance", -12.0, 12.0, 0.0, Unit::Db));
        }
        Kind::Horns => {
            v.push(Param::new(5, "Auto Vibrato", 0.0, 1.0, 0.3, Unit::Ratio));
            v.push(Param::new(6, "Humanize", 0.0, 1.0, 0.2, Unit::Ratio));
            v.push(Param::new(7, "Key Clicks", 0.0, 1.0, 0.3, Unit::Ratio));
            v.push(Param::menu(32, "Manual Vibrato Mode", VIB_MODES, 1.0));
        }
    }
    v.sort_by_key(|p| p.id);
    v
}

static STRINGS_PARAMS: LazyLock<Vec<Param>> = LazyLock::new(|| build_params(Kind::Strings));
static HORNS_PARAMS: LazyLock<Vec<Param>> = LazyLock::new(|| build_params(Kind::Horns));

pub fn params(kind: Kind) -> &'static [Param] {
    match kind {
        Kind::Strings => &STRINGS_PARAMS,
        Kind::Horns => &HORNS_PARAMS,
    }
}

/// What an articulation does to a note over time: (gain, pitch offset in semitones, extra
/// vibrato 0..1, the time after which the note ends by itself).
fn shape(kind: Kind, art: usize, t: f32) -> (f32, f32, f32, Option<f32>) {
    let sq = |x: f32| x * x;
    let wave = |hz: f32| dsp::sin_cycles((t * hz).fract());
    let flip = |hz: f32| (t * hz).fract() >= 0.5;
    match (kind, art) {
        (Kind::Strings, 1) => (1.0 - 0.6 * (0.5 + 0.5 * wave(13.0)), 0.0, 0.0, None),
        (Kind::Strings, 2) => (1.0 + 0.8 * (-t / 0.12).exp(), 0.0, 0.0, None),
        (Kind::Strings, 3) => (0.35 + 0.9 * (-t / 0.5).exp(), 0.0, 0.0, None),
        (Kind::Strings, 4) => (0.35 + 0.9 * (-t / 0.15).exp(), 0.0, 0.0, None),
        (Kind::Strings, 5) => (1.2, 0.0, 0.0, Some(0.08)),
        (Kind::Strings, 6) => (1.1, 0.0, 0.0, Some(0.17)),
        (Kind::Strings, 7) => (1.0, 0.0, 0.0, Some(0.04)),
        (Kind::Strings, 8) => (1.0, if flip(7.0) { 1.0 } else { 0.0 }, 0.0, None),
        (Kind::Strings, 9) => (1.0, if flip(7.0) { 2.0 } else { 0.0 }, 0.0, None),
        (Kind::Strings, 10) | (Kind::Horns, 7) => fall(t, 0.75, 9.0),
        (Kind::Strings, 11) | (Kind::Horns, 11) => fall(t, 0.28, 5.0),
        (Kind::Horns, 9) => fall(t, 0.45, 7.0),
        (Kind::Strings, 12) => (1.0, -2.0 * sq((1.0 - t / 0.4).max(0.0)), 0.0, None),
        (Kind::Strings, 13) | (Kind::Horns, 13) => (1.0, -2.0 * sq((1.0 - t / 0.13).max(0.0)), 0.0, None),
        (Kind::Strings, 14) => (0.15 + 0.85 * (t / 2.0).min(1.0), 0.0, 0.0, None),
        (Kind::Strings, 15) => (0.15 + 0.85 * (t / 0.7).min(1.0), 0.0, 0.0, None),
        (Kind::Strings, 16) => ((1.0 - sq(t / 0.7)).max(0.0), 7.0 * (t / 0.7).min(1.0), 0.0, Some(0.7)),
        (Kind::Strings, 17) => ((1.0 - sq(t / 0.7)).max(0.0), -7.0 * (t / 0.7).min(1.0), 0.0, Some(0.7)),
        (Kind::Horns, 1) => (1.0 - 0.4 * (0.5 + 0.5 * wave(27.0)), 0.0, 0.0, None),
        (Kind::Horns, 2) => (0.45 + 0.75 * (std::f32::consts::PI * (t / 1.0).min(1.0) * 0.5).sin(), 0.0, 0.2, None),
        (Kind::Horns, 3) => (0.4 + 0.8 * (std::f32::consts::PI * (t / 2.2).min(1.0) * 0.5).sin(), 0.0, 0.3, None),
        (Kind::Horns, 4) => (0.5 + 0.7 * (std::f32::consts::PI * (t / 0.35).min(1.0)).sin(), 0.0, 0.0, Some(0.38)),
        (Kind::Horns, 5) => (1.1, 0.0, 0.0, Some(0.15)),
        (Kind::Horns, 6) => (1.0 + 0.9 * (-t / 0.08).exp(), 0.0, 0.0, Some(0.28)),
        (Kind::Horns, 8) => (0.9 + 0.9 * (-t / 0.15).exp(), 0.0, 0.0, None),
        (Kind::Horns, 10) => (0.55 + 0.6 * (t / 1.5).min(1.0), -0.6 * sq((1.0 - t / 0.2).max(0.0)), (t / 1.2).min(1.0), None),
        (Kind::Horns, 12) => ((1.0 - sq(t / 0.4)).max(0.0), 7.0 * sq((t / 0.4).min(1.0)), 0.0, Some(0.4)),
        (Kind::Horns, 14) => (1.0, if flip(6.5) { 2.5 * (t / 0.3).min(1.0) } else { 0.0 }, 0.0, None),
        _ => (1.0, 0.0, 0.0, None),
    }
}

fn fall(t: f32, len: f32, depth: f32) -> (f32, f32, f32, Option<f32>) {
    let x = (t / len).min(1.0);
    ((1.0 - x * x).max(0.0), -depth * x * x, 0.0, Some(len))
}

/// Whether an articulation is a fall (its keyswitch triggers on held notes).
fn is_fall(kind: Kind, art: usize) -> bool {
    matches!((kind, art), (Kind::Strings, 10 | 11) | (Kind::Horns, 7 | 9 | 11))
}

struct Part {
    def: PartDef,
    inst: Box<dyn Instrument>,
    pizz: Option<Box<dyn Instrument>>,
    sampled: bool,
    /// The notes it is playing (and whether on the pizzicato instrument).
    notes: [u8; 8],
    count: usize,
    on_pizz: bool,
    // the articulation of its current note
    art: usize,
    age: f32,
    ended: bool,
    gain: f32,
    bend: f32,
    sent_bend: i16,
    /// Legato: a glide in from the last note (semitones left).
    glide: f32,
    humanize: (f32, f32),
    click: f32,
    last_note: Option<(u8, f32)>,
}

impl Part {
    fn new(def: PartDef, rate: f32, use_samples: bool) -> Part {
        let sampled = if use_samples { SampledInstrument::load(def.sample, rate) } else { None };
        let (inst, is_sampled): (Box<dyn Instrument>, bool) = match sampled {
            Some(mut s) => {
                s.set(12, 24.0);
                (Box::new(s), true)
            }
            None => {
                let mut e = StudioEnsemble::new(rate);
                e.set(0, def.model);
                e.set(4, def.size);
                e.set(8, 0.15);
                e.set(11, 24.0);
                e.set(15, 0.0);
                (Box::new(e), false)
            }
        };
        let pizz = if def.pizz.is_empty() || !is_sampled { None } else { SampledInstrument::load(def.pizz, rate).map(|s| Box::new(s) as Box<dyn Instrument>) };
        Part { def, inst, pizz, sampled: is_sampled, notes: [0; 8], count: 0, on_pizz: false, art: 0, age: 0.0, ended: false, gain: 1.0, bend: 0.0, sent_bend: 0, glide: 0.0, humanize: (1.0, 0.0), click: 0.0, last_note: None }
    }
    fn player(&mut self) -> &mut dyn Instrument {
        if self.on_pizz {
            if let Some(p) = self.pizz.as_mut() {
                return p.as_mut();
            }
        }
        self.inst.as_mut()
    }
    /// `model_art`: the modelled ensemble's own articulation for short notes (its envelopes).
    fn start(&mut self, note: u8, vel: u8, art: usize, pizz: bool, model_art: f32) {
        if !self.sampled {
            self.inst.set(13, model_art);
        }
        if self.count == self.notes.len() {
            let old = self.notes[0];
            self.player().note_off(old);
            self.notes.copy_within(1.., 0);
            self.count -= 1;
        }
        if self.count == 0 {
            self.on_pizz = pizz && self.pizz.is_some();
        }
        self.notes[self.count] = note;
        self.count += 1;
        self.art = art;
        self.age = 0.0;
        self.ended = false;
        self.player().note_on(note, vel);
    }
    fn stop(&mut self, note: u8) {
        if let Some(i) = self.notes[..self.count].iter().position(|n| *n == note) {
            self.notes.copy_within(i + 1..self.count, i);
            self.count -= 1;
            self.last_note = Some((note, 0.0));
            self.player().note_off(note);
        }
    }
    fn stop_all(&mut self) {
        while self.count > 0 {
            let n = self.notes[self.count - 1];
            self.stop(n);
        }
    }
    fn in_range(&self, note: u8) -> bool {
        (self.def.lo..=self.def.hi).contains(&note)
    }
}

/// How an incoming note is being played: (part, the note it plays there).
#[derive(Clone, Copy, Default)]
struct Route {
    list: [(u8, u8); 8],
    n: usize,
}

pub struct StudioSection {
    pub kind: Kind,
    params: crate::poly::Params,
    parts: Vec<Part>,
    preset: usize,
    rate: f32,
    /// Play the bundled sampled instruments when they are installed.
    use_samples: bool,
    routes: [Route; 128],
    held: [bool; 128],
    sustained: [bool; 128],
    sustain: bool,
    mono_note: Option<u8>,
    last_art: usize,
    // controllers
    bend: f32,
    vib_ctl: f32,
    vib_smooth: f32,
    dyn_ctl: f32,
    dyn_at_note: f32,
    dyn_caught: bool,
    dynamics: f32,
    last_vel: f32,
    expression: f32,
    volume: f32,
    pan: f32,
    filt: [Biquad; 2],
    click_f: [Biquad; 2],
    rng: Rng,
    sent_vib: u8,
}

impl StudioSection {
    pub fn new(kind: Kind, rate: f32) -> StudioSection {
        Self::with_samples(kind, rate, true)
    }
    /// `use_samples` false: the modelled parts only.
    pub fn with_samples(kind: Kind, rate: f32, use_samples: bool) -> StudioSection {
        let mut s = StudioSection {
            kind,
            use_samples,
            params: crate::poly::Params::new(params(kind)),
            parts: Vec::new(),
            preset: usize::MAX,
            rate,
            routes: [Route::default(); 128],
            held: [false; 128],
            sustained: [false; 128],
            sustain: false,
            mono_note: None,
            last_art: 0,
            bend: 0.0,
            vib_ctl: 0.0,
            vib_smooth: 0.0,
            dyn_ctl: 1.0,
            dyn_at_note: 1.0,
            dyn_caught: false,
            dynamics: 1.0,
            last_vel: 0.8,
            expression: 1.0,
            volume: 1.0,
            pan: 0.0,
            filt: [Biquad::default(); 2],
            click_f: [Biquad::default(); 2],
            rng: Rng::new(4711),
            sent_vib: 255,
        };
        for f in &mut s.click_f {
            f.bandpass(2600.0, 1.2, rate);
        }
        s.apply();
        s
    }
    pub fn strings(rate: f32) -> StudioSection {
        Self::new(Kind::Strings, rate)
    }
    pub fn horns(rate: f32) -> StudioSection {
        Self::new(Kind::Horns, rate)
    }

    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn on(&self, id: u32) -> bool {
        self.params.get(id) >= 0.5
    }
    pub fn presets(&self) -> &'static [Preset] {
        match self.kind {
            Kind::Strings => STRING_PRESETS,
            Kind::Horns => HORN_PRESETS,
        }
    }
    pub fn arts(&self) -> &'static [Art] {
        match self.kind {
            Kind::Strings => STRING_ARTS,
            Kind::Horns => HORN_ARTS,
        }
    }
    pub fn preset(&self) -> &'static Preset {
        &self.presets()[self.preset.min(self.presets().len() - 1)]
    }
    /// A section preset (several instruments)?
    pub fn is_section(&self) -> bool {
        self.parts.len() > 1
    }
    /// Whether the preset has an articulation (the solo-only ones are missing in sections).
    pub fn has_art(&self, art: usize) -> bool {
        self.arts().get(art).is_some_and(|a| !a.limited || self.preset().full)
    }
    /// The articulation of the note played last (it can differ from the header's when a
    /// keyswitch or a triggered fall chose it).
    pub fn last_played_articulation(&self) -> &'static str {
        self.arts()[self.last_art.min(self.arts().len() - 1)].name
    }
    /// The instruments of the preset, lead first.
    pub fn part_names(&self) -> Vec<&'static str> {
        self.parts.iter().map(|p| p.def.name).collect()
    }
    /// Whether the saxophones' Key Clicks apply to this preset.
    pub fn has_key_clicks(&self) -> bool {
        self.kind == Kind::Horns && self.parts.iter().any(|p| p.def.sax)
    }
    /// Which instrument is playing which notes now (for the window and for tests).
    pub fn playing(&self) -> Vec<(&'static str, Vec<u8>)> {
        self.parts.iter().map(|p| (p.def.name, p.notes[..p.count].to_vec())).collect()
    }

    fn apply(&mut self) {
        let rate = self.rate;
        let preset = (self.p(0).round() as usize).min(self.presets().len() - 1);
        if preset != self.preset {
            self.preset = preset;
            let samples = self.use_samples;
            self.parts = self.presets()[preset].parts.iter().map(|d| Part::new(*d, rate, samples)).collect();
            self.routes = [Route::default(); 128];
            self.held = [false; 128];
            self.sustained = [false; 128];
            self.mono_note = None;
            self.sent_vib = 255;
            if !self.has_art(self.p(13).round() as usize) {
                self.params.set(13, 0.0);
            }
        }
        let (atk, rel) = (self.p(1), self.p(2) * (0.15 + 0.85 * self.p(24) / 0.7).min(1.6));
        for part in &mut self.parts {
            let sampled = part.sampled;
            for inst in std::iter::once(&mut part.inst).chain(part.pizz.iter_mut()) {
                if sampled {
                    inst.set(1, 0.4 + atk * 8.0);
                    inst.set(4, 0.25 + rel * 4.0);
                } else {
                    inst.set(1, 0.01 + atk * 1.5);
                    inst.set(2, 0.04 + rel * 2.0);
                    inst.set(13, 0.0);
                }
            }
        }
        if self.kind == Kind::Strings {
            let (hz, res) = (self.p(3), self.p(4));
            for f in &mut self.filt {
                // a low-pass at the cutoff, with a cut or a boost around it
                f.lowpass(hz.min(rate * 0.45), 0.707 * 10f32.powf(res / 20.0), rate);
            }
        }
        self.volume = lg_dsp::db_to_gain(self.p(10));
    }

    /// The part that plays a MIDI channel alone (Inst via MIDI Channel), if any.
    fn part_for_channel(&self, chan: u8) -> Option<usize> {
        if !self.on(31) || chan == 0 || !self.is_section() {
            return None;
        }
        self.parts.iter().position(|p| p.def.channel == chan + 1)
    }

    /// The parts a new note goes to, with the note each plays.
    fn assign(&self, note: u8) -> Route {
        let mut r = Route::default();
        let k = self.parts.len();
        let extended = self.on(26);
        let add = |r: &mut Route, part: usize, n: i32| {
            let n = n.clamp(0, 127) as u8;
            if r.n < r.list.len() && (extended || self.parts[part].in_range(n) || k == 1 && extended) {
                r.list[r.n] = (part as u8, n);
                r.n += 1;
            }
        };
        if k == 1 {
            add(&mut r, 0, note as i32);
            return r;
        }
        if !self.on(12) {
            match self.kind {
                // strings: split by key range, nothing layered
                Kind::Strings => {
                    let pick = (0..k).rev().find(|i| note <= self.parts[*i].def.lo + 11).or_else(|| (0..k).find(|i| self.parts[*i].in_range(note))).unwrap_or(0);
                    let pick = if self.parts[pick].in_range(note) || extended { pick } else { (0..k).find(|i| self.parts[*i].in_range(note)).unwrap_or(pick) };
                    add(&mut r, pick, note as i32);
                }
                // horns: the instruments that share the key range play together
                Kind::Horns => {
                    for i in 0..k {
                        add(&mut r, i, note as i32);
                    }
                }
            }
            return r;
        }
        // Auto Voice Split: the chord's notes over the section's voices
        let mut chord: [u8; 16] = [0; 16];
        let mut m = 0;
        for n in (0..128u8).rev() {
            if (self.held[n as usize] || n == note) && m < 16 {
                chord[m] = n;
                m += 1;
            }
        }
        let j = chord[..m].iter().position(|n| *n == note).unwrap_or(0);
        let mode = self.p(29).round() as usize;
        let spread = |j: usize, m: usize, k: usize| -> usize {
            if m <= 1 {
                0
            } else if m >= k {
                (j * k / m).min(k - 1)
            } else {
                ((j * (k - 1)) as f32 / (m.max(3) - 1) as f32).round() as usize
            }
        };
        let voice = match mode {
            1 => k - 1 - spread(m - 1 - j, m, k),
            2 => {
                // below the split key: the bass and the lower middle; above: the lead and upper middle
                let split = self.p(30).round() as u8;
                let top = k.div_ceil(2);
                let upper: usize = chord[..m].iter().filter(|n| **n >= split).count();
                if note >= split {
                    spread(j, upper, top)
                } else {
                    top + spread(j - upper, m - upper, k - top).min(k - top - 1)
                }
            }
            _ => spread(j, m, k),
        };
        // the voicing: which octave each chord note is played in
        let voicing = self.p(28).round() as usize;
        let mut n = note as i32;
        if (voicing == 2 && j == 1) || (voicing == 3 && (j == 1 || j == 3)) {
            n -= 12;
        }
        // a free voice near the wanted one (each key one instrument)
        let free = |i: usize| self.parts[i].count == 0;
        let pick = if free(voice) { voice } else { (1..k).flat_map(|d| [voice.saturating_sub(d), (voice + d).min(k - 1)]).find(|i| free(*i)).unwrap_or(voice) };
        add(&mut r, pick, n);
        if voicing == 1 && j == 0 {
            // 8va: the top part doubled an octave higher
            let other = (0..k).find(|i| *i != pick && free(*i)).unwrap_or(pick);
            add(&mut r, other, n + 12);
        }
        if self.on(27) {
            // Allow Unison: the instruments left over double the note
            for i in 0..k {
                if i != pick && free(i) && !r.list[..r.n].iter().any(|x| x.0 as usize == i) {
                    add(&mut r, i, n);
                }
            }
        }
        if r.n == 0 {
            // (out of every range: the nearest instrument still plays it)
            let near = (0..k).min_by_key(|i| (self.parts[*i].def.lo as i32 - n).abs().min((self.parts[*i].def.hi as i32 - n).abs())).unwrap_or(0);
            r.list[0] = (near as u8, n.clamp(0, 127) as u8);
            r.n = 1;
        }
        r
    }

    fn start_note(&mut self, chan: u8, pitch: u8, velocity: u8) {
        let art = self.p(13).round() as usize;
        let art = if self.has_art(art) { art } else { 0 };
        self.last_art = art;
        let pizz = self.kind == Kind::Strings && art == 7;
        let v = velocity as f32 / 127.0;
        self.last_vel = v;
        self.dyn_at_note = self.dyn_ctl;
        self.dyn_caught = false;
        // Dynamics via CC, absolute: the controller alone sets the level
        let vel = if self.on(16) && self.p(22).round() as i32 == 0 { 100 } else { velocity };
        let route = match self.part_for_channel(chan) {
            Some(i) => {
                let mut r = Route::default();
                r.list[0] = (i as u8, pitch);
                r.n = 1;
                r
            }
            None => self.assign(pitch),
        };
        let human = if self.kind == Kind::Horns { self.p(6) } else { 0.0 };
        let clicks = if self.kind == Kind::Horns { self.p(7) } else { 0.0 };
        for (pi, n) in &route.list[..route.n] {
            let (hg, hp) = (1.0 + 0.18 * human * self.rng.bipolar(), 0.12 * human * self.rng.bipolar());
            let part = &mut self.parts[*pi as usize];
            // a fall (or doit) on the pitch just played joins on to it
            let joins = (is_fall(self.kind, art) || (self.kind == Kind::Horns && art == 12)) && part.last_note.is_some_and(|(ln, age)| ln == *n && age < 0.08);
            part.humanize = (hg, hp);
            if part.def.sax {
                part.click = clicks * (0.5 + 0.5 * v);
            }
            let model_art = match (self.kind, art) {
                (Kind::Strings, 5) => 2.0,
                (Kind::Strings, 6) | (Kind::Horns, 5) => 1.0,
                (Kind::Strings, 7) => 3.0,
                (Kind::Horns, 6) => 8.0,
                _ => 0.0,
            };
            part.start(*n, if joins { vel.min(70) } else { vel }, art, pizz, model_art);
        }
        self.routes[pitch as usize] = route;
    }

    fn stop_note(&mut self, pitch: u8) {
        let route = std::mem::take(&mut self.routes[pitch as usize]);
        for (pi, n) in &route.list[..route.n] {
            if let Some(part) = self.parts.get_mut(*pi as usize) {
                part.stop(*n);
            }
        }
    }

    fn key_off(&mut self, pitch: u8) {
        self.held[pitch as usize] = false;
        if self.on(11) {
            // Monophonic: back to a key still held, else silence
            if self.mono_note == Some(pitch) {
                self.stop_note(pitch);
                self.mono_note = None;
                if let Some(back) = (0..128u8).rev().find(|n| self.held[*n as usize]) {
                    self.legato_to(pitch, back, (self.last_vel * 127.0) as u8, 0);
                }
            }
            return;
        }
        self.stop_note(pitch);
    }

    /// Monophonic: the next note takes over; with Legato Transitions it slides in from the last.
    fn legato_to(&mut self, from: u8, to: u8, velocity: u8, chan: u8) {
        self.start_note(chan, to, velocity);
        self.mono_note = Some(to);
        if self.on(23) {
            let d = (from as f32 - to as f32).clamp(-12.0, 12.0);
            let route = self.routes[to as usize];
            for (pi, _) in &route.list[..route.n] {
                self.parts[*pi as usize].glide = d;
            }
        }
    }

    /// The level and brightness from the Dynamic Controller (Dynamics via CC).
    fn update_dynamics(&mut self) {
        if !self.on(16) {
            self.dynamics = 1.0;
            return;
        }
        let c = self.dyn_ctl;
        self.dynamics = match self.p(22).round() as i32 {
            0 => c,
            1 => {
                // Catch: the controller counts once it has reached the velocity's value
                if !self.dyn_caught && (c - self.last_vel).abs() < 0.06 {
                    self.dyn_caught = true;
                }
                if self.dyn_caught { c } else { self.last_vel }
            }
            _ => (self.last_vel + (c - self.dyn_at_note)).clamp(0.0, 1.0),
        };
    }
}

impl Plugin for StudioSection {
    fn params(&self) -> &[Param] {
        params(self.kind)
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            if id == 13 && !self.has_art(self.p(13).round() as usize) {
                self.params.set(13, 0.0);
            }
            self.apply();
            self.update_dynamics();
        }
    }
    fn reset(&mut self) {
        for p in &mut self.parts {
            p.inst.reset();
            if let Some(z) = p.pizz.as_mut() {
                z.reset();
            }
            p.count = 0;
            p.gain = 1.0;
            p.bend = 0.0;
            p.glide = 0.0;
            p.sent_bend = 0;
        }
        self.routes = [Route::default(); 128];
        self.held = [false; 128];
        self.sustained = [false; 128];
        self.sustain = false;
        self.mono_note = None;
        self.bend = 0.0;
        self.expression = 1.0;
        self.sent_vib = 255;
        self.apply();
    }
    fn tail(&self) -> f32 {
        2.5
    }
    fn value_text(&self, id: u32, value: f32) -> Option<String> {
        match id {
            20 | 21 => Some(match value.round() as i32 {
                0 => "Off".into(),
                120 => "Aftertouch".into(),
                n => format!("CC {n}"),
            }),
            30 => Some(crate::sampler_doc::note_name(value.round() as u8)),
            _ => None,
        }
    }
    /// A Logic project's plug-in floats belong to Logic's instrument, not to this one.
    fn load_floats(&mut self, _floats: &[f32]) {}
}

impl Instrument for StudioSection {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        self.note_on_ch(0, pitch, velocity);
    }
    fn note_off(&mut self, pitch: u8) {
        let pitch = pitch.min(127);
        if self.sustain && self.held[pitch as usize] {
            self.sustained[pitch as usize] = true;
            return;
        }
        self.key_off(pitch);
    }
    fn note_on_ch(&mut self, chan: u8, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        let pitch = pitch.min(127);
        // keyswitches: from C0, the octave can be moved; they don't sound
        if self.on(15) {
            let base = 24 + 12 * self.p(14).round() as i32;
            let k = pitch as i32 - base;
            if (0..self.arts().len() as i32).contains(&k) {
                let k = k as usize;
                if !self.has_art(k) {
                    return;
                }
                let sounding = self.parts.iter().any(|p| p.count > 0);
                if is_fall(self.kind, k) && sounding {
                    // trigger mode: the held notes end with the fall
                    self.last_art = k;
                    for p in &mut self.parts {
                        if p.count > 0 {
                            p.art = k;
                            p.age = 0.0;
                            p.ended = false;
                        }
                    }
                } else {
                    self.params.set(13, k as f32);
                }
                return;
            }
        }
        self.sustained[pitch as usize] = false;
        if self.routes[pitch as usize].n > 0 {
            self.stop_note(pitch);
        }
        if self.on(11) {
            let from = self.mono_note.take();
            if let Some(f) = from {
                self.stop_note(f);
                self.held[pitch as usize] = true;
                self.legato_to(f, pitch, velocity, chan);
                return;
            }
            self.mono_note = Some(pitch);
        }
        self.start_note(chan, pitch, velocity);
        self.held[pitch as usize] = true;
    }
    fn note_off_ch(&mut self, _chan: u8, pitch: u8) {
        self.note_off(pitch);
    }
    fn pressure(&mut self, value: u8) {
        let v = value as f32 / 127.0;
        if self.p(20).round() as i32 == 120 {
            self.vib_ctl = v;
        }
        if self.p(21).round() as i32 == 120 {
            self.dyn_ctl = v;
            self.update_dynamics();
        }
    }
    fn controller(&mut self, cc: u8, value: u8) {
        let v = value as f32 / 127.0;
        match cc {
            120 | 123 => {
                for p in &mut self.parts {
                    p.stop_all();
                }
                self.routes = [Route::default(); 128];
                self.held = [false; 128];
                self.sustained = [false; 128];
                self.mono_note = None;
                return;
            }
            64 => {
                let down = value >= 64;
                if self.sustain && !down {
                    for n in 0..128u8 {
                        if self.sustained[n as usize] {
                            self.sustained[n as usize] = false;
                            self.key_off(n);
                        }
                    }
                }
                self.sustain = down;
                return;
            }
            _ => {}
        }
        let mut used = false;
        if cc as i32 == self.p(20).round() as i32 {
            self.vib_ctl = v;
            used = true;
        }
        if self.on(16) && cc as i32 == self.p(21).round() as i32 {
            self.dyn_ctl = v;
            self.update_dynamics();
            used = true;
        }
        if used {
            return;
        }
        match cc {
            7 => self.volume = lg_dsp::db_to_gain(self.p(10)) * v * v,
            10 => self.pan = v * 2.0 - 1.0,
            11 => self.expression = v,
            1 => {}
            _ => {
                for p in &mut self.parts {
                    p.inst.controller(cc, value);
                }
            }
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.bend = value as f32 / 8192.0;
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let rate = self.rate;
        let kind = self.kind;
        let dt = n as f32 / rate;
        // vibrato: the automatic amount (horns) and the manual controller by its response mode
        let manual = if kind == Kind::Horns {
            let c = self.vib_ctl;
            match self.p(32).round() as i32 {
                0 => 0.0,
                2 => {
                    self.vib_smooth += (c - self.vib_smooth) * (dt / 0.6).min(1.0);
                    self.vib_smooth
                }
                3 => (c * 1.4).min(1.0),
                4 => c * 0.5,
                5 => c * c * 1.5,
                _ => c,
            }
        } else {
            self.vib_ctl
        };
        let auto = if kind == Kind::Horns { self.p(5) * 0.5 } else { 0.0 };
        let range = self.p(25);
        let dynamics = self.dynamics;
        let dyn_on = self.on(16);
        // level and brightness follow the dynamics
        let dyn_gain = if dyn_on { 0.12 + 0.88 * dynamics * dynamics } else { 1.0 };
        let bright = if dyn_on { (40.0 + 87.0 * dynamics) as u8 } else { 64 };
        let pan = (self.pan + 1.0) * 0.25 * std::f32::consts::PI;
        let (gl, gr) = (pan.cos() * std::f32::consts::SQRT_2, pan.sin() * std::f32::consts::SQRT_2);
        let out = self.volume * self.expression * if self.is_section() { 0.8 } else { 1.0 };
        let filter = kind == Kind::Strings && (self.p(3) < 19990.0 || self.p(4) != 0.0);
        let mut bl = [0.0f32; CHUNK];
        let mut br = [0.0f32; CHUNK];
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            let cdt = m as f32 / rate;
            let mut sum_l = [0.0f32; CHUNK];
            let mut sum_r = [0.0f32; CHUNK];
            for part in &mut self.parts {
                if let Some((_, age)) = part.last_note.as_mut() {
                    *age += cdt;
                }
                let (g, semis, vib, end) = if part.count > 0 { shape(kind, part.art, part.age) } else { (part.gain, 0.0, 0.0, None) };
                part.age += cdt;
                if let Some(t) = end {
                    if part.age >= t && !part.ended && part.count > 0 {
                        part.ended = true;
                        part.stop_all();
                    }
                }
                // the legato glide runs out in about a tenth of a second
                part.glide *= (-cdt / 0.035).exp();
                if part.glide.abs() < 0.01 {
                    part.glide = 0.0;
                }
                let bend = self.bend * range + semis + part.glide + part.humanize.1;
                let raw = (bend / 24.0 * 8192.0).clamp(-8192.0, 8191.0) as i16;
                if raw != part.sent_bend {
                    part.sent_bend = raw;
                    part.inst.pitch_bend(raw);
                    if let Some(z) = part.pizz.as_mut() {
                        z.pitch_bend(raw);
                    }
                }
                let v = (((auto + manual + vib).min(1.0)) * 127.0) as u8;
                if v != self.sent_vib {
                    part.inst.controller(1, v);
                }
                if dyn_on {
                    part.inst.controller(74, bright);
                }
                bl[..m].fill(0.0);
                br[..m].fill(0.0);
                part.inst.render(&mut bl[..m], &mut br[..m]);
                if let Some(z) = part.pizz.as_mut() {
                    z.render(&mut bl[..m], &mut br[..m]);
                }
                // the articulation's level, smoothed across the block
                let target = g * part.humanize.0;
                let step = (target - part.gain) / m as f32;
                for i in 0..m {
                    part.gain += step;
                    let mut l = bl[i] * part.gain;
                    let mut r = br[i] * part.gain;
                    if part.click > 1e-4 {
                        // saxophone keys and pads
                        let k = self.rng.bipolar() * part.click * 0.05;
                        l += self.click_f[0].run(k);
                        r += self.click_f[1].run(k);
                        part.click *= 0.9985;
                    }
                    sum_l[i] += l;
                    sum_r[i] += r;
                }
            }
            self.sent_vib = (((auto + manual).min(1.0)) * 127.0) as u8;
            for i in 0..m {
                let (mut l, mut r) = (sum_l[i] * dyn_gain, sum_r[i] * dyn_gain);
                if filter {
                    l = self.filt[0].run(l);
                    r = self.filt[1].run(r);
                }
                left[pos + i] += l * out * gl;
                right[pos + i] += r * out * gr;
            }
            pos += m;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }
    fn play(s: &mut StudioSection, secs: f32) -> Vec<f32> {
        let n = (48000.0 * secs) as usize;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            s.render(a, b);
        }
        l
    }
    fn who(s: &StudioSection) -> Vec<(&'static str, Vec<u8>)> {
        s.playing().into_iter().filter(|(_, n)| !n.is_empty()).collect()
    }

    #[test]
    fn auto_voice_split_spreads_a_chord_over_the_section() {
        let mut s = StudioSection::strings(48000.0);
        s.set(26, 1.0);
        assert_eq!(s.part_names(), vec!["Violins 1", "Violins 2", "Violas", "Cellos", "Double Basses"]);
        s.note_on(72, 100);
        assert_eq!(who(&s), vec![("Violins 1", vec![72])], "the first note: the lead voice");
        s.note_on(60, 100);
        s.note_on(48, 100);
        assert_eq!(who(&s), vec![("Violins 1", vec![72]), ("Violas", vec![60]), ("Double Basses", vec![48])]);
        s.note_off(60);
        assert_eq!(who(&s).len(), 2);
        // start with the bass voice
        let mut s = StudioSection::strings(48000.0);
        s.set(26, 1.0);
        s.set(29, 1.0);
        s.note_on(48, 100);
        assert_eq!(who(&s), vec![("Double Basses", vec![48])]);
        // by key split: below the split key the lower voices, above it the upper ones
        let mut s = StudioSection::strings(48000.0);
        s.set(26, 1.0);
        s.set(29, 2.0);
        s.note_on(50, 100);
        s.note_on(70, 100);
        let w = who(&s);
        assert_eq!(w[0], ("Violins 1", vec![70]));
        assert!(matches!(w[1].0, "Cellos" | "Double Basses"), "{w:?}");
    }

    #[test]
    fn unison_and_voicings() {
        let mut s = StudioSection::strings(48000.0);
        s.set(26, 1.0);
        s.set(27, 1.0);
        s.note_on(60, 100);
        assert_eq!(who(&s).len(), 5, "Allow Unison: every instrument plays the one key");
        let mut s = StudioSection::strings(48000.0);
        s.set(26, 1.0);
        s.note_on(60, 100);
        assert_eq!(who(&s).len(), 1, "without it one key is one instrument");
        // 8va doubles the top part an octave higher
        let mut s = StudioSection::strings(48000.0);
        s.set(26, 1.0);
        s.set(28, 1.0);
        s.note_on(72, 100);
        let notes: Vec<u8> = who(&s).into_iter().flat_map(|x| x.1).collect();
        assert_eq!(notes, vec![72, 84]);
        // Drop 2: the second note from the top an octave lower
        let mut s = StudioSection::strings(48000.0);
        s.set(26, 1.0);
        s.set(28, 2.0);
        s.note_on(72, 100);
        s.note_on(67, 100);
        let notes: Vec<u8> = who(&s).into_iter().flat_map(|x| x.1).collect();
        assert!(notes.contains(&72) && notes.contains(&55), "{notes:?}");
    }

    #[test]
    fn without_the_split_strings_divide_by_range_and_horns_layer() {
        let mut s = StudioSection::strings(48000.0);
        s.set(12, 0.0);
        s.set(15, 0.0);
        s.note_on(30, 100);
        assert_eq!(who(&s), vec![("Double Basses", vec![30])]);
        s.note_on(90, 100);
        assert_eq!(who(&s).len(), 2, "one instrument per key: nothing layered");
        let mut h = StudioSection::horns(48000.0);
        h.set(12, 0.0);
        h.note_on(60, 100);
        assert!(who(&h).len() >= 4, "the horns sharing the range play together: {:?}", who(&h));
    }

    #[test]
    fn channels_play_single_instruments() {
        let mut s = StudioSection::strings(48000.0);
        s.note_on_ch(4, 60, 100); // channel 5: the cellos
        assert_eq!(who(&s), vec![("Cellos", vec![60])]);
        s.note_off(60);
        s.note_on_ch(11, 60, 100); // a channel the section doesn't use: the full section
        assert_eq!(who(&s), vec![("Violins 1", vec![60])]);
        s.note_off(60);
        s.set(31, 0.0);
        s.note_on_ch(4, 72, 100);
        assert_eq!(who(&s), vec![("Violins 1", vec![72])], "off: every channel plays the section");
        let h = StudioSection::horns(48000.0);
        assert_eq!(h.part_for_channel(6), Some(0), "channel 7: Trumpet 1");
    }

    #[test]
    fn keyswitches_choose_articulations() {
        let mut s = StudioSection::strings(48000.0);
        s.note_on(30, 100); // F#0: Staccato
        assert_eq!(s.get(13), 6.0);
        assert!(who(&s).is_empty(), "a keyswitch makes no sound");
        s.note_on(60, 100);
        assert_eq!(s.last_played_articulation(), "Staccato");
        play(&mut s, 0.3);
        assert!(who(&s).is_empty(), "staccato ends by itself");
        // the solo-only articulations aren't in the full section
        s.note_on(34, 100);
        assert_eq!(s.get(13), 6.0);
        s.set(0, 6.0);
        s.note_on(34, 100);
        assert_eq!(s.get(13), 10.0, "Fall Long on the solo violin");
        // the octave can be moved
        s.set(14, 1.0);
        s.note_on(36, 100);
        assert_eq!(s.get(13), 0.0);
        // a fall keyswitch while a note sounds ends that note with the fall
        s.note_on(72, 100);
        play(&mut s, 0.2);
        s.note_on(36 + 10, 100);
        assert_eq!(s.get(13), 0.0, "trigger mode: the header's articulation stays");
        assert_eq!(s.last_played_articulation(), "Fall Long");
        play(&mut s, 1.0);
        assert!(who(&s).is_empty());
    }

    #[test]
    fn natural_ranges_and_extended_key_range() {
        let mut s = StudioSection::strings(48000.0);
        s.set(0, 1.0);
        s.set(15, 0.0);
        s.note_on(30, 100);
        assert!(who(&s).is_empty() || who(&s)[0].1 != vec![30], "below the violins' range");
        s.set(26, 1.0);
        s.note_on(31, 100);
        assert_eq!(who(&s), vec![("Violins 1", vec![31])]);
    }

    #[test]
    fn monophonic_and_dynamics_via_cc() {
        let mut s = StudioSection::strings(48000.0);
        s.set(0, 6.0);
        s.set(11, 1.0);
        s.note_on(60, 100);
        s.note_on(64, 100);
        assert_eq!(who(&s), vec![("Violin", vec![64])]);
        assert!(s.parts[0].glide != 0.0, "legato: it slides in from the last note");
        s.note_off(64);
        assert_eq!(who(&s), vec![("Violin", vec![60])], "back to the key still held");
        s.note_off(60);
        assert!(who(&s).is_empty());
        // dynamics
        let level = |ctl: Option<u8>, mode: f32| {
            let mut s = StudioSection::strings(48000.0);
            s.set(16, 1.0);
            s.set(22, mode);
            if let Some(c) = ctl {
                s.controller(11, c);
            }
            s.note_on(60, 100);
            play(&mut s, 0.5);
            rms(&play(&mut s, 0.3))
        };
        assert!(level(Some(20), 0.0) < level(Some(127), 0.0) * 0.5, "absolute: the controller sets the level");
        let mut s = StudioSection::strings(48000.0);
        s.set(16, 1.0);
        s.set(22, 1.0);
        s.note_on(60, 100);
        s.controller(11, 10);
        assert!((s.dynamics - 100.0 / 127.0).abs() < 0.01, "catch: nothing until the controller gets there");
        s.controller(11, 100);
        s.controller(11, 30);
        assert!(s.dynamics < 0.3);
        s.set(22, 2.0);
        s.controller(11, 60);
        s.note_on(62, 64);
        s.controller(11, 80);
        assert!((s.dynamics - (64.0 + 20.0) / 127.0).abs() < 0.01, "relative: an offset from the velocity");
        // off: the controller is plain expression
        let mut s = StudioSection::strings(48000.0);
        s.controller(11, 0);
        assert_eq!(s.dynamics, 1.0);
        assert_eq!(s.expression, 0.0);
    }

    #[test]
    fn every_articulation_plays() {
        for kind in [Kind::Strings, Kind::Horns] {
            let mut s = StudioSection::new(kind, 48000.0);
            s.set(0, s.presets().len() as f32 - 3.0);
            for a in 0..s.arts().len() {
                s.set(13, a as f32);
                assert_eq!(s.get(13), a as f32, "{}", s.arts()[a].name);
                s.note_on(62, 100);
                let l = play(&mut s, 0.25);
                assert!(l.iter().all(|x| x.is_finite()));
                assert!(rms(&l) > 1e-4, "{:?} {} is silent", kind, s.arts()[a].name);
                s.note_off(62);
                play(&mut s, 0.4);
            }
        }
    }

    #[test]
    fn horns_have_key_clicks_only_for_saxophones() {
        let mut h = StudioSection::horns(48000.0);
        assert!(h.has_key_clicks());
        h.set(0, 2.0);
        assert!(!h.has_key_clicks(), "trumpets");
        h.set(0, 7.0);
        assert!(h.has_key_clicks());
        assert_eq!(h.part_names(), vec!["Tenor Sax 1"]);
    }
}
