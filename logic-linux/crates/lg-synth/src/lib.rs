//! Bramble Studio's software instruments: our own stand-ins for Logic's, written from scratch
//! (no Apple code, samples or data). Every sound is synthesized (physical models, additive, FM,
//! virtual analog); the Sampler plays the user's own content (SFZ, SoundFont, EXS).
//!
//! - `by_logic_name("Vintage Electric Piano", rate)` → the stand-in for a Logic plug-in name.
//! - `for_patch("Deluxe Classic", rate)` → an instrument voiced for a Logic library patch.
//! - `list()` → what is here.
//!
//! When the bundled sample library (lg-sounds) is installed, drums, piano, orchestral
//! instruments, guitars, basses and mallets play sampled instruments instead (`sampled`); the
//! synthesized ones remain the fallback, and electronic kits, synths, organ and electric pianos
//! stay synthesized.

pub mod alchemy;
pub mod alchemy_arp;
pub mod alchemy_extra;
pub mod alchemy_filter;
pub mod alchemy_fx;
pub mod alchemy_mod;
pub mod alchemy_partials;
pub mod arp;
pub mod bass;
pub mod clav;
pub mod dmd;
pub mod drums;
pub mod drum_synth;
pub mod dsp;
pub mod legacy;
pub mod ensemble;
pub mod ep;
pub mod es2;
pub mod es2_tutorials;
pub mod evoc_ps;
pub mod mellotron;
pub mod organ;
pub mod patches;
pub mod piano;
pub mod pluck;
pub mod poly;
pub mod quick;
pub mod retro;
pub mod sample_alchemy;
pub mod sample_alchemy_an;
pub mod sampled;
pub mod sampler;
pub mod sampler_doc;
pub mod sampler_synth;
pub mod sculpture;
pub mod sculpture_tutorials;
pub mod sculpture_fx;
pub mod sculpture_string;
pub mod string;
pub mod studio;
pub mod studio_bass;
pub mod studio_piano;
pub mod synthkit;
pub mod ultrabeat;
pub mod ultrabeat_dsp;
pub mod ultrabeat_seq;
pub mod utility;
pub mod es1;
pub mod es_emp;
pub mod efm1;
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
pub const NAME_ALCHEMY: &str = "Alchemist";
pub const NAME_ES2: &str = "Trio Synth";
pub const NAME_SCULPTURE: &str = "Sculptor";
pub const NAME_MELLOTRON: &str = "Tape Keys";
pub const NAME_DMD: &str = "Drum Machine";
pub const NAME_QUICK: &str = "Quick Sampler";
pub const NAME_ES1: &str = "Mono One";
pub const NAME_ESM: &str = "Bass Mono";
pub const NAME_ESP: &str = "Poly Six";
pub const NAME_ESE: &str = "Ensemble Synth";
pub const NAME_EFM1: &str = "FM Two";
pub const NAME_EVOC_PS: &str = "Vocoder Poly";
pub const NAME_ULTRABEAT: &str = "Beat Synth";
pub const NAME_SAMPLE_ALCHEMY: &str = "Sample Alchemist";

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
    InstrumentInfo {
        name: NAME_ALCHEMY,
        logic_names: &["Alchemy"],
        description: "Four sources (virtual analog, additive, spectral, granular), master filters, Perform knobs and the Transform pad.",
    },
    InstrumentInfo {
        name: NAME_ES2,
        logic_names: &["ES2"],
        description: "Three oscillators on a triangle mixer, two filters, three envelopes, LFOs and a ten-slot modulation router.",
    },
    InstrumentInfo {
        name: NAME_ES1,
        logic_names: &["ES1"],
        description: "One oscillator plus sub, filter and ADSR (on the Trio Synth engine).",
    },
    InstrumentInfo {
        name: NAME_ESM,
        logic_names: &["ES M"],
        description: "Monophonic bass synth (on the Trio Synth engine).",
    },
    InstrumentInfo {
        name: NAME_ESP,
        logic_names: &["ES P"],
        description: "Polysynth with six waveform level sliders (on the Trio Synth engine).",
    },
    InstrumentInfo {
        name: NAME_ESE,
        logic_names: &["ES E"],
        description: "String-ensemble synth (on the Trio Synth engine).",
    },
    InstrumentInfo {
        name: NAME_EFM1,
        logic_names: &["EFM1"],
        description: "Two-operator FM synth with FM envelope (on the Retro engine).",
    },
    InstrumentInfo {
        name: NAME_SCULPTURE,
        logic_names: &["Sculpture"],
        description: "Component modelling: a string excited by struck, plucked, bowed or blown objects, pickups and body.",
    },
    InstrumentInfo {
        name: NAME_MELLOTRON,
        logic_names: &["Vintage Mellotron"],
        description: "Tape-replay keyboard with the ten factory tape sets.",
    },
    InstrumentInfo {
        name: NAME_DMD,
        logic_names: &["Drum Machine Designer"],
        description: "48 pads, each a synthesized drum (any kit, tuned) or the user's sample.",
    },
    InstrumentInfo {
        name: NAME_QUICK,
        logic_names: &["Quick Sampler (Single Sample)", "Quick Sampler"],
        description: "One sample: classic, one-shot, slice and recorder modes.",
    },
    InstrumentInfo {
        name: NAME_EVOC_PS,
        logic_names: &["EVOC 20 PolySynth", "EVOC 20 PS"],
        description: "Vocoder with a polyphonic synth: the side chain's spectrum, in up to 20 bands, shapes the synth played from the keys.",
    },
    InstrumentInfo {
        name: NAME_ULTRABEAT,
        logic_names: &["Ultrabeat"],
        description: "Drum synthesizer: 25 voices of two oscillators, ring modulator, noise, filter, envelopes, and a step sequencer.",
    },
    InstrumentInfo {
        name: NAME_SAMPLE_ALCHEMY,
        logic_names: &["Sample Alchemy"],
        description: "One sample played in classic, loop, granular, additive or spectral modes, with filters, envelopes and a mod matrix.",
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
    // the studio instruments: sections of sampled or modelled parts with articulations
    match plugin_name.trim() {
        "Studio Strings" => return Some(Box::new(studio::StudioSection::strings(rate))),
        "Studio Horns" => return Some(Box::new(studio::StudioSection::horns(rate))),
        "Studio Bass" => return Some(Box::new(studio_bass::StudioBassInst::new(rate))),
        "Studio Piano" => return Some(Box::new(studio_piano::StudioPianoInst::new(rate))),
        _ => {}
    }
    if let Some(s) = sampled::for_plugin(plugin_name, rate) {
        return Some(s);
    }
    synthesized_by_logic_name(plugin_name, rate)
}

/// The synthesized stand-in for a Logic plug-in name (never the sampled one).
pub fn synthesized_by_logic_name(plugin_name: &str, rate: f32) -> Option<Box<dyn Instrument>> {
    let inst: Box<dyn Instrument> = match plugin_name.trim() {
        "Studio Bass" => Box::new(studio_bass::StudioBassInst::with_samples(rate, false)),
        "Studio Piano" => Box::new(studio_piano::StudioPianoInst::with_samples(rate, false)),
        "Vintage Electric Piano" | "Electric Piano" | "E-Piano" | NAME_EP => {
            Box::new(ep::VintageKeysEp::new(rate))
        }
        "Vintage B3 Organ" | "Vintage B3" | "Tonewheel Organ" | NAME_B3 => {
            Box::new(organ::TonewheelB3::new(rate))
        }
        "Vintage Clav" | "Electric Clav" | NAME_CLAV => Box::new(clav::VintageClav::new(rate)),
        "ES2" | NAME_ES2 => Box::new(es2::Es2::new(rate)),
        "Alchemy" | NAME_ALCHEMY => Box::new(alchemy::Alchemy::new(rate)),
        "Sample Alchemy" | NAME_SAMPLE_ALCHEMY => Box::new(sample_alchemy::SampleAlchemy::new(rate)),
        "EVOC 20 PolySynth" | "EVOC 20 PS" | NAME_EVOC_PS => Box::new(evoc_ps::EvocPs::new(rate)),
        "ES1" | NAME_ES1 => Box::new(es1::Es1::new(rate)),
        "ES M" | NAME_ESM => Box::new(es_emp::EsSmall::new(es_emp::Model::M, rate)),
        "ES P" | NAME_ESP => Box::new(es_emp::EsSmall::new(es_emp::Model::P, rate)),
        "ES E" | NAME_ESE => Box::new(es_emp::EsSmall::new(es_emp::Model::E, rate)),
        "EFM1" | NAME_EFM1 => Box::new(efm1::Efm1::new(rate)),
        "Sculpture" | NAME_SCULPTURE => Box::new(sculpture::Sculpture::new(rate)),
        "Retro Synth" | NAME_RETRO => Box::new(retro::RetroSynth::new(rate)),
        "Piano" | NAME_PIANO => Box::new(piano::StudioPiano::new(rate)),
        "Drum Kit Designer" | "Drum Kits" | "Drum Kit" | NAME_DRUMS => {
            Box::new(drums::StudioDrumKit::new(rate))
        }
        "Drum Machine Designer" | NAME_DMD => Box::new(dmd::DrumMachineDesigner::new(rate)),
        "Ultrabeat" | NAME_ULTRABEAT => Box::new(ultrabeat::Ultrabeat::new(rate)),
        "Drum Synth" => Box::new(drum_synth::DrumSynth::new(rate)),
        utility::KLOPFGEIST => Box::new(utility::Klopfgeist::new(rate)),
        utility::TEST_OSCILLATOR => Box::new(utility::TestOscInstrument::new(rate)),
        utility::EXTERNAL_INSTRUMENT => Box::new(utility::ExternalInstrument::new(rate)),
        n if legacy::variant_of(n).is_some() => Box::new(legacy::Legacy::new(legacy::variant_of(n).unwrap_or(legacy::Variant::AnalogBasic), rate)),
        "Bass" => Box::new(bass::StudioBass::new(rate)),
        "Strings" | NAME_ENSEMBLE => Box::new(ensemble::StudioEnsemble::new(rate)),
        "Studio Strings" => Box::new(studio::StudioSection::with_samples(studio::Kind::Strings, rate, false)),
        "Horns" => with(Engine::Ensemble, &[(0, 3.0)], rate),
        "Studio Horns" => Box::new(studio::StudioSection::with_samples(studio::Kind::Horns, rate, false)),
        "Woodwind" => with(Engine::Ensemble, &[(0, 10.0)], rate),
        "Voice" => with(Engine::Ensemble, &[(0, 7.0)], rate),
        "Vintage Mellotron" | NAME_MELLOTRON => Box::new(mellotron::VintageMellotron::new(rate)),
        "Guitar" | NAME_PLUCKS => Box::new(pluck::StudioPlucks::new(rate)),
        "Tuned Percussion" => with(Engine::Plucks, &[(0, 8.0)], rate),
        "Quick Sampler (Single Sample)" | "Quick Sampler" => {
            Box::new(quick::QuickSampler::new(rate))
        }
        "Sampler" | "Sampler (Multi-Sample)" | "EXS24" => {
            Box::new(sampler::Sampler::new(Vec::new(), rate))
        }
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
        Engine::Alchemy => Box::new(alchemy::Alchemy::new(rate)),
        Engine::Es2 => Box::new(es2::Es2::new(rate)),
        Engine::Sculpture => Box::new(sculpture::Sculpture::new(rate)),
        Engine::Mellotron => Box::new(mellotron::VintageMellotron::new(rate)),
        Engine::DrumMachine => Box::new(dmd::DrumMachineDesigner::new(rate)),
    };
    for &(id, v) in &spec.settings {
        inst.set(id, v);
    }
    inst
}

/// An instrument voiced for a Logic library patch ("Deluxe Classic", "Classic Rock Organ"…).
pub fn for_patch(patch: &str, rate: f32) -> Option<Box<dyn Instrument>> {
    let spec = patches::patch(patch)?;
    if let Some(s) = sampled::for_patch(patch, &spec, rate) {
        return Some(s);
    }
    Some(build(&spec, rate))
}

/// The synthesized instrument for a Logic library patch (never the sampled one).
pub fn synthesized_for_patch(patch: &str, rate: f32) -> Option<Box<dyn Instrument>> {
    patches::patch(patch).map(|s| build(&s, rate))
}
