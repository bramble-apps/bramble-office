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
    }
}
