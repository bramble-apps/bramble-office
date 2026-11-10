//! The bundled sampled instruments (lg-sounds). Skipped when the sample library is not installed
//! (set BRAMBLE_SOUNDS to its directory, or install it with crates/lg-sounds/fetch.sh).

use lg_dsp::Instrument;
use lg_synth::sampled::{SampledInstrument, sound_for_patch, sound_for_plugin};

const RATE: f32 = 48000.0;

fn render(inst: &mut dyn Instrument, secs: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (secs * RATE) as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
        inst.render(a, b);
    }
    (l, r)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[test]
fn mapping_by_name_and_kit() {
    let id = |n: &str| lg_synth::patch(n).and_then(|p| sound_for_patch(n, &p)).map(|s| s.id);
    assert_eq!(id("SoCal"), Some("drums-modern"));
    assert_eq!(id("Brooklyn"), Some("drums-modern"));
    assert_eq!(id("Bluebird"), Some("drums-vintage"));
    assert_eq!(id("Speakeasy"), Some("drums-brush"));
    assert_eq!(id("Blue Ridge"), Some("drums-brush"));
    assert_eq!(id("Steinway Grand Piano"), Some("piano-grand"));
    assert_eq!(sound_for_plugin("Drum Kit Designer").unwrap().id, "drums-modern");
    assert_eq!(sound_for_plugin("Studio Strings").unwrap().id, "strings-section");
    // electronic kits, synths, organ and electric pianos stay synthesized
    assert_eq!(id("Trap Door"), None);
    assert_eq!(id("Classic Rock Organ"), None);
    assert_eq!(id("Deluxe Classic"), None);
    assert!(sound_for_plugin("Retro Synth").is_none());
}

#[test]
fn every_sampled_instrument_plays() {
    if !lg_sounds::installed() {
        eprintln!("sample library not installed: skipped");
        return;
    }
    for s in lg_sounds::index() {
        let mut i = SampledInstrument::load(s.id, RATE).unwrap_or_else(|| panic!("{} did not load", s.id));
        // the middle of the instrument's range
        let keys = |op: &'static str| -> Vec<u8> {
            s.sfz
                .lines()
                .filter_map(|l| l.split_whitespace().find_map(|t| t.strip_prefix(op)?.parse::<u8>().ok()))
                .collect()
        };
        let (lo, hi) = (keys("lokey=").into_iter().min().unwrap_or(0), keys("hikey=").into_iter().max().unwrap_or(127));
        let mid = ((lo as u16 + hi as u16) / 2) as u8;
        let tonal = [mid, mid.saturating_add(4).min(hi), mid.saturating_add(7).min(hi)];
        let notes: &[u8] = if s.id.starts_with("drums") { &[36, 38, 42, 49] } else { &tonal };
        for &n in notes {
            i.note_on(n, 100);
        }
        let (l, r) = render(&mut i, 0.5);
        for &n in notes {
            i.note_off(n);
        }
        let (l2, r2) = render(&mut i, 1.0);
        for (a, b) in l.iter().chain(&l2).zip(r.iter().chain(&r2)) {
            assert!(a.is_finite() && b.is_finite() && a.abs() < 2.0 && b.abs() < 2.0, "{}: {a}/{b}", s.id);
        }
        assert!(rms(&l) > 1e-3, "{}: silent ({})", s.id, rms(&l));
    }
}

#[test]
fn piano_pedal_holds_and_hats_choke() {
    if !lg_sounds::installed() {
        return;
    }
    let mut p = SampledInstrument::load("piano-grand", RATE).unwrap();
    p.controller(64, 127);
    p.note_on(60, 100);
    render(&mut p, 0.2);
    p.note_off(60);
    let (held, _) = render(&mut p, 1.0);
    assert!(rms(&held[38400..]) > 1e-3, "pedal did not hold");
    p.controller(64, 0);
    let (after, _) = render(&mut p, 2.0);
    assert!(rms(&after[after.len() - 4800..]) < rms(&held[38400..]) * 0.1, "pedal up did not damp");

    // a closed hi-hat cuts the open one
    let mut k = SampledInstrument::load("drums-modern", RATE).unwrap();
    k.note_on(46, 110);
    let (open, _) = render(&mut k, 0.3);
    k.note_on(42, 1);
    let (_, _) = render(&mut k, 0.1);
    let (after, _) = render(&mut k, 0.3);
    assert!(rms(&after) < rms(&open) * 0.1, "open hat not choked");
}

#[test]
fn velocity_layers_change_level() {
    if !lg_sounds::installed() {
        return;
    }
    let mut k = SampledInstrument::load("drums-modern", RATE).unwrap();
    k.note_on(38, 30);
    let (soft, _) = render(&mut k, 0.3);
    k.note_on(38, 127);
    let (loud, _) = render(&mut k, 0.3);
    assert!(rms(&loud) > rms(&soft) * 4.0, "snare dynamics too flat");
}
