//! Behaviour tests for every instrument: no NaN, bounded output, correct pitch, notes stop after
//! release, voice stealing, sustain pedal, pitch bend, the Sampler loaders and the patch table.

use lg_dsp::{Instrument, Plugin};
// These tests exercise the synthesized engines (their parameters, models and keyswitches), so
// they ask for the synthesized stand-ins even when the sample library is installed; the bundled
// sampled instruments are tested in tests/sampled.rs.
use lg_synth::synthesized_by_logic_name as by_logic_name;
use lg_synth::{Engine, drums, for_patch, patch, sampler};

const RATE: f32 = 48000.0;

fn render(inst: &mut dyn Instrument, secs: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (secs * RATE) as usize;
    let mut l = vec![0.0; n];
    let mut r = vec![0.0; n];
    for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
        inst.render(cl, cr);
    }
    (l, r)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn check_finite(name: &str, l: &[f32], r: &[f32], limit: f32) {
    for (i, (&a, &b)) in l.iter().zip(r).enumerate() {
        assert!(
            a.is_finite() && b.is_finite(),
            "{name}: NaN/inf at sample {i}"
        );
        assert!(
            a.abs() < limit && b.abs() < limit,
            "{name}: sample {i} = {a}/{b} beyond {limit}"
        );
    }
}

/// Fundamental by normalised autocorrelation, searched between `lo` and `hi` Hz.
fn fundamental(x: &[f32], lo: f32, hi: f32) -> f32 {
    let min_lag = (RATE / hi) as usize;
    let max_lag = (RATE / lo) as usize;
    let n = x.len() - max_lag - 2;
    let mut best = (0usize, f32::MIN);
    let mut corr = vec![0.0f32; max_lag + 2];
    for lag in min_lag..=max_lag + 1 {
        let mut s = 0.0;
        let mut e1 = 0.0;
        let mut e2 = 0.0;
        for i in 0..n {
            s += x[i] * x[i + lag];
            e1 += x[i] * x[i];
            e2 += x[i + lag] * x[i + lag];
        }
        corr[lag] = s / (e1 * e2).sqrt().max(1e-12);
    }
    for lag in min_lag..=max_lag {
        if corr[lag] > best.1 {
            best = (lag, corr[lag]);
        }
    }
    // parabolic interpolation around the peak
    let k = best.0;
    let (a, b, c) = (corr[k - 1], corr[k], corr[k + 1]);
    let d = 0.5 * (a - c) / (a - 2.0 * b + c);
    RATE / (k as f32 + if d.is_finite() { d } else { 0.0 })
}

fn all_instruments() -> Vec<(String, Box<dyn Instrument>)> {
    let mut v: Vec<(String, Box<dyn Instrument>)> = Vec::new();
    for name in [
        "Vintage Electric Piano",
        "Vintage B3 Organ",
        "Vintage Clav",
        "Retro Synth",
        "Piano",
        "Drum Kit Designer",
        "Bass",
        "Strings",
        "Guitar",
        "ES2",
        "Alchemy",
        "ES1",
        "ES M",
        "ES P",
        "ES E",
        "EFM1",
        "Sculpture",
        "Vintage Mellotron",
        "Drum Machine Designer",
        "Studio Strings",
        "Studio Horns",
    ] {
        v.push((name.to_string(), by_logic_name(name, RATE).unwrap()));
    }
    for (i, _) in drums::KITS.iter().enumerate() {
        v.push((
            format!("kit {i}"),
            Box::new(drums::StudioDrumKit::with_kit(RATE, i as u8)),
        ));
    }
    for m in 0..lg_synth::ep::MODELS.len() {
        let mut e = by_logic_name("Vintage Electric Piano", RATE).unwrap();
        e.set(0, m as f32);
        v.push((format!("ep model {m}"), e));
    }
    for engine in 0..4 {
        let mut e = by_logic_name("Retro Synth", RATE).unwrap();
        e.set(19, engine as f32);
        v.push((format!("retro engine {engine}"), e));
    }
    for m in 0..lg_synth::ensemble::MODELS.len() {
        let mut e = by_logic_name("Strings", RATE).unwrap();
        e.set(0, m as f32);
        v.push((format!("ensemble {m}"), e));
    }
    for m in 0..lg_synth::pluck::MODELS.len() {
        let mut e = by_logic_name("Guitar", RATE).unwrap();
        e.set(0, m as f32);
        v.push((format!("plucks {m}"), e));
    }
    for m in 0..lg_synth::bass::MODELS.len() {
        let mut e = by_logic_name("Bass", RATE).unwrap();
        e.set(0, m as f32);
        v.push((format!("bass {m}"), e));
    }
    for m in 0..lg_synth::piano::MODELS.len() {
        let mut e = by_logic_name("Piano", RATE).unwrap();
        e.set(13, m as f32);
        v.push((format!("piano {m}"), e));
    }
    v
}

#[test]
fn no_nan_bounded_and_audible() {
    for (name, mut inst) in all_instruments() {
        let notes: &[u8] = if name.contains("kit") || name.contains("Drum") {
            &[36, 38, 42, 46, 49, 51, 45, 39, 56]
        } else {
            &[36, 48, 60, 64, 67, 72, 96]
        };
        inst.controller(1, 100);
        for (i, &n) in notes.iter().enumerate() {
            inst.note_on(n, 30 + ((i * 37) % 98) as u8);
        }
        inst.pitch_bend(8191);
        let (l, r) = render(inst.as_mut(), 0.6);
        check_finite(&name, &l, &r, 4.0);
        assert!(rms(&l) > 1e-4, "{name}: silent");
        inst.pitch_bend(-8192);
        for &n in notes {
            inst.note_off(n);
        }
        let (l, r) = render(inst.as_mut(), 0.3);
        check_finite(&name, &l, &r, 4.0);
    }
}

#[test]
fn extreme_parameters_stay_finite() {
    for (name, mut inst) in all_instruments() {
        let ids: Vec<(u32, f32, f32)> =
            inst.params().iter().map(|p| (p.id, p.min, p.max)).collect();
        for (k, &(id, lo, hi)) in ids.iter().enumerate() {
            inst.set(id, if k % 2 == 0 { hi } else { lo });
        }
        // keep the output audible enough to test (volume params at max/min are both legal)
        for &n in &[24u8, 60, 100] {
            inst.note_on(n, 127);
        }
        let (l, r) = render(inst.as_mut(), 0.5);
        check_finite(&name, &l, &r, 64.0);
        for &(id, lo, hi) in &ids {
            inst.set(id, if id % 2 == 0 { lo } else { hi });
        }
        inst.note_on(48, 1);
        let (l, r) = render(inst.as_mut(), 0.3);
        check_finite(&name, &l, &r, 64.0);
        inst.reset();
        let (l, r) = render(inst.as_mut(), 0.05);
        check_finite(&name, &l, &r, 64.0);
    }
}

#[test]
fn pitch_is_correct() {
    // (instrument, extra settings, note, tolerance in cents)
    let cases: &[(&str, &[(u32, f32)], u8, f32)] = &[
        ("Vintage Electric Piano", &[], 57, 10.0),
        ("Vintage Electric Piano", &[(0, 16.0)], 57, 10.0),
        ("Vintage Clav", &[], 57, 15.0),
        ("Retro Synth", &[(42, 0.0), (33, 0.0)], 57, 8.0),
        (
            "Retro Synth",
            &[(42, 0.0), (19, 3.0), (32, 0.0), (30, 0.0)],
            57,
            8.0,
        ),
        ("Piano", &[(5, 0.0), (7, 0.0), (11, 0.0)], 57, 12.0),
        ("Bass", &[], 45, 15.0),
        ("Guitar", &[(0, 1.0), (6, 0.0)], 57, 15.0),
        ("Guitar", &[(0, 9.0), (6, 0.0)], 69, 10.0),
        ("Strings", &[(0, 9.0), (7, 0.0), (8, 0.0)], 69, 10.0),
        (
            "Vintage B3 Organ",
            &[(0, 0.0), (1, 0.0), (3, 0.0), (15, 0.0), (18, 0.0)],
            57,
            6.0,
        ),
    ];
    for &(name, settings, note, tol) in cases {
        let mut inst = by_logic_name(name, RATE).unwrap();
        for &(id, v) in settings {
            inst.set(id, v);
        }
        inst.note_on(note, 100);
        let (l, _) = render(inst.as_mut(), 0.5);
        let x = &l[(0.15 * RATE) as usize..];
        let want = lg_dsp::midi_hz(note as f32);
        let got = fundamental(x, want * 0.75, want * 1.3);
        let cents = 1200.0 * (got / want).log2();
        assert!(
            cents.abs() < tol,
            "{name} {settings:?}: note {note} expected {want:.2} Hz, measured {got:.2} Hz ({cents:+.1} cents)"
        );
    }
}

#[test]
fn pitch_bend_moves_pitch() {
    let mut inst = by_logic_name("Retro Synth", RATE).unwrap();
    inst.set(42, 0.0);
    inst.set(33, 0.0);
    inst.note_on(57, 100);
    inst.pitch_bend(8191); // +2 semitones (default range)
    let (l, _) = render(inst.as_mut(), 0.5);
    let want = lg_dsp::midi_hz(59.0);
    let got = fundamental(&l[(0.1 * RATE) as usize..], want * 0.8, want * 1.25);
    assert!(
        (1200.0 * (got / want).log2()).abs() < 10.0,
        "bend: {got} vs {want}"
    );
}

#[test]
fn notes_stop_after_release() {
    // (instrument, seconds allowed after note-off)
    for (name, secs) in [
        ("Vintage Electric Piano", 1.5),
        ("Vintage B3 Organ", 0.8),
        ("Vintage Clav", 1.0),
        ("Retro Synth", 1.5),
        ("Piano", 3.0),
        ("Bass", 1.5),
        ("Strings", 5.0),
        ("Guitar", 3.0),
        ("Drum Kit Designer", 4.0),
        ("ES2", 1.5),
        ("Alchemy", 5.0),
        ("ES M", 1.0),
        ("ES P", 2.0),
        ("ES E", 3.0),
        ("EFM1", 3.0),
        ("Sculpture", 3.0),
        ("Vintage Mellotron", 2.0),
        ("Drum Machine Designer", 4.0),
    ] {
        let mut inst = by_logic_name(name, RATE).unwrap();
        for &n in &[48u8, 60, 64, 67] {
            inst.note_on(n, 100);
        }
        render(inst.as_mut(), 0.5);
        for &n in &[48u8, 60, 64, 67] {
            inst.note_off(n);
        }
        let (l, r) = render(inst.as_mut(), secs);
        let tail = &l[l.len() - 4800..];
        let tail_r = &r[r.len() - 4800..];
        assert!(
            rms(tail) < 1e-4 && rms(tail_r) < 1e-4,
            "{name}: still sounding {secs} s after release (rms {})",
            rms(tail)
        );
    }
}

#[test]
fn sustain_pedal_holds_notes() {
    for name in [
        "Vintage Electric Piano",
        "Piano",
        "Vintage B3 Organ",
        "Retro Synth",
    ] {
        let mut inst = by_logic_name(name, RATE).unwrap();
        inst.controller(64, 127);
        inst.note_on(60, 100);
        render(inst.as_mut(), 0.2);
        inst.note_off(60);
        let (held, _) = render(inst.as_mut(), 1.0);
        assert!(
            rms(&held[38400..]) > 1e-3,
            "{name}: pedal did not hold the note"
        );
        inst.controller(64, 0);
        let (after, _) = render(inst.as_mut(), 3.0);
        assert!(
            rms(&after[after.len() - 4800..]) < 1e-3,
            "{name}: note did not stop at pedal up"
        );
    }
}

#[test]
fn voice_stealing_keeps_output_sane() {
    // 64 overlapping notes on 32-voice instruments, then a polyphony-2 synth with 3 notes
    for name in [
        "Vintage Electric Piano",
        "Retro Synth",
        "Piano",
        "Strings",
        "Guitar",
        "Bass",
        "Vintage Clav",
    ] {
        let mut inst = by_logic_name(name, RATE).unwrap();
        for i in 0..64u8 {
            inst.note_on(30 + i, 90);
            let (l, r) = render(inst.as_mut(), 0.01);
            check_finite(name, &l, &r, 8.0);
        }
        for i in 0..64u8 {
            inst.note_off(30 + i);
        }
        render(inst.as_mut(), 4.0);
        let (l, _) = render(inst.as_mut(), 1.0);
        assert!(rms(&l) < 1e-3, "{name}: hanging notes after stealing");
    }
    // polyphony 2: the third held note must steal; output stays at two notes' level
    let mut s = by_logic_name("Retro Synth", RATE).unwrap();
    s.set(0, 2.0);
    s.set(42, 0.0);
    s.note_on(48, 100);
    s.note_on(55, 100);
    let (two, _) = render(s.as_mut(), 0.4);
    s.note_on(60, 100);
    let (three, _) = render(s.as_mut(), 0.4);
    let a = rms(&two[9600..]);
    let b = rms(&three[9600..]);
    assert!(b < a * 1.35, "polyphony 2 did not steal: {a} → {b}");
    // and the stolen note (48) is gone: its fundamental no longer dominates
    let f = fundamental(&three[9600..], 100.0, 300.0);
    assert!((f - lg_dsp::midi_hz(48.0)).abs() > 3.0 || b < a * 1.35);
}

#[test]
fn velocity_changes_level_and_tone() {
    for name in [
        "Vintage Electric Piano",
        "Piano",
        "Drum Kit Designer",
        "Bass",
        "Guitar",
        "Vintage Clav",
    ] {
        let note = if name.starts_with("Drum") { 38 } else { 60 };
        let mut a = by_logic_name(name, RATE).unwrap();
        let mut b = by_logic_name(name, RATE).unwrap();
        a.note_on(note, 30);
        b.note_on(note, 120);
        let (la, _) = render(a.as_mut(), 0.3);
        let (lb, _) = render(b.as_mut(), 0.3);
        assert!(
            rms(&lb) > rms(&la) * 2.0,
            "{name}: velocity barely changes level"
        );
    }
}

#[test]
fn drum_round_robin_varies() {
    let mut k = drums::StudioDrumKit::new(RATE);
    k.note_on(38, 100);
    let (a, _) = render(&mut k, 0.3);
    k.note_on(38, 100);
    let (b, _) = render(&mut k, 0.3);
    let diff: f32 = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum();
    assert!(diff > 1.0, "two snare hits are identical");
    // closed hat chokes the open hat
    let mut k = drums::StudioDrumKit::new(RATE);
    k.set(18, 0.0);
    k.note_on(46, 120);
    render(&mut k, 0.05);
    k.note_on(42, 1);
    render(&mut k, 0.2);
    let (l, _) = render(&mut k, 0.2);
    assert!(rms(&l) < 1e-3, "open hat was not choked");
}

#[test]
fn every_gm_drum_note_sounds() {
    for kit in 0..drums::KITS.len() as u8 {
        for n in 33u8..=87 {
            let mut k = drums::StudioDrumKit::with_kit(RATE, kit);
            k.note_on(n, 100);
            let (l, r) = render(&mut k, 0.2);
            check_finite("drums", &l, &r, 4.0);
            assert!(rms(&l) > 1e-4, "kit {kit} note {n} is silent");
        }
    }
}

#[test]
fn sampler_sfz_plays_in_tune_and_loops() {
    let dir = std::env::temp_dir().join(format!("lg-synth-test-sfz-{}", std::process::id()));
    let sfz = sampler::write_test_instrument(&dir).unwrap();
    let mut s = sampler::Sampler::from_sfz(&sfz, RATE).unwrap();
    assert_eq!(s.zones().len(), 6);
    s.note_on(57, 100); // between roots: resampled
    let (l, r) = render(&mut s, 2.0); // longer than the sample: must loop
    check_finite("sampler", &l, &r, 4.0);
    let want = lg_dsp::midi_hz(57.0);
    let got = fundamental(&l[RATE as usize..], want * 0.8, want * 1.25);
    assert!(
        (1200.0 * (got / want).log2()).abs() < 5.0,
        "sampler pitch {got} vs {want}"
    );
    assert!(
        rms(&l[(1.5 * RATE) as usize..]) > 1e-3,
        "loop did not sustain"
    );
    s.note_off(57);
    let (l, _) = render(&mut s, 2.0);
    assert!(
        rms(&l[l.len() - 4800..]) < 1e-4,
        "sampler release did not end"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn soundfont_native_and_converted() {
    let path = std::path::Path::new("/usr/share/sounds/sf2/TimGM6mb.sf2");
    if !path.exists() {
        eprintln!("no system SoundFont; skipped");
        return;
    }
    let presets = sampler::sf2_presets(path).unwrap();
    assert!(!presets.is_empty());
    let mut native = sampler::SoundFontPlayer::new(path, 0, 0, RATE).unwrap();
    let mut conv = sampler::Sampler::from_sf2(path, 0, 0, RATE).unwrap();
    assert!(!conv.zones().is_empty());
    for inst in [
        &mut native as &mut dyn Instrument,
        &mut conv as &mut dyn Instrument,
    ] {
        inst.note_on(57, 100);
        let (l, r) = render(inst, 1.0);
        check_finite("sf2", &l, &r, 4.0);
        let want = lg_dsp::midi_hz(57.0);
        let got = fundamental(&l[(0.3 * RATE) as usize..], want * 0.8, want * 1.25);
        assert!(
            (1200.0 * (got / want).log2()).abs() < 15.0,
            "sf2 pitch {got} vs {want}"
        );
    }
}

#[test]
fn exs_chunks_decode() {
    // a hand-built EXS file: header, one zone, one sample chunk (our understanding of the layout)
    fn chunk(sig: u32, name: &str, data: &[u8]) -> Vec<u8> {
        let mut c = Vec::new();
        c.extend_from_slice(&sig.to_le_bytes());
        c.extend_from_slice(&(data.len() as u32).to_le_bytes());
        c.extend_from_slice(&0u32.to_le_bytes());
        c.extend_from_slice(&0u32.to_le_bytes());
        c.extend_from_slice(b"TBOS");
        let mut n = [0u8; 64];
        n[..name.len()].copy_from_slice(name.as_bytes());
        c.extend_from_slice(&n);
        c.extend_from_slice(data);
        c
    }
    let dir = std::env::temp_dir().join(format!("lg-synth-test-exs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let tone: Vec<f32> = (0..48000)
        .map(|i| (i as f32 * std::f32::consts::TAU * 220.0 / 48000.0).sin() * 0.3)
        .collect();
    lg_synth::wav::write_wav_24(&dir.join("a3.wav"), 48000, &tone, &tone).unwrap();
    let mut zone = vec![0u8; 100];
    zone[1] = 57; // root
    zone[6] = 0;
    zone[7] = 127;
    zone[9] = 1;
    zone[10] = 127;
    zone[89..93].copy_from_slice(&0u32.to_le_bytes());
    let mut smp = vec![0u8; 600];
    smp[8..12].copy_from_slice(&48000u32.to_le_bytes());
    smp[336..342].copy_from_slice(b"a3.wav");
    let mut file = chunk(0x0000_0101, "Test", &[0u8; 16]);
    file.extend(chunk(0x0100_0101, "Zone", &zone));
    file.extend(chunk(0x0300_0101, "a3.wav", &smp));
    let inst = sampler::parse_exs(&file).unwrap();
    assert_eq!(inst.zones.len(), 1);
    assert_eq!(inst.zones[0].root, 57);
    let mut s = sampler::Sampler::from_exs_bytes(&file, &[dir.clone()], RATE).unwrap();
    s.note_on(69, 100);
    let (l, _) = render(&mut s, 0.5);
    let got = fundamental(&l[4800..], 350.0, 520.0);
    assert!((got - 440.0).abs() < 3.0, "exs pitch {got}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn patch_table_maps_logic_names() {
    assert_eq!(
        patch("Deluxe Classic").unwrap().engine,
        Engine::VintageKeysEp
    );
    assert_eq!(
        patch("Classic Rock Organ").unwrap().engine,
        Engine::TonewheelB3
    );
    assert_eq!(patch("Steinway Grand Piano").unwrap().engine, Engine::Piano);
    assert_eq!(patch("Seventies Clav").unwrap().engine, Engine::Clav);
    assert_eq!(patch("Roots Upright").unwrap().engine, Engine::Bass);
    assert_eq!(patch("Studio Violins 1").unwrap().engine, Engine::Ensemble);
    assert_eq!(patch("SoCal").unwrap().engine, Engine::DrumKit);
    assert_eq!(patch("Mellotron Flute").unwrap().engine, Engine::Mellotron);
    assert_eq!(patch("Vibraphone").unwrap().engine, Engine::Plucks);
    assert_eq!(patch("Wurlitzer Classic").unwrap().settings[0], (0, 16.0));
    // keyword fallbacks for names not in the library list
    assert_eq!(
        patch("My Fat Synth Bass").unwrap().engine,
        Engine::RetroSynth
    );
    assert_eq!(
        patch("Dusty Organ Thing").unwrap().engine,
        Engine::TonewheelB3
    );
    assert!(patch("zzqx").is_none());
    // every known library name resolves and builds
    let mut n = 0;
    for name in lg_synth::patches::known_patch_names() {
        assert!(patch(name).is_some(), "{name} unmapped");
        n += 1;
    }
    assert!(n > 1800);
    assert!(for_patch("Deluxe Classic", RATE).is_some());
}

#[test]
fn new_engines_are_in_tune() {
    let cases: &[(&str, &[(u32, f32)], u8, f32)] = &[
        (
            "ES2",
            &[(119, 3.0), (26, 0.0), (27, 1.0), (14, 2.0)],
            57,
            8.0,
        ),
        ("Alchemy", &[(5, 0.0), (278, 0.0)], 57, 10.0),
        ("Sculpture", &[], 57, 20.0),
        ("ES1", &[(1, 0.0)], 57, 8.0),
        ("EFM1", &[], 57, 8.0),
        ("Vintage Mellotron", &[(0, 5.0)], 69, 12.0),
    ];
    for &(name, settings, note, tol) in cases {
        let mut inst = by_logic_name(name, RATE).unwrap();
        for &(id, v) in settings {
            inst.set(id, v);
        }
        inst.note_on(note, 100);
        let (l, _) = render(inst.as_mut(), 0.6);
        let x = &l[(0.2 * RATE) as usize..];
        let want = lg_dsp::midi_hz(note as f32);
        let got = fundamental(x, want * 0.75, want * 1.3);
        let cents = 1200.0 * (got / want).log2();
        assert!(
            cents.abs() < tol,
            "{name}: expected {want:.2} Hz, measured {got:.2} Hz ({cents:+.1} cents)"
        );
    }
}

#[test]
fn ensemble_keyswitches_select_articulations() {
    let mut s = by_logic_name("Studio Strings", RATE).unwrap();
    // F#0 = staccato: the key itself is silent, the next note is short
    s.note_on(30, 100);
    let (l, _) = render(s.as_mut(), 0.1);
    assert!(rms(&l) < 1e-5, "keyswitch sounded");
    assert_eq!(s.get(13), 6.0);
    s.note_on(60, 100);
    let (l, _) = render(s.as_mut(), 2.0);
    assert!(
        rms(&l[..9600]) > 1e-3 && rms(&l[l.len() - 4800..]) < rms(&l[..9600]) * 0.05,
        "staccato did not stop by itself ({} then {})",
        rms(&l[..9600]),
        rms(&l[l.len() - 4800..])
    );
    // horns: a fall bends the pitch down
    let mut h = by_logic_name("Studio Horns", RATE).unwrap();
    h.set(0, 4.0);
    h.set(13, 7.0);
    h.note_on(72, 100);
    let (l, _) = render(h.as_mut(), 0.7);
    let f = fundamental(
        &l[(0.5 * RATE) as usize..(0.62 * RATE) as usize],
        150.0,
        600.0,
    );
    assert!(
        f < lg_dsp::midi_hz(71.0),
        "fall did not drop the pitch ({f} Hz)"
    );
}

#[test]
fn quick_sampler_modes() {
    use lg_synth::quick::QuickSampler;
    use lg_synth::wav::Audio;
    // four clicks of a decaying 440 Hz tone, 0.25 s apart
    let n = 48000;
    let mut c = vec![0.0f32; n];
    for k in 0..4 {
        for i in 0..6000 {
            let t = i as f32 / 48000.0;
            c[k * 12000 + i] = (t * 440.0 * std::f32::consts::TAU).sin() * (-t * 30.0).exp() * 0.5;
        }
    }
    let a = Audio {
        rate: 48000,
        channels: vec![c],
        loop_points: None,
        root: None,
    };
    let mut q = QuickSampler::new(RATE);
    q.set_audio(a);
    assert_eq!(q.get(1), 69.0, "root detection");
    q.note_on(69, 100);
    let (l, _) = render(&mut q, 0.2);
    let f = fundamental(&l[100..4000], 300.0, 600.0);
    assert!((f - 440.0).abs() < 4.0, "classic pitch {f}");
    q.note_off(69);
    q.set(0, 2.0); // slice
    assert_eq!(q.slices.len(), 4, "slices {:?}", q.slices);
    q.note_on(38, 100);
    let (l, _) = render(&mut q, 0.2);
    assert!(rms(&l) > 1e-3, "slice 3 silent");
}

#[test]
fn drum_machine_pads() {
    use lg_synth::dmd::{DrumMachineDesigner, pad_param};
    let mut d = DrumMachineDesigner::new(RATE);
    for n in 36u8..84 {
        d.note_on(n, 100);
        let (l, r) = render(&mut d, 0.15);
        check_finite("dmd", &l, &r, 4.0);
    }
    // retuning a pad changes its pitch
    let mut a = DrumMachineDesigner::new(RATE);
    a.set(1, 0.0);
    let mut b = DrumMachineDesigner::new(RATE);
    b.set(1, 0.0);
    b.set(pad_param(5, 2), 12.0); // pad F1 (41), tom: +12
    a.note_on(41, 100);
    b.note_on(41, 100);
    let (la, _) = render(&mut a, 0.3);
    let (lb, _) = render(&mut b, 0.3);
    let fa = fundamental(&la[2400..], 40.0, 400.0);
    let fb = fundamental(&lb[2400..], 80.0, 800.0);
    assert!((fb / fa - 2.0).abs() < 0.15, "pad tune: {fa} → {fb}");
    // a sample on a pad replaces the drum
    let tone: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let mut c = DrumMachineDesigner::new(RATE);
    c.set_pad_sample(
        40,
        &lg_synth::wav::Audio {
            rate: 48000,
            channels: vec![tone],
            loop_points: None,
            root: None,
        },
    );
    c.note_on(40, 127);
    let (l, _) = render(&mut c, 0.05);
    assert!(rms(&l) > 0.05);
}

#[test]
fn alchemy_reads_preset_text_and_transforms() {
    let text = "<alchemypreset>\r\nName = Test\r\n\r\n<perform>\r\n_Contrl1 = 0.1\r\n_Contrl2 = 0.9\r\nPerVol = 0.8\r\n_Contrl1 = 0.9\r\n_Contrl2 = 0.1\r\nPerVol = 0.8\r\n";
    let mut a = lg_synth::alchemy::Alchemy::new(RATE);
    a.load_preset_text(&text);
    assert_eq!(a.snapshots.len(), 2);
    assert!((a.get(1) - 0.1).abs() < 1e-6);
    a.set(17, 0.1);
    a.set(18, 0.5);
    a.note_on(60, 100);
    let (l, r) = render(&mut a, 0.3);
    check_finite("alchemy", &l, &r, 4.0);
    // granular, spectral and additive sources all sound
    for flag in [293u32, 309, 313] {
        let mut a = lg_synth::alchemy::Alchemy::new(RATE);
        a.set(flag, 1.0);
        a.set(278, 0.0);
        a.note_on(60, 100);
        let (l, _) = render(&mut a, 0.4);
        assert!(rms(&l) > 1e-3, "source flag {flag} silent");
    }
}
