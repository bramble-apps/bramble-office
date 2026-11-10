//! Bramble Studio's software instruments: our own stand-ins for Logic's, written from scratch
//! (no Apple code, samples or data). Every sound is synthesized (physical models, additive, FM,
//! virtual analog); the Sampler plays the user's own content (SFZ, SoundFont, EXS).
//!
//! - `by_logic_name("Vintage Electric Piano", rate)` → the stand-in for a Logic plug-in name.
//! - `for_patch("Deluxe Classic", rate)` → an instrument voiced for a Logic library patch.
//! - `list()` → what is here.

pub mod bass;
pub mod clav;
pub mod drums;
pub mod dsp;
pub mod ensemble;
pub mod ep;
pub mod organ;
pub mod patches;
pub mod piano;
pub mod pluck;
pub mod poly;
pub mod retro;
pub mod sampler;
pub mod string;
pub mod wav;

pub use patches::{Engine, PatchSpec, patch};

use lg_dsp::Instrument;

// Working names (the user picks the final ones).
pub const NAME_EP: &str = "Vintage Keys EP";
pub const NAME_B3: &str = "Tonewheel B3";
pub const NAME_CLAV: &str = "Pickup Clav";
pub const NAME_RETRO: &str = "Retro Poly";
pub const NAME_PIANO: &str = "Studio Grand";
pub const NAME_DRUMS: &str = "Studio Drum Kit";
pub const NAME_BASS: &str = "Studio Bass";
pub const NAME_ENSEMBLE: &str = "Studio Ensemble";
pub const NAME_PLUCKS: &str = "Studio Plucks & Mallets";
pub const NAME_SAMPLER: &str = "Sampler";

/// One of our instruments and the Logic plug-ins it stands in for.
#[derive(Clone, Copy, Debug)]
pub struct InstrumentInfo {
    pub name: &'static str,
    pub logic_names: &'static [&'static str],
    pub description: &'static str,
}

static LIST: &[InstrumentInfo] = &[
    InstrumentInfo {
        name: NAME_EP,
        logic_names: &["Vintage Electric Piano", "Electric Piano"],
        description: "Tine and reed electric pianos: modelled tines/reeds through nonlinear pickups, with drive, phaser, tremolo and chorus.",
    },
    InstrumentInfo {
        name: NAME_B3,
        logic_names: &["Vintage B3 Organ", "Vintage B3", "Tonewheel Organ"],
        description: "Tonewheel organ: 91 wheels on real gear ratios, drawbars, percussion, key click, scanner vibrato/chorus, rotary speaker.",
    },
    InstrumentInfo {
        name: NAME_CLAV,
        logic_names: &["Vintage Clav", "Electric Clav"],
        description: "Clavinet-style struck string with two pickups, tone switches, wah, phaser and drive.",
    },
    InstrumentInfo {
        name: NAME_RETRO,
        logic_names: &["Retro Synth"],
        description: "Four-engine synth (analog, sync, wavetable, FM) with Logic's Retro Synth parameter list.",
    },
    InstrumentInfo {
        name: NAME_PIANO,
        logic_names: &["Piano", "Studio Piano"],
        description: "Modelled acoustic piano: inharmonic strings, felt hammers, unison beating, soundboard, sympathetic resonance.",
    },
    InstrumentInfo {
        name: NAME_DRUMS,
        logic_names: &[
            "Drum Kit Designer",
            "Drum Kits",
            "Drum Machine Designer",
            "Ultrabeat",
            "Drum Synth",
        ],
        description: "Synthesized acoustic and analog drum kits on the GM / Drum Kit Designer note map, velocity layers and round robin.",
    },
    InstrumentInfo {
        name: NAME_BASS,
        logic_names: &["Bass", "Studio Bass"],
        description: "Modelled electric and upright basses (finger, pick, slap, fretless, muted).",
    },
    InstrumentInfo {
        name: NAME_ENSEMBLE,
        logic_names: &[
            "Strings",
            "Studio Strings",
            "Horns",
            "Studio Horns",
            "Woodwind",
            "Voice",
            "Vintage Mellotron",
        ],
        description: "Sections and pads: strings, brass, woodwind, choir, tape-replay keyboard, harmonium.",
    },
    InstrumentInfo {
        name: NAME_PLUCKS,
        logic_names: &["Guitar", "Tuned Percussion"],
        description: "Guitars, harp, harpsichord, koto (waveguide strings) and mallets/bells (modal bars and tubes).",
    },
    InstrumentInfo {
        name: NAME_SAMPLER,
        logic_names: &[
            "Sampler",
            "Sampler (Multi-Sample)",
            "EXS24",
            "Quick Sampler (Single Sample)",
        ],
        description: "Multi-sample player for the user's SFZ, SoundFont (native or converted) and EXS instruments.",
    },
];

/// Our instruments.
pub fn list() -> &'static [InstrumentInfo] {
    LIST
}

fn with(engine: Engine, settings: &[(u32, f32)], rate: f32) -> Box<dyn Instrument> {
    build(
        &PatchSpec {
            engine,
            settings: settings.to_vec(),
            category: None,
        },
        rate,
    )
}

/// The stand-in for a Logic plug-in, by the name the project's AuCU chunk gives.
pub fn by_logic_name(plugin_name: &str, rate: f32) -> Option<Box<dyn Instrument>> {
    let inst: Box<dyn Instrument> = match plugin_name.trim() {
        "Vintage Electric Piano" | "Electric Piano" | "E-Piano" | NAME_EP => {
            Box::new(ep::VintageKeysEp::new(rate))
        }
        "Vintage B3 Organ" | "Vintage B3" | "Tonewheel Organ" | NAME_B3 => {
            Box::new(organ::TonewheelB3::new(rate))
        }
        "Church Organ" => with(
            Engine::TonewheelB3,
            &[(3, 8.0), (4, 6.0), (5, 8.0), (8, 6.0), (18, 0.0), (15, 0.0)],
            rate,
        ),
        "Vintage Clav" | "Electric Clav" | NAME_CLAV => Box::new(clav::VintageClav::new(rate)),
        "Retro Synth" | "ES2" | "ES1" | "ES P" | "ES M" | "ES E" | "EFM1" | "Alchemy"
        | "Sample Alchemy" | "Sculpture" | NAME_RETRO => Box::new(retro::RetroSynth::new(rate)),
        "Piano" | "Studio Piano" | NAME_PIANO => Box::new(piano::StudioPiano::new(rate)),
        "Drum Kit Designer" | "Drum Kits" | "Drum Kit" | NAME_DRUMS => {
            Box::new(drums::StudioDrumKit::new(rate))
        }
        "Ultrabeat" | "Drum Machine Designer" | "Drum Synth" => {
            Box::new(drums::StudioDrumKit::with_kit(rate, 4))
        }
        "Bass" | NAME_BASS => Box::new(bass::StudioBass::new(rate)),
        "Strings" | "Studio Strings" | NAME_ENSEMBLE => {
            Box::new(ensemble::StudioEnsemble::new(rate))
        }
        "Horns" | "Studio Horns" => with(Engine::Ensemble, &[(0, 3.0)], rate),
        "Woodwind" => with(Engine::Ensemble, &[(0, 10.0)], rate),
        "Voice" => with(Engine::Ensemble, &[(0, 7.0)], rate),
        "Vintage Mellotron" => with(Engine::Ensemble, &[(0, 11.0)], rate),
        "Guitar" | NAME_PLUCKS => Box::new(pluck::StudioPlucks::new(rate)),
        "Tuned Percussion" => with(Engine::Plucks, &[(0, 8.0)], rate),
        "Sampler"
        | "Sampler (Multi-Sample)"
        | "EXS24"
        | "Quick Sampler (Single Sample)"
        | "Quick Sampler" => Box::new(sampler::Sampler::new(Vec::new(), rate)),
        _ => return None,
    };
    Some(inst)
}

/// Builds the instrument a patch spec names, with its settings applied.
pub fn build(spec: &PatchSpec, rate: f32) -> Box<dyn Instrument> {
    let mut inst: Box<dyn Instrument> = match spec.engine {
        Engine::VintageKeysEp => Box::new(ep::VintageKeysEp::new(rate)),
        Engine::TonewheelB3 => Box::new(organ::TonewheelB3::new(rate)),
        Engine::Clav => Box::new(clav::VintageClav::new(rate)),
        Engine::RetroSynth => Box::new(retro::RetroSynth::new(rate)),
        Engine::Piano => Box::new(piano::StudioPiano::new(rate)),
        Engine::DrumKit => Box::new(drums::StudioDrumKit::new(rate)),
        Engine::Bass => Box::new(bass::StudioBass::new(rate)),
        Engine::Ensemble => Box::new(ensemble::StudioEnsemble::new(rate)),
        Engine::Plucks => Box::new(pluck::StudioPlucks::new(rate)),
    };
    for &(id, v) in &spec.settings {
        inst.set(id, v);
    }
    inst
}

/// An instrument voiced for a Logic library patch ("Deluxe Classic", "Classic Rock Organ"…).
pub fn for_patch(patch: &str, rate: f32) -> Option<Box<dyn Instrument>> {
    patches::patch(patch).map(|s| build(&s, rate))
}
