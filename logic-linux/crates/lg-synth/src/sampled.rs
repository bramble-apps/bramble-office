//! The bundled sampled instruments (lg-sounds) played by the Sampler engine, and the choice
//! between them and the synthesized stand-ins: a sampled instrument is used when the sample
//! library is installed, the synthesized one otherwise.

use lg_dsp::{Instrument, Param, Plugin};
use lg_sounds::{Family, Sound};

use crate::patches::{Engine, PatchSpec};
use crate::sampler::{self, Sampler};

/// A bundled sampled instrument: the Sampler with one of lg-sounds' SFZs loaded.
pub struct SampledInstrument {
    inner: Sampler,
    pub sound: &'static Sound,
    /// Notes below this are not played (Studio Strings / Studio Horns keyswitches, C0–B0).
    pub ignore_below: u8,
}

impl SampledInstrument {
    /// Loads a bundled instrument by id; None when the library is not installed or none of its
    /// samples load. Samples are decoded here (on all cores) and shared with other instances.
    pub fn load(id: &str, rate: f32) -> Option<SampledInstrument> {
        let sound = lg_sounds::find(id)?;
        let root = lg_sounds::root()?;
        let zones = sampler::parse_sfz(sound.sfz, root).ok()?;
        if zones.is_empty() {
            return None;
        }
        let mut inner = Sampler::new(zones, rate);
        inner.name = sound.name.to_string();
        inner.set(13, sound.polyphony as f32);
        Some(SampledInstrument { inner, sound, ignore_below: 0 })
    }
}

impl Plugin for SampledInstrument {
    fn params(&self) -> &[Param] {
        self.inner.params()
    }
    fn get(&self, id: u32) -> f32 {
        self.inner.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        self.inner.set(id, v)
    }
    fn reset(&mut self) {
        self.inner.reset()
    }
    fn tail(&self) -> f32 {
        self.inner.tail()
    }
    /// A Logic project's plug-in floats belong to Logic's instrument, not to this one.
    fn load_floats(&mut self, _floats: &[f32]) {}
}

impl Instrument for SampledInstrument {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if pitch >= self.ignore_below {
            self.inner.note_on(pitch, velocity)
        }
    }
    fn note_off(&mut self, pitch: u8) {
        if pitch >= self.ignore_below {
            self.inner.note_off(pitch)
        }
    }
    fn controller(&mut self, cc: u8, value: u8) {
        self.inner.controller(cc, value)
    }
    fn pitch_bend(&mut self, value: i16) {
        self.inner.pitch_bend(value)
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.inner.render(left, right)
    }
}

fn boxed(id: &str, rate: f32) -> Option<Box<dyn Instrument>> {
    SampledInstrument::load(id, rate).map(|s| Box::new(s) as Box<dyn Instrument>)
}

/// The bundled sound for a Logic plug-in name ("Drum Kit Designer", "Studio Strings"…), if any.
pub fn sound_for_plugin(plugin_name: &str) -> Option<&'static Sound> {
    let id = match plugin_name.trim() {
        "Drum Kit Designer" | "Drum Kits" | "Drum Kit" | crate::NAME_DRUMS => "drums-modern",
        "Piano" | "Studio Piano" | crate::NAME_PIANO => "piano-grand",
        "Strings" | "Studio Strings" | crate::NAME_ENSEMBLE => "strings-section",
        "Horns" | "Studio Horns" => "brass-section",
        "Woodwind" => "woodwind-section",
        "Guitar" | crate::NAME_PLUCKS => "guitar-steel",
        "Bass" | crate::NAME_BASS => "bass-electric",
        "Tuned Percussion" => "vibraphone",
        _ => return None,
    };
    lg_sounds::find(id)
}

/// The sampled stand-in for a plug-in name, when the library is installed.
pub fn for_plugin(plugin_name: &str, rate: f32) -> Option<Box<dyn Instrument>> {
    let mut s = SampledInstrument::load(sound_for_plugin(plugin_name)?.id, rate)?;
    if matches!(plugin_name.trim(), "Studio Strings" | "Studio Horns") {
        s.ignore_below = 36;
    }
    Some(Box::new(s))
}

/// The bundled sound for a Logic patch (or Drummer kit) name, given the synthesized spec it maps
/// to: kits by name first, then the engine family and the words in the name.
pub fn sound_for_patch(name: &str, spec: &PatchSpec) -> Option<&'static Sound> {
    if let Some(k) = lg_sounds::for_kit(name) {
        return Some(k);
    }
    let low = name.trim().to_lowercase();
    let setting = |id: u32| {
        spec.settings
            .iter()
            .rev()
            .find(|(i, _)| *i == id)
            .map(|(_, v)| *v)
    };
    let model = setting(0).unwrap_or(0.0).round() as i32;
    let pick = |f: Family| lg_sounds::pick(f, &low);
    match spec.engine {
        Engine::DrumKit => {
            // timpani are voiced on the kit with a pitch drop
            if low.contains("timpani") {
                return lg_sounds::find("timpani");
            }
            // tuned membranes (taiko, surdo…) stay modelled
            if setting(10).is_some() {
                return None;
            }
            match model {
                3 => lg_sounds::find("drums-brush"),
                1 => lg_sounds::find("drums-vintage"),
                _ => lg_sounds::find("drums-modern"),
            }
        }
        Engine::Piano => {
            let m = setting(13).unwrap_or(0.0).round() as i32;
            // honky-tonk and the worn old upright stay modelled
            if m >= 4 { None } else { lg_sounds::find("piano-grand") }
        }
        Engine::Ensemble => match model {
            0 | 1 => pick(Family::Strings),
            15 => lg_sounds::find("violin-solo"),
            3 => pick(Family::Brass),
            9 => lg_sounds::find("flute"),
            10 => {
                if low.contains("sax") {
                    lg_sounds::find("tenor-sax")
                } else {
                    let s = pick(Family::Woodwind)?;
                    // the reed model's default is the clarinet
                    if s.id == "woodwind-section" && !low.contains("section") && !low.contains("ensemble") {
                        lg_sounds::find("clarinet")
                    } else {
                        Some(s)
                    }
                }
            }
            _ => None,
        },
        Engine::Plucks => match model {
            0 => lg_sounds::find("guitar-steel"),
            1 => lg_sounds::find("guitar-nylon"),
            2 | 3 => lg_sounds::find("guitar-electric"),
            4 => lg_sounds::find("harp"),
            7 => lg_sounds::find("strings-pizz"),
            8 => lg_sounds::find("vibraphone"),
            9 => lg_sounds::find("marimba"),
            10 => lg_sounds::find("xylophone"),
            11 => lg_sounds::find("glockenspiel"),
            14 => lg_sounds::find("tubular-bells"),
            15 => lg_sounds::find("kalimba"),
            _ => None,
        },
        Engine::Bass => match model {
            3 => lg_sounds::find("bass-upright"),
            _ => lg_sounds::find("bass-electric"),
        },
        _ => None,
    }
}

/// The sampled stand-in for a patch, when the library is installed.
pub fn for_patch(name: &str, spec: &PatchSpec, rate: f32) -> Option<Box<dyn Instrument>> {
    boxed(sound_for_patch(name, spec)?.id, rate)
}
