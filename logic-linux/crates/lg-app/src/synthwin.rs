//! The synthesizers' windows: which panel an instrument gets (instwin asks here), one file
//! per synthesizer.

use crate::app::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Synth {
    Es1,
    EsE,
    EsM,
    EsP,
    Efm1,
    Es2,
    Retro,
    Sculpture,
    Evoc,
    Ultrabeat,
    SampleAlchemy,
    DrumSynth,
    Legacy,
}

pub fn kind_of(any: &dyn std::any::Any) -> Option<Synth> {
    if any.is::<lg_synth::es1::Es1>() {
        Some(Synth::Es1)
    } else if let Some(s) = any.downcast_ref::<lg_synth::es_emp::EsSmall>() {
        Some(match s.model {
            lg_synth::es_emp::Model::E => Synth::EsE,
            lg_synth::es_emp::Model::M => Synth::EsM,
            lg_synth::es_emp::Model::P => Synth::EsP,
        })
    } else if any.is::<lg_synth::efm1::Efm1>() {
        Some(Synth::Efm1)
    } else if any.is::<lg_synth::es2::Es2>() {
        Some(Synth::Es2)
    } else if any.is::<lg_synth::retro::RetroSynth>() {
        Some(Synth::Retro)
    } else if any.is::<lg_synth::sculpture::Sculpture>() {
        Some(Synth::Sculpture)
    } else if any.is::<lg_synth::evoc_ps::EvocPs>() {
        Some(Synth::Evoc)
    } else if any.is::<lg_synth::ultrabeat::Ultrabeat>() {
        Some(Synth::Ultrabeat)
    } else if any.is::<lg_synth::sample_alchemy::SampleAlchemy>() {
        Some(Synth::SampleAlchemy)
    } else if any.is::<lg_synth::drum_synth::DrumSynth>() {
        Some(Synth::DrumSynth)
    } else if any.is::<lg_synth::legacy::Legacy>() {
        Some(Synth::Legacy)
    } else {
        None
    }
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize, s: Synth) {
    match s {
        Synth::Es1 => crate::es1win::window(app, ctx, t),
        Synth::EsE => crate::esewin::window(app, ctx, t),
        Synth::EsM => crate::esmwin::window(app, ctx, t),
        Synth::EsP => crate::espwin::window(app, ctx, t),
        Synth::Efm1 => crate::efm1win::window(app, ctx, t),
        Synth::Es2 => crate::es2win::window(app, ctx, t),
        Synth::Retro => crate::retrowin::window(app, ctx, t),
        Synth::Sculpture => crate::sculptwin::window(app, ctx, t),
        Synth::Evoc => crate::evocwin::window(app, ctx, t),
        Synth::Ultrabeat => crate::ubwin::window(app, ctx, t),
        Synth::SampleAlchemy => crate::salchwin::window(app, ctx, t),
        Synth::DrumSynth => crate::drumsynthwin::window(app, ctx, t),
        Synth::Legacy => crate::legacywin::window(app, ctx, t),
    }
}
