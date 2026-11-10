//! Bramble Studio's audio effects: from-scratch stand-ins for Logic Pro's built-in plug-ins, all
//! on lg-dsp's `Effect` interface.
//!
//! - Parameter ids are the float index in Logic's saved parameter block: counting from the 16th
//!   byte after the "GAMETSPP" tag (the block id and one reserved zero word come first). That is
//!   the "project index" of the research's parameter maps (preset index - 1), and for Channel EQ
//!   it makes the Low Cut's on switch float 0.
//! - Every effect renders stereo in place, allocation-free in `process` (buffers are sized from
//!   the sample rate in `new`), and denormal-safe (feedback paths flush tiny values).
//! - `by_logic_name` builds the stand-in for a plug-in name as a project gives it (the display
//!   name or the short form in the AuCU, e.g. "St-Delay", "Space D"); `list` pairs Logic's names
//!   with ours. Apple-branded names get working names of our own: "Chroma Hall Verb" for
//!   ChromaVerb, "Space Convolver" for Space Designer (final names still to be chosen).
//! - Look-ahead units (Limiter, Adaptive Limiter in NoOver mode, Noise Gate and Enveloper with
//!   Lookahead) delay their output; they offer `latency_samples()` for the host to compensate.
//! - tests/reference.rs compares against Logic's own renders of a test signal when the private
//!   research folder is present (read at test time, never copied here).

pub mod util;
pub mod channel_eq;
pub mod compressor;
pub mod limiter;
pub mod noise_gate;
pub mod chroma_verb;
pub mod convolve;
pub mod space_convolver;
pub mod delays;
pub mod modulation;
pub mod autofilter;
pub mod distortion;
pub mod dynamics;

use lg_dsp::Effect;

/// (Logic's plug-in name, our name).
const NAMES: &[(&str, &str)] = &[
    ("Channel EQ", "Channel EQ"),
    ("Compressor", "Compressor"),
    ("Limiter", "Limiter"),
    ("Adaptive Limiter", "Adaptive Limiter"),
    ("Noise Gate", "Noise Gate"),
    ("ChromaVerb", "Chroma Hall Verb"),
    ("Space Designer", "Space Convolver"),
    ("Stereo Delay", "Stereo Delay"),
    ("Tape Delay", "Tape Delay"),
    ("Echo", "Echo"),
    ("Chorus", "Chorus"),
    ("Ensemble", "Ensemble"),
    ("Flanger", "Flanger"),
    ("Phaser", "Phaser"),
    ("Microphaser", "Microphaser"),
    ("Tremolo", "Tremolo"),
    ("AutoFilter", "AutoFilter"),
    ("Distortion", "Distortion"),
    ("Overdrive", "Overdrive"),
    ("Bitcrusher", "Bitcrusher"),
    ("Clip Distortion", "Clip Distortion"),
    ("Exciter", "Exciter"),
    ("DeEsser 2", "DeEsser 2"),
    ("Gain", "Gain"),
    ("Enveloper", "Enveloper"),
];

/// The stand-in for a Logic plug-in, by the name a project gives it (display or short name).
pub fn by_logic_name(name: &str, rate: f32) -> Option<Box<dyn Effect>> {
    let fx: Box<dyn Effect> = match name.trim() {
        "Channel EQ" => Box::new(channel_eq::ChannelEq::new(rate)),
        "Compressor" => Box::new(compressor::Compressor::new(rate)),
        "Limiter" => Box::new(limiter::Limiter::new(rate)),
        "Adaptive Limiter" | "AdLimit" => Box::new(limiter::AdaptiveLimiter::new(rate)),
        "Noise Gate" => Box::new(noise_gate::NoiseGate::new(rate)),
        "ChromaVerb" | "Chroma Hall Verb" => Box::new(chroma_verb::ChromaVerb::new(rate)),
        "Space Designer" | "Space D" | "Space Convolver" => Box::new(space_convolver::SpaceConvolver::new(rate)),
        "Stereo Delay" | "St-Delay" => Box::new(delays::StereoDelay::new(rate)),
        "Tape Delay" => Box::new(delays::TapeDelay::new(rate)),
        "Echo" => Box::new(delays::Echo::new(rate)),
        "Chorus" => Box::new(modulation::Chorus::new(rate)),
        "Ensemble" => Box::new(modulation::Ensemble::new(rate)),
        "Flanger" => Box::new(modulation::Flanger::new(rate)),
        "Phaser" => Box::new(modulation::Phaser::new(rate)),
        "Microphaser" => Box::new(modulation::Phaser::micro(rate)),
        "Tremolo" => Box::new(modulation::Tremolo::new(rate)),
        "AutoFilter" => Box::new(autofilter::AutoFilter::new(rate)),
        "Distortion" => Box::new(distortion::Drive::distortion(rate)),
        "Overdrive" => Box::new(distortion::Drive::overdrive(rate)),
        "Bitcrusher" => Box::new(distortion::Bitcrusher::new(rate)),
        "Clip Distortion" | "ClipDist" => Box::new(distortion::ClipDistortion::new(rate)),
        "Exciter" => Box::new(distortion::Exciter::new(rate)),
        "DeEsser 2" => Box::new(dynamics::DeEsser::new(rate)),
        "Gain" => Box::new(dynamics::Gain::new(rate)),
        "Enveloper" => Box::new(dynamics::Enveloper::new(rate)),
        _ => return None,
    };
    Some(fx)
}

/// Every effect here: (Logic's name, our name).
pub fn list() -> Vec<(&'static str, &'static str)> {
    NAMES.to_vec()
}
