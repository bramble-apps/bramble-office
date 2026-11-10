//! Logic's plug-ins and patches → Bramble's stand-ins (the matched-sounds table): the
//! instruments come from lg-synth, the effects from lg-fx. A name we have no stand-in for gives
//! None, and the track plays through the SoundFont instead.

/// An instrument for a Logic plug-in name ("Vintage Electric Piano") or patch name ("Deluxe
/// Classic").
pub fn instrument(name: &str, rate: f32) -> Option<Box<dyn lg_dsp::Instrument>> {
    if std::env::var_os("LG_SOUNDFONT_ONLY").is_some() {
        return None;
    }
    lg_synth::by_logic_name(name, rate).or_else(|| lg_synth::for_patch(name, rate))
}

/// An effect for a Logic plug-in name ("Compressor", "ChromaVerb").
pub fn effect(name: &str, rate: f32) -> Option<Box<dyn lg_dsp::Effect>> {
    let _ = (name, rate);
    None
}

/// The effects Studio has, by Logic's names (the Browser's Effects list).
pub fn effect_names() -> Vec<&'static str> {
    Vec::new()
}
