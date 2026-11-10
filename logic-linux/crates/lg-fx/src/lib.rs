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
//!   ChromaVerb, "Space Convolver" for Space Designer, "Chroma Saturator" for ChromaGlow, "Amp
//!   Sculptor" / "Bass Amp Sculptor" for Amp Designer / Bass Amp Designer, "Stomp Board" for
//!   Pedalboard, "Mastering Chain" for Mastering Assistant (final names still to be chosen).
//! - Look-ahead units (Limiter, Adaptive Limiter in NoOver mode, Noise Gate and Enveloper with
//!   Lookahead, Mastering Chain's limiter) report their delay through `Effect::latency`.
//! - Pedalboard's pedals live in their own sub-records of the AuCU, not in its float block: build
//!   it with `stomp::Pedalboard::from_aucu(payload, rate)` to get them (by_logic_name gives an
//!   empty board).
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
pub mod vintage_eq;
pub mod chroma_glow;
pub mod phat_fx;
pub mod amp;
pub mod multipressor;
pub mod stomp;
pub mod mastering;

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
    ("Vintage Graphic EQ", "Vintage Graphic EQ"),
    ("Vintage Tube EQ", "Vintage Tube EQ"),
    ("Vintage Console EQ", "Vintage Console EQ"),
    ("ChromaGlow", "Chroma Saturator"),
    ("Phat FX", "Phat FX"),
    ("Amp Designer", "Amp Sculptor"),
    ("Bass Amp Designer", "Bass Amp Sculptor"),
    ("Multipressor", "Multipressor"),
    ("Pedalboard", "Stomp Board"),
    ("Robo Flanger", "Robo Flanger"),
    ("Total Tremolo", "Total Tremolo"),
    ("Blue Echo", "Blue Echo"),
    ("Tru-Tape Delay", "Tru-Tape Delay"),
    ("Vintage Drive", "Vintage Drive"),
    ("Delay Designer", "Delay Designer"),
    ("Mastering Assistant", "Mastering Chain"),
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
        "Vintage Graphic EQ" | "Graph EQ" => Box::new(vintage_eq::GraphicEq::new(rate)),
        "Vintage Tube EQ" | "Tube EQ" => Box::new(vintage_eq::TubeEq::new(rate)),
        "Vintage Console EQ" | "Cnsl EQ" => Box::new(vintage_eq::ConsoleEq::new(rate)),
        "ChromaGlow" | "Chroma Saturator" => Box::new(chroma_glow::ChromaGlow::new(rate)),
        "Phat FX" => Box::new(phat_fx::PhatFx::new(rate)),
        "Amp Designer" | "Amp" | "Amp Sculptor" => Box::new(amp::AmpDesigner::new(rate)),
        "Bass Amp Designer" | "Bass Amp" | "Bass Amp Sculptor" => Box::new(amp::BassAmp::new(rate)),
        "Multipressor" | "Multipr" => Box::new(multipressor::Multipressor::new(rate)),
        "Pedalboard" | "Stomp Board" => Box::new(stomp::Pedalboard::new(rate)),
        "Robo Flanger" | "RoboFlng" => Box::new(stomp::RoboFlanger::new(rate)),
        "Total Tremolo" | "TotalTrem" => Box::new(stomp::TotalTremolo::new(rate)),
        "Blue Echo" => Box::new(stomp::BlueEcho::new(rate)),
        "Tru-Tape Delay" | "Tru-Tape" => Box::new(stomp::TruTapeDelay::new(rate)),
        "Vintage Drive" => Box::new(stomp::VintageDrive::new(rate)),
        "Delay Designer" | "Delay D" => Box::new(mastering::DelayDesigner::new(rate)),
        "Mastering Assistant" | "Mastering" | "Mastering Chain" => Box::new(mastering::MasteringAssistant::new(rate)),
        _ => return None,
    };
    Some(fx)
}

/// Every effect here: (Logic's name, our name).
pub fn list() -> Vec<(&'static str, &'static str)> {
    NAMES.to_vec()
}
