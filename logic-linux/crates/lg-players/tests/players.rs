//! The players against Logic's measurements, and the guarantees the app relies on.
use lg_players::bass::{self, BassParams, BassStyle};
use lg_players::character::DRUMMERS;
use lg_players::chords::{parse_bars, Progression};
use lg_players::drummer::{self, DrummerParams, KitSection, SnareMode, TempoFeel};
use lg_players::keys::{self, KeysParams, KeysStyle, LeftHand};
use lg_players::kit::{family_of_note, Family};
use lg_players::measured::{drum_stats, mean_stats, POP_ROCK_GRID};
use lg_players::{Marker, Note, RegionSpec};

/// Inside the region, sorted, positive lengths, no two notes of a pitch overlapping.
fn assert_sane(notes: &[Note], r: &RegionSpec, what: &str) {
    assert!(!notes.is_empty(), "{what}: no notes");
    for w in notes.windows(2) {
        assert!(w[0].pos <= w[1].pos, "{what}: not sorted");
    }
    for (i, n) in notes.iter().enumerate() {
        assert!(n.pos >= r.start && n.pos + n.len <= r.end(), "{what}: outside the region {n:?}");
        assert!(n.len >= 1 && n.velocity >= 1 && n.velocity <= 127 && n.pitch <= 127, "{what}: {n:?}");
        for m in &notes[i + 1..] {
            if m.pos >= n.pos + n.len {
                break;
            }
            assert!(!(m.pitch == n.pitch && m.channel == n.channel), "{what}: overlap {n:?} {m:?}");
        }
    }
}

#[test]
fn the_grid_matches_logic() {
    let region = RegionSpec::new(0, 2880 * 8, (3, 4));
    for g in POP_ROCK_GRID {
        let all: Vec<_> = (0..24)
            .map(|s| drum_stats(&drummer::generate(&DrummerParams { complexity: g.complexity, intensity: g.intensity, seed: 500 + s, ..Default::default() }, &region, None), &region))
            .collect();
        let m = mean_stats(&all);
        for k in 0..5 {
            let tol = (g.per_bar[k] * 0.2).max(0.4);
            assert!((m.per_bar[k] - g.per_bar[k]).abs() <= tol, "{} at c {} i {}: {} per bar, Logic {}", Family::ALL[k].name(), g.complexity, g.intensity, m.per_bar[k], g.per_bar[k]);
        }
        assert!((m.mean_velocity - g.mean_velocity).abs() <= 4.5, "mean velocity {} vs {}", m.mean_velocity, g.mean_velocity);
        assert!((m.kick_velocity - g.kick_velocity).abs() <= 3.0, "kick velocity {} vs {}", m.kick_velocity, g.kick_velocity);
        assert!((m.hat_velocity - g.hat_velocity).abs() <= 5.0, "hat velocity {} vs {}", m.hat_velocity, g.hat_velocity);
        assert!((m.offset_sd - 9.5).abs() < 2.5, "timing spread {}", m.offset_sd);
    }
}

#[test]
fn intensity_changes_velocity_not_notes_and_soft_is_side_stick() {
    let region = RegionSpec::new(0, 2880 * 8, (3, 4));
    let soft = drummer::generate(&DrummerParams { intensity: 0.08, ..Default::default() }, &region, None);
    let loud = drummer::generate(&DrummerParams { intensity: 0.86, ..Default::default() }, &region, None);
    assert!((soft.len() as i32 - loud.len() as i32).abs() <= 3, "{} vs {}", soft.len(), loud.len());
    assert!(soft.iter().filter(|n| n.pitch == 37).count() > 8);
    assert_eq!(loud.iter().filter(|n| n.pitch == 37).count(), 0);
}

#[test]
fn ghost_notes_and_feel_as_measured() {
    let region = RegionSpec::new(0, 2880 * 8, (3, 4));
    let ghosts = |g: f32| mean_stats(&(0..24).map(|s| drum_stats(&drummer::generate(&DrummerParams { complexity: 0.85, ghost_notes: g, seed: s, ..Default::default() }, &region, None), &region)).collect::<Vec<_>>()).ghosts_per_bar;
    assert_eq!(ghosts(0.0), 0.0);
    assert!((ghosts(0.22) - 0.4).abs() < 0.3);
    assert!((ghosts(0.61) - 1.1).abs() < 0.3);
    for (f, logic) in lg_players::measured::FEEL_OFFSET {
        let m = mean_stats(&(0..16).map(|s| drum_stats(&drummer::generate(&DrummerParams { feel: f, seed: s, ..Default::default() }, &region, None), &region)).collect::<Vec<_>>());
        assert!((m.offset_mean - logic).abs() < 3.0, "feel {f}: {} vs {logic}", m.offset_mean);
    }
}

#[test]
fn deterministic_per_seed_and_regenerate_varies() {
    let region = RegionSpec::new(3840, 3840 * 8, (4, 4));
    let p = DrummerParams::default();
    assert_eq!(drummer::generate(&p, &region, None), drummer::generate(&p, &region, None));
    let q = p.regenerated();
    assert_ne!(p.seed, q.seed);
    let (a, b) = (drummer::generate(&p, &region, None), drummer::generate(&q, &region, None));
    assert_ne!(a, b);
    // Regenerate keeps the groove: the same hi-hat backbone
    let hats = |n: &[Note]| n.iter().filter(|x| family_of_note(x.pitch) == Family::HiHat && x.velocity >= 40).count() as i32;
    assert!((hats(&a) - hats(&b)).abs() <= 6, "{} vs {}", hats(&a), hats(&b));
    let chords = parse_bars("C Am F G", 3840, 3840);
    let bp = BassParams::default();
    assert_eq!(bass::generate(&bp, &region, &chords), bass::generate(&bp, &region, &chords));
    assert_ne!(bass::generate(&bp, &region, &chords), bass::generate(&bp.regenerated(), &region, &chords));
    let kp = KeysParams::default();
    assert_eq!(keys::generate(&kp, &region, &chords), keys::generate(&kp, &region, &chords));
}

#[test]
fn every_character_meter_and_option_stays_sane() {
    let meters = [(4u8, 4u8), (3, 4), (6, 8), (7, 8), (5, 4), (12, 8), (2, 4)];
    for (ci, ch) in DRUMMERS.iter().enumerate() {
        for (mi, sig) in meters.iter().enumerate() {
            let bar = lg_players::bar_ticks(*sig);
            // a partial last bar too
            let mut region = RegionSpec::new(bar * 2, bar * 9 + bar / 2, *sig);
            region.ending = mi % 2 == 0;
            region.markers = vec![Marker { pos: 0, name: "Intro".into() }, Marker { pos: bar * 4, name: "Verse".into() }, Marker { pos: bar * 8, name: "Chorus".into() }];
            let mut p = DrummerParams::for_character(ch);
            p.complexity = (ci % 5) as f32 / 4.0;
            p.intensity = (mi % 4) as f32 / 3.0;
            p.ghost_notes = 0.7;
            p.swing = 0.5;
            p.fills = 0.9;
            p.percussion = KitSection { enabled: ci % 3 == 0, variation: (ci % 8) as u8 };
            p.cymbals.variation = (ci % 7) as u8;
            p.kick_snare.variation = (mi % 7) as u8;
            p.snare = [SnareMode::Automatic, SnareMode::Snare, SnareMode::SideStick, SnareMode::Rimshot][ci % 4];
            p.tempo = [TempoFeel::Standard, TempoFeel::Half, TempoFeel::Double][mi % 3];
            p.feel = -1.0 + (ci % 3) as f32;
            p.humanize = 1.0;
            let n = drummer::generate(&p, &region, None);
            assert_sane(&n, &region, &format!("{} {:?}", ch.name, sig));
        }
    }
    for style in BassStyle::ALL {
        for sig in meters {
            let bar = lg_players::bar_ticks(sig);
            let region = RegionSpec::new(0, bar * 6 + bar / 3, sig);
            let chords = Progression::OneFiveSixFour.chords(2, 0, bar, 7);
            let p = BassParams { style, complexity: 0.9, dead_notes: 1.0, slides: 1.0, double_stops: 1.0, pickup_hits: true, fills: 1.0, swing: 1.0, humanize: 1.0, ..Default::default() };
            assert_sane(&bass::generate(&p, &region, &chords), &region, &format!("{style:?} {sig:?}"));
        }
    }
    for style in KeysStyle::ALL {
        for (k, sig) in meters.iter().enumerate() {
            let bar = lg_players::bar_ticks(*sig);
            let mut region = RegionSpec::new(0, bar * 6 + bar / 3, *sig);
            region.markers = vec![Marker { pos: bar * 3, name: "Chorus".into() }];
            let chords = Progression::TwoFiveOne.chords(5, 0, bar, 7);
            let lh = [LeftHand::Off, LeftHand::Root, LeftHand::RootFifth, LeftHand::Octaves, LeftHand::Chord, LeftHand::Moving][k % 6];
            let p = KeysParams { style, complexity: 0.9, grace_notes: 1.0, fills: 1.0, upper_voicing: 1 + k as u8, left_hand: lh, arp_direction: 1 + k as u8, humanize: 1.0, ..Default::default() };
            assert_sane(&keys::generate(&p, &region, &chords), &region, &format!("{style:?} {sig:?}"));
        }
    }
}

#[test]
fn sections_bring_fills_crashes_and_an_ending() {
    let bar = 3840;
    let mut region = RegionSpec::new(0, bar * 16, (4, 4));
    region.markers = vec![Marker { pos: 0, name: "Intro".into() }, Marker { pos: bar * 4, name: "Verse".into() }, Marker { pos: bar * 12, name: "Chorus".into() }];
    region.ending = true;
    let p = DrummerParams { complexity: 0.6, fills: 0.4, ..Default::default() };
    let n = drummer::generate(&p, &region, None);
    let crash_at = |b: u32| n.iter().any(|x| (x.pitch == 49 || x.pitch == 57) && x.pos.abs_diff(b * bar) < 60);
    assert!(crash_at(4) && crash_at(12), "crashes at the section starts");
    // the bar before a section change ends in a fill: toms or snare, no hats in its last beat
    for b in [3u32, 11] {
        let last_beat: Vec<_> = n.iter().filter(|x| x.pos >= b * bar + 2880 + 60 && x.pos < (b + 1) * bar - 60).collect();
        assert!(last_beat.iter().any(|x| matches!(family_of_note(x.pitch), Family::Snare | Family::Tom)), "fill in bar {}", b + 1);
        assert!(!last_beat.iter().any(|x| x.pitch == 42), "no hats under the fill in bar {}", b + 1);
    }
    // the ending: a crash on the last bar's downbeat and nothing after it
    assert!(crash_at(15));
    assert!(n.iter().all(|x| x.pos < 15 * bar + 80), "nothing after the final hit");
}

#[test]
fn follow_makes_the_kick_follow_a_rhythm() {
    let region = RegionSpec::new(0, 3840 * 8, (4, 4));
    // a bass line pushing the "a" of beat 2 and the "and" of 3
    let onsets: Vec<u32> = (0..8).flat_map(|b| [b * 3840, b * 3840 + 960 + 720, b * 3840 + 2400]).collect();
    let p = DrummerParams { fills: 0.0, follow: 1.0, ..Default::default() };
    let n = drummer::generate(&p, &region, Some(&onsets));
    let kicks: Vec<u32> = n.iter().filter(|x| x.pitch == 36).map(|x| x.pos).collect();
    let hit = onsets.iter().filter(|o| kicks.iter().any(|k| k.abs_diff(**o) < 60)).count();
    assert!(hit as f32 >= onsets.len() as f32 * 0.8, "{hit} of {}", onsets.len());
}

#[test]
fn electronic_drummers_use_drum_machine_designer_notes() {
    let region = RegionSpec::new(0, 3840 * 4, (4, 4));
    let trap = DRUMMERS.iter().find(|c| c.name == "Trap").unwrap();
    let n = drummer::generate(&DrummerParams::for_character(trap), &region, None);
    assert!(n.iter().any(|x| x.pitch == 39), "claps");
    let house = DRUMMERS.iter().find(|c| c.name == "Modern House").unwrap();
    let n = drummer::generate(&DrummerParams { fills: 0.0, ..DrummerParams::for_character(house) }, &region, None);
    let kicks = n.iter().filter(|x| x.pitch == 36).count();
    assert!(kicks >= 16, "four on the floor: {kicks}");
}

#[test]
fn bass_follows_chords_in_its_register_as_logic() {
    let region = RegionSpec::new(0, 2880 * 8, (3, 4));
    let chords = parse_bars("C Am F G C Am F G", 0, 2880);
    let mut per_bar = 0.0;
    let mut v = 0.0;
    for s in 0..16 {
        let n = bass::generate(&BassParams { seed: s, ..Default::default() }, &region, &chords);
        per_bar += n.len() as f32 / 8.0 / 16.0;
        v += n.iter().map(|x| x.velocity as f32).sum::<f32>() / n.len() as f32 / 16.0;
        for b in 0..8u32 {
            let down = n.iter().filter(|x| x.pos.abs_diff(b * 2880) < 40 && x.len > 300).max_by_key(|x| x.len).unwrap();
            assert_eq!(down.pitch % 12, chords[b as usize].chord.root);
            assert!((40..52).contains(&down.pitch), "{}", down.pitch);
        }
    }
    assert!((per_bar - lg_players::measured::BASS_NOTES_PER_BAR).abs() < 0.6, "{per_bar}");
    assert!((v - lg_players::measured::BASS_MEAN_VELOCITY).abs() < 5.0, "{v}");
}

#[test]
fn keys_voice_lead_and_match_density() {
    let region = RegionSpec::new(0, 2880 * 8, (3, 4));
    let chords = parse_bars("C Dm Am Am C Dm Am Am", 0, 2880);
    let mut per_bar = 0.0;
    for s in 0..16 {
        let n = keys::generate(&KeysParams { seed: s, ..Default::default() }, &region, &chords);
        per_bar += n.len() as f32 / 8.0 / 16.0;
        // the left hand on the root, in the octave under the lower position (43..55)
        for b in 0..8u32 {
            let lh = n.iter().filter(|x| x.pos.abs_diff(b * 2880) < 60 && x.pitch < 56).min_by_key(|x| x.pitch).unwrap();
            assert_eq!(lh.pitch % 12, chords[b as usize].chord.root);
            assert!((43..=55).contains(&lh.pitch));
        }
    }
    assert!((per_bar - lg_players::measured::KEYS_NOTES_PER_BAR).abs() < 2.5, "{per_bar}");
}

/// Reads Logic's own regions when the research folder is present: the settings decode and the
/// chords are the ones Logic shows.
#[test]
fn reads_logic_projects_when_present() {
    let Some(home) = std::env::var_os("HOME") else { return };
    let path = std::path::Path::new(&home).join("CascadeProjects/logic-research-private/sample-files/SP-Keys-Freely-plus-Bass.logicx/Alternatives/000/ProjectData");
    let Ok(data) = std::fs::read(&path) else { return };
    let settings = lg_players::genm::all_settings(&data);
    assert_eq!(settings.len(), 3);
    let chords = lg_players::genm::all_chords(&data);
    let names: Vec<String> = chords.iter().find(|c| c.0 == 0x3C0000).unwrap().1.iter().map(|c| c.chord.name()).collect();
    assert_eq!(names, ["C", "Am", "F", "G", "C", "Am", "F", "G"]);
}
