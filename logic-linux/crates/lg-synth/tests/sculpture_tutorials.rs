//! Sculpture's Tutorial Settings: each loads every parameter in range and plays.

use lg_dsp::{Instrument, Plugin};
use lg_synth::sculpture::{params_table, Sculpture, B_ON, D_ON, O_TYPE, obj, types_of};
use lg_synth::sculpture_tutorials::{setting, NAMES};

#[test]
fn tutorial_settings_load_and_play() {
    assert!(NAMES.contains(&"E-Bass Fingered Basic EQ1") && NAMES.contains(&"Fretless Roundwound#1") && NAMES.contains(&"0001 raw pad"));
    for n in NAMES {
        let s = setting(n).unwrap();
        assert_eq!(s.len(), params_table().len());
        for p in params_table() {
            let v = s.iter().find(|e| e.0 == p.id).unwrap().1;
            assert!(v >= p.min - 1e-6 && v <= p.max + 1e-6, "{n}: {} = {v}", p.name);
        }
        let mut sc = Sculpture::new(48000.0);
        for &(id, v) in &s {
            sc.set(id, v);
        }
        sc.note_on(48, 100);
        let n_s = 48000;
        let (mut l, mut r) = (vec![0.0f32; n_s], vec![0.0f32; n_s]);
        for c in (0..n_s).step_by(256) {
            sc.render(&mut l[c..c + 256], &mut r[c..c + 256]);
        }
        let rms = (l.iter().map(|x| x * x).sum::<f32>() / n_s as f32).sqrt();
        assert!(rms > 1e-4, "{n} plays: {rms}");
    }
    let get = |n: &str, id: u32| setting(n).unwrap().iter().find(|e| e.0 == id).unwrap().1;
    assert_eq!(get("E-Bass Fingered Basic EQ1", B_ON), 1.0, "with Body EQ");
    assert_eq!(get("Fretless Roundwound#1", D_ON), 1.0, "with effects");
    assert_eq!(types_of(0)[get("0001 raw pad", obj(0, O_TYPE)) as usize], "Bow");
    assert!(setting("x").is_none());
}
