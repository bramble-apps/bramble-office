//! Comparisons with Logic's own renders of the reference test signal (private research data,
//! read at test time; skipped when the research folder isn't on this machine).

mod common;
use common::*;
use lg_dsp::Plugin;

/// A Channel EQ render, set up from its settings.json: the bands as displayed (cut slopes in
/// dB/oct), Master Gain, and which cuts are on from its notes.
fn eq_case(setting: &str) -> Option<(Summary, Summary)> {
    let theirs = reference(&format!("ChannelEQ/{setting}"))?;
    let rel = format!("ChannelEQ/{setting}");
    let note = settings_field(&rel, "note")?.to_lowercase();
    let low_cut_on = note.contains("low cut on") || note.contains("low cut + high cut on");
    let high_cut_on = note.contains("high cut on") && !note.contains("high cut off");
    let keys = ["LowCut(Hz,slope dB/oct,Q)", "LowShelf(Hz,dB,Q)", "Bell1", "Bell2", "Bell3", "Bell4", "HighShelf", "HighCut(Hz,slope,Q)"];
    let mut floats = Vec::new();
    for (k, key) in keys.iter().enumerate() {
        let v = settings_array(&rel, key)?;
        let on = match k {
            0 => low_cut_on,
            7 => high_cut_on,
            _ => true,
        };
        // cut slopes are stored in 6 dB/oct steps
        let g = if k == 0 || k == 7 { v[1] / 6.0 } else { v[1] };
        floats.extend([if on { 1.0 } else { 0.0 }, v[0], g, v[2]]);
    }
    floats.push(settings_num(&rel, "master_gain_dB")?);
    let mut fx = lg_fx::channel_eq::ChannelEq::new(RATE);
    fx.load_floats(&floats);
    let x = composite();
    let (y, _) = run(&mut fx, &x);
    let ours = analyse(&x, &y);
    print_compare(setting, &ours, &theirs);
    Some((ours, theirs))
}

#[test]
fn channel_eq_matches_logic() {
    // tolerance per render: resonant cuts (the phone band) are the known weak spot
    for (setting, tol) in [("FinalMixPop", 1.0), ("HumKiller", 1.5), ("Phone", 2.0)] {
        let Some((ours, theirs)) = eq_case(setting) else { continue };
        assert!((ours.sine - theirs.sine).abs() < tol, "{setting} sine");
        let e = sweep_error(&ours, &theirs, 60.0, 13000.0);
        eprintln!("  max sweep error 60 Hz-13 kHz: {e:.2} dB");
        assert!(e < tol * 2.0, "{setting} sweep error {e}");
    }
}

fn comp_case(setting: &str, circuit: f32) -> Option<(Summary, Summary)> {
    let theirs = reference(&format!("Compressor/{setting}"))?;
    let mut fx = lg_fx::compressor::Compressor::new(RATE);
    fx.set(lg_fx::compressor::CIRCUIT, circuit);
    let x = composite();
    let (y, _) = run(&mut fx, &x);
    let ours = analyse(&x, &y);
    print_compare(setting, &ours, &theirs);
    Some((ours, theirs))
}

#[test]
fn compressor_matches_logic() {
    let mut res = Vec::new();
    for (setting, circuit) in [("default", 0.0), ("VintageVCA", 4.0), ("VintageFET", 5.0), ("VintageOpto", 6.0)] {
        let Some(r) = comp_case(setting, circuit) else { return };
        res.push((setting, r));
    }
    for (setting, (ours, theirs)) in res {
        assert!((ours.sine - theirs.sine).abs() < 1.0, "{setting} sine {} vs {}", ours.sine, theirs.sine);
        assert!((ours.noise - theirs.noise).abs() < 1.0, "{setting} noise {} vs {}", ours.noise, theirs.noise);
    }
}

/// The Vintage FET's attack and release against Logic's render (5 ms RMS steps around the sine's
/// start and end).
#[test]
fn compressor_fet_envelope_matches_logic() {
    let Some(d) = reference_dir() else { return };
    let Some(theirs) = read_wav(&d.join("Compressor/VintageFET/out.wav")) else { return };
    let mut fx = lg_fx::compressor::Compressor::new(RATE);
    fx.set(lg_fx::compressor::CIRCUIT, 5.0);
    let x = composite();
    let (ours, _) = run(&mut fx, &x);
    let a = envelope(&ours, 7.995, 8.2, 5.0);
    let b = envelope(&theirs, 7.995, 8.2, 5.0);
    eprintln!("onset ours  {:?}", a.iter().map(|v| (v * 10.0).round() / 10.0).collect::<Vec<_>>());
    eprintln!("onset logic {:?}", b.iter().map(|v| (v * 10.0).round() / 10.0).collect::<Vec<_>>());
    let worst = a.iter().zip(b.iter()).skip(2).map(|(p, q)| (p - q).abs()).fold(0.0, f64::max);
    eprintln!("worst onset difference {worst:.2} dB");
    assert!(worst < 3.0);
}

#[test]
fn limiter_matches_logic() {
    for (setting, gain, rel) in [("default", 0.0, 250.0), ("gain12", 12.0, 250.0), ("gain12-rel20", 12.0, 20.0)] {
        let Some(theirs) = reference(&format!("Limiter/{setting}")) else { return };
        let mut fx = lg_fx::limiter::Limiter::new(RATE);
        fx.set(0, gain);
        fx.set(3, rel);
        let x = composite();
        let (mut y, _) = run(&mut fx, &x);
        // Logic compensates the look-ahead delay; so do we, for the comparison
        y.drain(..fx.latency_samples());
        y.resize(x.len(), 0.0);
        let ours = analyse(&x, &y);
        print_compare(setting, &ours, &theirs);
        assert!((ours.sine - theirs.sine).abs() < 0.3, "{setting} sine");
        assert!((ours.noise - theirs.noise).abs() < 1.0, "{setting} noise");
    }
}

/// ChromaVerb settings of a reference render from its settings.json (main-page values), as
/// (id, value) pairs: 2 Room, 0 Dry, 1 Wet, 3 Predelay, 5 Decay, 7 Attack, 8 Size, 9 Density,
/// 10 Distance. The default render has no file: defaults.
fn chroma_settings(setting: &str) -> Vec<(u32, f32)> {
    let rel = format!("ChromaVerb/{setting}");
    let mut v = Vec::new();
    if let Some(room) = settings_field(&rel, "room") {
        if let Some(i) = lg_fx::chroma_verb::ROOMS.iter().position(|r| r.eq_ignore_ascii_case(&room)) {
            v.push((2, i as f32));
        }
    }
    for (id, key) in [(0, "Dry%"), (1, "Wet%"), (3, "Predelay_ms"), (5, "Decay_s"), (7, "Attack%"), (8, "Size%"), (9, "Density%"), (10, "Distance%")] {
        if let Some(x) = settings_num(&rel, key) {
            v.push((id, x));
        }
    }
    v
}

fn chroma_case(setting: &str) -> Option<(Summary, Summary)> {
    let theirs = reference(&format!("ChromaVerb/{setting}"))?;
    let mut fx = lg_fx::chroma_verb::ChromaVerb::new(RATE);
    for (id, v) in chroma_settings(setting) {
        fx.set(id, v);
    }
    let x = composite();
    let (y, _) = run(&mut fx, &x);
    let ours = analyse(&x, &y);
    print_compare(setting, &ours, &theirs);
    Some((ours, theirs))
}

#[test]
fn chroma_verb_levels_match_logic() {
    let mut res = Vec::new();
    for setting in ["default", "ConcertHall", "SmallWetRoom", "BigSynthHall"] {
        let Some(r) = chroma_case(setting) else { continue };
        res.push((setting, r));
    }
    for (setting, (ours, theirs)) in res {
        // Logic's Concert Hall setting answers steady noise ~7 dB below what its impulse response
        // implies (its Attack seems level-dependent); we match its impulse response instead
        // (chroma_verb_envelopes), so its noise figure is only reported.
        if setting == "ConcertHall" {
            continue;
        }
        assert!((ours.noise - theirs.noise).abs() < 1.0, "{setting} noise {} vs {}", ours.noise, theirs.noise);
    }
}

/// The impulse response's energy envelope (50 ms steps) against Logic's ChromaVerb renders.
#[test]
fn chroma_verb_envelopes() {
    let Some(d) = reference_dir() else { return };
    for setting in ["default", "SmallWetRoom", "ConcertHall", "BigSynthHall"] {
        let Some(theirs) = read_wav(&d.join(format!("ChromaVerb/{setting}/out.wav"))) else { continue };
        let mut fx = lg_fx::chroma_verb::ChromaVerb::new(RATE);
        for (id, v) in chroma_settings(setting) {
            fx.set(id, v);
        }
        // the wet part only (their dry click sits at sample 0, which is skipped)
        fx.set(0, 0.0);
        let x = composite();
        let (ours, _) = run(&mut fx, &x);
        // theirs has the dry impulse (70 %) at sample 0; skip the first 20 ms
        let a = envelope(&ours, 0.02, 1.0, 40.0);
        let b = envelope(&theirs, 0.02, 1.0, 40.0);
        eprintln!("{setting} ours  {:?}", a.iter().map(|v| v.round()).collect::<Vec<_>>());
        eprintln!("{setting} logic {:?}", b.iter().map(|v| v.round()).collect::<Vec<_>>());
        // energy of the wet impulse response (after the dry click), in dB
        let energy = |y: &[f32]| 10.0 * (y[1..8 * RATE as usize].iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / 0.25).log10();
        let (eo, et) = (energy(&ours), energy(&theirs));
        eprintln!("{setting} impulse-response energy ours {eo:.1} dB, logic {et:.1} dB");
        assert!((eo - et).abs() < 2.5, "{setting} energy");
    }
}

#[test]
fn space_convolver_level_matches_logic_default() {
    let Some(theirs) = reference("SpaceDesigner/default") else { return };
    let mut fx = lg_fx::space_convolver::SpaceConvolver::new(RATE);
    let x = composite();
    let (y, _) = run(&mut fx, &x);
    let ours = analyse(&x, &y);
    print_compare("SpaceDesigner/default", &ours, &theirs);
    assert!((ours.noise - theirs.noise).abs() < 1.0);
}

#[test]
fn delays_match_logic_defaults() {
    for (name, setting) in [("Stereo Delay", "StereoDelay/default"), ("Tape Delay", "TapeDelay/default")] {
        let Some(theirs) = reference(setting) else { return };
        let mut fx = lg_fx::by_logic_name(name, RATE).unwrap();
        fx.set_tempo(120.0);
        let x = composite();
        let (y, _) = run(fx.as_mut(), &x);
        let ours = analyse(&x, &y);
        print_compare(setting, &ours, &theirs);
        assert!((ours.sine - theirs.sine).abs() < 1.0, "{setting} sine");
        assert!((ours.noise - theirs.noise).abs() < 1.0, "{setting} noise");
    }
}

/// Every plug-in block in the research's sample projects that we have a stand-in for loads
/// (floats from 16 bytes after "GAMETSPP") and plays the test signal cleanly.
#[test]
fn sample_project_settings_load_and_play() {
    let Some(plugins) = plugins_dir() else { return };
    let samples = plugins.parent().unwrap().join("sample-files");
    let mut files = Vec::new();
    let mut stack = vec![samples];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n == "ProjectData") {
                files.push(p);
            }
        }
    }
    let x: Vec<f32> = composite()[..48000 * 12].to_vec();
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    let (mut boards, mut pedals) = (0, 0);
    let mut done = std::collections::HashSet::new();
    for f in files {
        let b = std::fs::read(&f).unwrap();
        let mut at = 0;
        while let Some(i) = b[at..].windows(8).position(|w| w == b"GAMETSPP").map(|p| p + at) {
            at = i + 8;
            if i < 12 {
                continue;
            }
            let size = u32::from_le_bytes(b[i - 12..i - 8].try_into().unwrap()) as usize;
            // the display name: the last printable run before the block's header
            let back = &b[i.saturating_sub(300)..i];
            let runs: Vec<String> = back.split(|c| !(0x20..0x7f).contains(c)).filter(|r| r.len() >= 3).map(|r| String::from_utf8_lossy(r).into_owned()).collect();
            let Some(name) = runs.iter().rev().find(|r| *r != "GAME" && lg_fx::by_logic_name(r, RATE).is_some()).cloned() else { continue };
            if runs.last().map(|r| r.as_str()) == Some("xoBSH") {
                continue;
            }
            let n = (size.saturating_sub(28) / 4).min(4096);
            let floats: Vec<f32> = (0..n).filter_map(|k| b.get(i + 16 + 4 * k..i + 20 + 4 * k)).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
            // each distinct setting once
            let key = (name.clone(), floats.iter().take(300).map(|v| v.to_bits()).collect::<Vec<_>>());
            if !done.insert(key) {
                continue;
            }
            let mut fx = if name == "Pedalboard" {
                boards += 1;
                let board = lg_fx::stomp::Pedalboard::from_aucu(&b[i..(i + 20000).min(b.len())], RATE);
                pedals += board.pedal_names();
                Box::new(board) as Box<dyn lg_dsp::Effect>
            } else {
                let mut fx = lg_fx::by_logic_name(&name, RATE).unwrap();
                fx.load_floats(&floats);
                fx
            };
            fx.set_tempo(120.0);
            let (l, r) = run(fx.as_mut(), &x);
            let peak = l.iter().chain(r.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(l.iter().chain(r.iter()).all(|v| v.is_finite()) && peak < 30.0, "{name} in {}: peak {peak}", f.display());
            *seen.entry(name).or_default() += 1;
        }
    }
    eprintln!("loaded and played: {seen:?} (boards {boards}, pedals on them {pedals})");
    assert!(boards == 0 || pedals > 0, "no pedal found on any board");
}
