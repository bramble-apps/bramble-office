//! Patch-library sound check: renders every known Logic patch name (instrument + its channel-strip
//! effects) and prints, per library folder, the closest pair of differently named patches and how
//! many pairs are near-identical.
//!
//!   cargo run --release -p lg-synth --example patch_distance [folder-substring]

use lg_synth::patches;
use lg_synth::soundcheck::{distance, features, render};
use std::collections::BTreeMap;

const NEAR: f32 = 1.0;

fn main() {
    let only = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    let mut fam: BTreeMap<String, Vec<&'static str>> = BTreeMap::new();
    for n in patches::known_patch_names() {
        if let Some(c) = patches::family(n) {
            fam.entry(format!("{c:?}")).or_default().push(n);
        }
    }
    let mut overall = f32::MAX;
    let mut near_total = 0usize;
    let mut pairs_total = 0usize;
    for (f, names) in &fam {
        if !only.is_empty() && !f.to_lowercase().contains(&only) {
            continue;
        }
        let t = std::time::Instant::now();
        let feats: Vec<(&str, Vec<f32>)> = names.iter().filter_map(|n| render(n).map(|(x, s)| (*n, features(&x, s)))).collect();
        let mut best = (f32::MAX, "", "");
        let mut near = 0;
        let mut pairs = 0;
        for i in 0..feats.len() {
            for j in i + 1..feats.len() {
                let d = distance(&feats[i].1, &feats[j].1);
                pairs += 1;
                if d < NEAR {
                    near += 1;
                }
                if d < best.0 {
                    best = (d, feats[i].0, feats[j].0);
                }
            }
        }
        overall = overall.min(best.0);
        near_total += near;
        pairs_total += pairs;
        println!("{f:16} n={:3} min={:6.3}  near(<{NEAR})={near:5}/{pairs:6}  closest: {} | {}  ({:.1}s)", feats.len(), best.0, best.1, best.2, t.elapsed().as_secs_f32());
    }
    println!("OVERALL min={overall:.3} near={near_total}/{pairs_total}");
    for n in ["Empty Kit", "Empty Pad"] {
        if let Some((x, _)) = render(n) {
            println!("{n}: peak {:.6}", lg_synth::soundcheck::peak(&x));
        }
    }
}
