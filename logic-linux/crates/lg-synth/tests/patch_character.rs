//! Patches with different names sound different (as in Logic), and Empty Kit is silent.

use lg_synth::patches::{self, Cat};
use lg_synth::soundcheck::{closest_pair, distance, features, peak, render};

/// Below this the two renders count as near-identical (dB, see soundcheck::distance).
const NEAR: f32 = 1.0;

fn family(c: Cat) -> Vec<&'static str> {
    patches::known_patch_names().filter(|n| patches::family(n) == Some(c)).collect()
}

fn feats(n: &str) -> Vec<f32> {
    let (x, s) = render(n).unwrap();
    features(&x, s)
}

#[test]
fn empty_kit_is_silent_with_empty_pads() {
    for n in ["Empty Kit", "Empty Pad"] {
        let (x, _) = render(n).unwrap();
        assert!(peak(&x) < 1e-6, "{n}: {}", peak(&x));
        assert!(patches::fx_chain(n).is_empty());
    }
    let mut d = lg_synth::dmd::DrumMachineDesigner::new(48000.0);
    lg_dsp::Plugin::set(&mut d, 2, lg_synth::dmd::EMPTY_STYLE as f32);
    assert!((0..128).all(|n| d.pad_empty(n)));
    // a sample put on an empty pad plays
    let a = lg_synth::wav::Audio { rate: 48000, channels: vec![vec![0.5; 4800]], ..Default::default() };
    d.set_pad_sample(36, &a);
    assert!(!d.pad_empty(36));
}

#[test]
fn the_named_kits_differ() {
    let names = ["Hyper Drop", "Absolute Zero", "Analog Haze", "Dusty Fingers"];
    let f: Vec<Vec<f32>> = names.iter().map(|n| feats(n)).collect();
    for i in 0..f.len() {
        for j in i + 1..f.len() {
            let d = distance(&f[i], &f[j]);
            assert!(d > 2.0, "{} vs {}: {d}", names[i], names[j]);
        }
    }
    let fx = |n: &str| patches::fx_chain(n).into_iter().map(|f| f.name).collect::<Vec<_>>();
    assert!(fx("Dusty Fingers").contains(&"Bitcrusher"));
    assert!(!fx("Absolute Zero").iter().any(|n| *n == "Bitcrusher" || *n == "Overdrive"));
    // "cold and clean": its EQ lifts the top; "dusty": cuts it
    let high = |n: &str| patches::fx_chain(n).into_iter().find(|f| f.name == "Vintage Console EQ").and_then(|f| f.settings.iter().find(|s| s.0 == 0).map(|s| s.1)).unwrap();
    assert!(high("Absolute Zero") > high("Dusty Fingers") + 6.0);
}

#[test]
fn kits_in_a_folder_all_sound_different() {
    for c in [Cat::ElectronicDrums, Cat::AcousticDrums, Cat::Percussion] {
        let names: Vec<&str> = family(c).into_iter().filter(|n| !n.starts_with("Empty")).collect();
        let (d, a, b) = closest_pair(&names).unwrap();
        assert!(d >= NEAR, "{c:?}: {a} vs {b} = {d}");
    }
}

#[test]
fn keys_in_a_folder_all_sound_different() {
    for c in [Cat::ElectricPiano, Cat::Organ, Cat::Clavinet] {
        let (d, a, b) = closest_pair(&family(c)).unwrap();
        assert!(d >= NEAR, "{c:?}: {a} vs {b} = {d}");
    }
}

#[test]
fn wurlitzers_are_clearly_apart() {
    let d = distance(&feats("Wurlitzer Classic"), &feats("Wurlitzer Modern"));
    assert!(d > 2.0, "{d}");
}

/// Every name of every folder gets its own voicing (instrument settings or effects).
#[test]
fn no_two_names_share_a_voicing() {
    let mut seen = std::collections::HashMap::new();
    // (Empty Kit and Empty Pad are both silent)
    for n in patches::known_patch_names().filter(|n| *n != "Empty Pad") {
        let s = patches::patch(n).unwrap();
        let key = format!("{:?}{:?}{:?}", s.engine, s.settings, patches::fx_chain(n));
        if let Some(o) = seen.insert(key, n) {
            panic!("{o} and {n} are voiced the same");
        }
    }
}

#[test]
fn voicing_is_deterministic() {
    for n in ["Dusty Fingers", "Wurlitzer Classic", "Abandoned Pad"] {
        assert_eq!(patches::patch(n), patches::patch(n));
        assert_eq!(patches::fx_chain(n), patches::fx_chain(n));
    }
}
