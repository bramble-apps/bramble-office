//! Bramble Studio's sound: the instruments and effects (our own stand-ins for Logic's, written
//! from scratch), all on one small interface so the engine, the mixer's slots and the plug-in
//! windows treat them alike.
//!
//! - Every unit renders 32-bit float stereo at the engine's sample rate, in blocks of any size.
//! - Parameters are listed in Logic's order (index for index with the plug-in's saved float
//!   block, the GAMETSPP state) so a project's settings and presets carry over.
//! - `registry` maps Logic's plug-in codes and names to ours (the matched-sounds idea).

pub mod registry;
pub mod routing;

/// One parameter, as the plug-in window and automation see it.
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    /// Index in Logic's saved parameter block (the GAMETSPP floats); our own extras after them.
    pub id: u32,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: Unit,
    /// Choices for a menu parameter (index = value).
    pub choices: &'static [&'static str],
    /// Stored state the window edits in its own view (Beat Breaker's slices, Step FX's steps,
    /// Delay Designer's taps): kept in the settings, left out of the Controls view.
    pub hidden: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    None,
    Db,
    Hz,
    Ms,
    Percent,
    Ratio,
    Semitones,
    Cents,
    Seconds,
    Octaves,
    Pan,
    OnOff,
    Menu,
}

impl Param {
    pub const fn new(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit) -> Param {
        Param { id, name, min, max, default, unit, choices: &[], hidden: false }
    }
    /// The same parameter, left out of the Controls view.
    pub const fn hide(mut self) -> Param {
        self.hidden = true;
        self
    }
    pub const fn menu(id: u32, name: &'static str, choices: &'static [&'static str], default: f32) -> Param {
        Param { id, name, min: 0.0, max: (choices.len() as f32) - 1.0, default, unit: Unit::Menu, choices, hidden: false }
    }
}

/// What instruments and effects share.
pub trait Plugin: Send {
    /// The unit's parameters (stable order).
    fn params(&self) -> &[Param];
    fn get(&self, id: u32) -> f32;
    fn set(&mut self, id: u32, value: f32);
    /// Clears delay lines, envelopes, voices (on stop and locate).
    fn reset(&mut self);
    /// Seconds the output keeps sounding after the input stops (reverb, delay tails), for bounces.
    fn tail(&self) -> f32 {
        0.0
    }
    /// The plug-in's own text for a value ("-6.0 dB", "Sawtooth"), when it has one (hosted
    /// plug-ins); None: the plug-in window formats it by the parameter's unit.
    fn value_text(&self, _id: u32, _value: f32) -> Option<String> {
        None
    }
    /// A hosted third-party plug-in (lg-host's Instance) for its editor and state; None for
    /// Studio's own units.
    fn hosted(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
    /// Sets parameters from Logic's saved float block (index = Param::id).
    fn load_floats(&mut self, floats: &[f32]) {
        let ids: Vec<u32> = self.params().iter().map(|p| p.id).collect();
        for id in ids {
            if let Some(v) = floats.get(id as usize) {
                self.set(id, *v);
            }
        }
    }
}

/// A software instrument: notes in, stereo out.
pub trait Instrument: Plugin {
    fn note_on(&mut self, pitch: u8, velocity: u8);
    fn note_off(&mut self, pitch: u8);
    /// MIDI controller (1 mod wheel, 7 volume, 10 pan, 11 expression, 64 sustain, 74 brightness…).
    fn controller(&mut self, _cc: u8, _value: u8) {}
    /// -8192..8191.
    fn pitch_bend(&mut self, _value: i16) {}
    /// Adds this block's sound into `left` and `right` (same length).
    fn render(&mut self, left: &mut [f32], right: &mut [f32]);
    /// Samples of delay the instrument adds (hosted plug-ins report theirs), for latency
    /// compensation.
    fn latency(&self) -> usize {
        0
    }
    /// The project tempo (tempo-synced LFOs, arpeggiators, sequencers).
    fn set_tempo(&mut self, _bpm: f64) {}
    /// The instrument itself, for editors that change more than parameters (a Sampler's zones,
    /// a Drum Machine Designer's pads): downcast to the concrete type.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }
}

/// An audio effect: processes the block in place.
pub trait Effect: Plugin {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
    /// The tempo, for tempo-synced delays and LFOs.
    fn set_tempo(&mut self, _bpm: f64) {}
    /// Samples of delay the effect adds (look-ahead limiters and gates), for latency
    /// compensation.
    fn latency(&self) -> usize {
        0
    }
    /// How much the effect is turning the level down right now, in dB (≥ 0): the Mixer's gain
    /// reduction meter (compressors, limiters).
    fn gain_reduction(&self) -> f32 {
        0.0
    }
    /// Where the song is (beats from the project start, tempo, playing): called before each
    /// block, for effects that follow the bar (Beat Breaker, Step FX, Remix FX's synced effects).
    fn set_transport(&mut self, _t: Transport) {}
    /// Live values the effect's window shows (meters, analysis results, detected pitch…), by
    /// name. Called from the interface, not the audio thread.
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        Vec::new()
    }
    /// A longer readout: a curve or spectrum the window draws (Match EQ's curves, Mastering
    /// Assistant's analysis, an analyzer), by name.
    fn curve(&self, _name: &str) -> Vec<f32> {
        Vec::new()
    }
}

/// The song position an effect follows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Transport {
    /// Beats (quarter notes) from the project start, at the block's first sample.
    pub beats: f64,
    pub bpm: f64,
    pub playing: bool,
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn gain_to_db(g: f32) -> f32 {
    20.0 * g.max(1e-9).log10()
}

/// The project's software-instrument tuning (Project Settings ▸ Tuning): cents per MIDI note,
/// added to equal temperament (all zero: A = 440 Hz equal tempered).
static TUNING: [std::sync::atomic::AtomicU32; 128] = [const { std::sync::atomic::AtomicU32::new(0) }; 128];

/// Sets the tuning table (cents per note, the global Tune included).
pub fn set_tuning(cents: &[f32; 128]) {
    for (k, c) in cents.iter().enumerate() {
        TUNING[k].store(c.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}

/// The tuning table's offset for a (possibly fractional) note, in cents.
pub fn tuning_cents(pitch: f32) -> f32 {
    let k = pitch.round().clamp(0.0, 127.0) as usize;
    f32::from_bits(TUNING[k].load(std::sync::atomic::Ordering::Relaxed))
}

pub fn midi_hz(pitch: f32) -> f32 {
    440.0 * 2f32.powf((pitch + tuning_cents(pitch) / 100.0 - 69.0) / 12.0)
}
