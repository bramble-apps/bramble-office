//! Logic's plug-ins and patches → Bramble's stand-ins (the matched-sounds table): the
//! instruments come from lg-synth, the effects from lg-fx. A name we have no stand-in for gives
//! None, and the track plays through the SoundFont instead.

/// An instrument for a Logic plug-in name ("Vintage Electric Piano") or patch name ("Deluxe
/// Classic").
pub fn instrument(name: &str, rate: f32) -> Option<Box<dyn lg_dsp::Instrument>> {
    if std::env::var_os("LG_SOUNDFONT_ONLY").is_some() {
        return None;
    }
    // a hosted third-party plug-in: "Name (CLAP)", "Name (VST3)", "Name (LV2)"
    if lg_host::parse_studio_name(name).is_some() {
        return lg_host::open_by_name(name, rate).map(|i| Box::new(i) as Box<dyn lg_dsp::Instrument>);
    }
    // a sampler made from loops (Browser ▸ Create Track with Loop): "<plug-in>\u{1f}<files>"
    if name.contains('\u{1f}') {
        return crate::soundlib::sampler_instrument(name, rate);
    }
    lg_synth::by_logic_name(name, rate).or_else(|| lg_synth::for_patch(name, rate))
}

/// An effect for a Logic plug-in name ("Compressor", "ChromaVerb").
pub fn effect(name: &str, rate: f32) -> Option<Box<dyn lg_dsp::Effect>> {
    if lg_host::parse_studio_name(name).is_some() {
        return lg_host::open_by_name(name, rate).map(|i| Box::new(i) as Box<dyn lg_dsp::Effect>);
    }
    lg_fx::by_logic_name(name, rate)
}

/// The effects Studio has, by Logic's names (the Browser's Effects list).
pub fn effect_names() -> Vec<&'static str> {
    lg_fx::list().into_iter().map(|(logic, _)| logic).collect()
}
