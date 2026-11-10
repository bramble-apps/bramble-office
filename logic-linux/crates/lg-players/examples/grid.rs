//! grid: the drummer at Logic's 3×3 complexity × intensity points (Pop Rock, 3/4, 8 bars, the
//! Half-pipe preset's details), notes per bar per kit piece and mean velocity, next to Logic's
//! measurements. Averages over several seeds (Logic's numbers are single regenerations).
use lg_players::drummer::{generate, DrummerParams};
use lg_players::measured::{drum_stats, mean_stats, POP_ROCK_GRID};
use lg_players::RegionSpec;

fn main() {
    // `grid show <character> <complexity> <intensity> [seed]`: our drummer's steps, 8 bars of 4/4
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("show") {
        show(&args[2..]);
        return;
    }
    let seeds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(16);
    let region = RegionSpec::new(0, 2880 * 8, (3, 4));
    println!("Pop Rock (Kyle), 3/4, 8 bars; ours = mean of {seeds} seeds | Logic");
    println!("{:<10} {:>13} {:>13} {:>13} {:>13} {:>13} | {:>11} {:>11} {:>11}", "c x i", "kick/bar", "snare/bar", "hat/bar", "toms/bar", "cym/bar", "mean vel", "kick vel", "hat vel");
    let mut worst = 0.0f32;
    for g in POP_ROCK_GRID {
        let all: Vec<_> = (0..seeds)
            .map(|s| {
                let p = DrummerParams { complexity: g.complexity, intensity: g.intensity, seed: 1000 + s, ..Default::default() };
                drum_stats(&generate(&p, &region, None), &region)
            })
            .collect();
        let m = mean_stats(&all);
        let lv = |k: usize| format!("{:>5.2}|{:<5.2}", m.per_bar[k], g.per_bar[k]);
        println!(
            "{:.2}x{:.2}  {:>13} {:>13} {:>13} {:>13} {:>13} | {:>5.1}|{:<5.1} {:>5.1}|{:<5.1} {:>5.1}|{:<5.1}",
            g.complexity, g.intensity, lv(0), lv(1), lv(2), lv(3), lv(4), m.mean_velocity, g.mean_velocity, m.kick_velocity, g.kick_velocity, m.hat_velocity, g.hat_velocity
        );
        let total_ours: f32 = m.per_bar[..5].iter().sum();
        let total_logic: f32 = g.per_bar.iter().sum();
        worst = worst.max((total_ours - total_logic).abs() / total_logic);
    }
    println!("largest notes-per-bar difference: {:.1}%", worst * 100.0);
    // details
    let region = RegionSpec::new(0, 2880 * 8, (3, 4));
    for (g, logic) in [(0.0, 0.0), (0.22, 0.4), (0.61, 1.1)] {
        let all: Vec<_> = (0..seeds).map(|s| drum_stats(&generate(&DrummerParams { complexity: 0.85, intensity: 0.86, ghost_notes: g, seed: 50 + s, ..Default::default() }, &region, None), &region)).collect();
        println!("ghost notes {:.2}: {:.2} ghosts/bar | Logic {:.2}", g, mean_stats(&all).ghosts_per_bar, logic);
    }
    for (f, logic) in [(-0.33, 12.1), (0.059, -4.2), (1.0, -33.5)] {
        let all: Vec<_> = (0..seeds).map(|s| drum_stats(&generate(&DrummerParams { feel: f, seed: 70 + s, ..Default::default() }, &region, None), &region)).collect();
        let m = mean_stats(&all);
        println!("feel {:+.2}: offset {:+.1} ticks (sd {:.1}) | Logic {:+.1} (sd 9.5)", f, m.offset_mean, m.offset_sd, logic);
    }

    // bass and keys with the settings Logic's regions had (SP-Keys-Freely-plus-Bass)
    use lg_players::chords::parse_bars;
    let bass_chords = parse_bars("C Am F G C Am F G", 0, 2880);
    let keys_chords = parse_bars("C Dm Am Am C Dm Am Am", 0, 2880);
    let (mut bn, mut bv, mut kn, mut kv, mut bc, mut kc) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for s in 0..seeds {
        let b = lg_players::bass::generate(&lg_players::bass::BassParams { humanize: 0.245, seed: s, ..Default::default() }, &region, &bass_chords);
        let k = lg_players::keys::generate(&lg_players::keys::KeysParams { fills: 0.69, dynamics: 1.19, seed: s, ..Default::default() }, &region, &keys_chords);
        bn += b.len() as f32;
        bc += 1.0;
        bv += b.iter().map(|n| n.velocity as f32).sum::<f32>() / b.len().max(1) as f32;
        kn += k.len() as f32;
        kc += 1.0;
        kv += k.iter().map(|n| n.velocity as f32).sum::<f32>() / k.len().max(1) as f32;
    }
    println!("bass (Pop Songwriter, c 0.5, i 0.71): {:.2} notes/bar, mean velocity {:.1} | Logic {:.2}, {:.1}", bn / bc / 8.0, bv / bc, lg_players::measured::BASS_NOTES_PER_BAR, lg_players::measured::BASS_MEAN_VELOCITY);
    println!("keys (Strummed, c 0.42, i 0.71): {:.2} notes/bar, mean velocity {:.1} | Logic {:.2}, {:.1}", kn / kc / 8.0, kv / kc, lg_players::measured::KEYS_NOTES_PER_BAR, lg_players::measured::KEYS_MEAN_VELOCITY);
}

fn show(a: &[String]) {
    let ch = lg_players::character::Character::find(a.first().map_or("Pop Rock", |s| s.as_str())).expect("no such drummer");
    let num = |k: usize, d: f32| a.get(k).and_then(|s| s.parse().ok()).unwrap_or(d);
    let p = DrummerParams { complexity: num(1, 0.5), intensity: num(2, 0.7), seed: num(3, 1.0) as u64, ..DrummerParams::for_character(ch) };
    let mut region = RegionSpec::new(0, 3840 * 8, (4, 4));
    region.ending = true;
    let notes = generate(&p, &region, None);
    let kit = ch.kit_map();
    println!("{} ({}), {} notes", ch.name, ch.personality, notes.len());
    for b in 0..8u32 {
        println!("bar {}", b + 1);
        let mut pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
        pitches.sort();
        pitches.dedup();
        for pitch in pitches.iter().rev() {
            let mut row = ['.'; 16];
            let mut any = false;
            for n in notes.iter().filter(|n| n.pitch == *pitch) {
                let rel = n.pos as i64 + 120 - (b * 3840) as i64;
                if (0..3840).contains(&rel) {
                    row[(rel / 240) as usize] = if n.velocity < 40 { 'g' } else if n.velocity < 80 { 'o' } else { 'X' };
                    any = true;
                }
            }
            if any {
                let name = kit.piece(*pitch).map_or("?", |p| p.name());
                println!("  {:>3} {:<16} {}", pitch, name, row.iter().collect::<String>());
            }
        }
    }
}
