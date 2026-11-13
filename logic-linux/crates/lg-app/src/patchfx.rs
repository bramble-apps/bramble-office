//! A Logic library patch's channel-strip effects: as in Logic, choosing a patch loads its Audio FX
//! with it (Studio's own effects, from `lg_synth::patches::fx_chain`: amp, tremolo, chorus, EQ,
//! compressor, reverb…), and the Browser's preview plays through them.

use crate::app::App;

/// Is this a library patch name (not a plug-in name)?
fn is_library_patch(name: &str) -> bool {
    lg_synth::patches::family(name).is_some() || crate::browser::patches().iter().any(|p| p.2 == name)
}

/// The patch's Audio FX as the effects' saved float blocks (index = parameter id).
pub fn inserts(name: &str) -> Vec<(String, Vec<f32>)> {
    if !is_library_patch(name) {
        return Vec::new();
    }
    lg_synth::patches::fx_chain(name)
        .into_iter()
        .filter_map(|f| {
            let mut e = crate::units::effect(f.name, crate::audio::RATE as f32)?;
            for &(id, v) in &f.settings {
                e.set(id, v);
            }
            let params = e.params().to_vec();
            let values: Vec<f32> = params.iter().map(|p| e.get(p.id)).collect();
            Some((f.name.to_string(), crate::pluginwin::floats_of(&params, &values, &[])))
        })
        .collect()
}

/// Loads a patch's Audio FX onto track `t` (Patch Merging off: they replace the track's; on: the
/// track's stay and the patch's missing ones are added).
pub fn apply(app: &mut App, t: usize, name: &str) {
    if !is_library_patch(name) {
        return;
    }
    let list: Vec<lg_core::Plugin> = inserts(name)
        .into_iter()
        .map(|(n, floats)| {
            let mut u = crate::mixstate::new_plugin(&n);
            u.params = floats;
            u
        })
        .collect();
    let merge = app.br.saved.flag("patch_merging", false);
    crate::mixstate::edit_inserts(app, t, |v| {
        if merge {
            for u in list {
                if !v.iter().any(|x| x.name == u.name) {
                    v.push(u);
                }
            }
        } else {
            *v = list;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_effects_come_with_the_patch() {
        let names = |p: &str| inserts(p).into_iter().map(|x| x.0).collect::<Vec<_>>();
        assert!(names("Dusty Fingers").iter().any(|n| n == "Bitcrusher"), "{:?}", names("Dusty Fingers"));
        assert!(!names("Absolute Zero").iter().any(|n| n == "Bitcrusher" || n == "Overdrive"));
        assert!(names("Empty Kit").is_empty());
        // plug-in names are not patches
        assert!(inserts("Alchemy").is_empty());
        // every float block reloads into its effect
        for (n, f) in inserts("Hyper Drop") {
            let mut e = crate::units::effect(&n, 48000.0).unwrap();
            e.load_floats(&f);
        }
    }
}
