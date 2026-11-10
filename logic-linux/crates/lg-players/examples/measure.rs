//! measure <project.logicx>…: statistics of each Session Player region's pre-rendered notes (per
//! kit piece per bar, velocities, timing against the sixteenth grid) — to compare our generator
//! with Logic's. Reads the projects; nothing of them is kept.
use std::collections::BTreeMap;

fn main() {
    for a in std::env::args().skip(1) {
        let p = match lg_core::open(std::path::Path::new(&a)) {
            Ok(p) => p,
            Err(e) => {
                println!("== {a}: {e}");
                continue;
            }
        };
        let bar = lg_core::project::bar_ticks(p.signature);
        println!("== {} {}/{}", p.name, p.signature.0, p.signature.1);
        // the players' settings and chords, as lg_players reads them
        let pkg = lg_core::project::package_of(std::path::Path::new(&a));
        if let Some((_, alt)) = lg_core::project::alternatives(&pkg).first() {
            if let Ok(data) = std::fs::read(alt.join("ProjectData")) {
                for (id, s) in lg_players::genm::all_settings(&data) {
                    println!("  settings of EvSq {id:#x}: {s:?}");
                }
                for (id, ch) in lg_players::genm::all_chords(&data) {
                    println!("  chords in EvSq {id:#x}: {}", ch.iter().map(|c| format!("{}@{}", c.chord.name(), c.pos)).collect::<Vec<_>>().join(" "));
                }
            }
        }
        for t in &p.tracks {
            for r in &t.regions {
                if !(r.name.starts_with("Drummer") || r.name.contains("Player")) {
                    continue;
                }
                // Logic's bar 1 is 38400, a bar of 4/4 after the reader's tick 0: a region placed at
                // the song's start has its first hits 3840 ticks in
                let mut origin = r.start;
                if r.notes.iter().map(|n| n.pos).min().unwrap_or(0) + 120 >= r.start + 3840 {
                    origin += 3840;
                }
                let last = r.notes.iter().map(|n| n.pos).max().unwrap_or(origin);
                let bars = ((last.saturating_sub(origin)) as f32 / bar as f32).ceil().max(1.0);
                println!("  {} [{}+{}] {} notes, {:.1} bars, ch {:?}", r.name, r.start, r.length, r.notes.len(), bars, r.notes.first().map(|n| n.channel));
                let mut by: BTreeMap<u8, Vec<&lg_core::events::Note>> = BTreeMap::new();
                for n in &r.notes {
                    by.entry(n.pitch).or_default().push(n);
                }
                for (pitch, ns) in &by {
                    let v: Vec<f32> = ns.iter().map(|n| n.velocity as f32).collect();
                    let m = v.iter().sum::<f32>() / v.len() as f32;
                    let sd = (v.iter().map(|x| (x - m).powi(2)).sum::<f32>() / v.len() as f32).sqrt();
                    let off: Vec<i32> = ns.iter().map(|n| { let rel = n.pos as i32 - origin as i32; let g = 240; let q = ((rel as f32 / g as f32).round() as i32) * g; rel - q }).collect();
                    let mo = off.iter().sum::<i32>() as f32 / off.len() as f32;
                    let sdo = (off.iter().map(|x| (*x as f32 - mo).powi(2)).sum::<f32>() / off.len() as f32).sqrt();
                    let lens: Vec<u32> = ns.iter().map(|n| n.len).collect();
                    println!("    {:>3} x{:<3} {:.2}/bar vel {:.1}±{:.1} [{}-{}] off {:+.1}±{:.1} len {:?}", pitch, ns.len(), ns.len() as f32 / bars, m, sd,
                        v.iter().cloned().fold(127.0, f32::min), v.iter().cloned().fold(0.0, f32::max), mo, sdo, &lens[..lens.len().min(6)]);
                }
                if std::env::var("GRID").is_ok() {
                    // a step grid per bar: one row per pitch, sixteenths
                    let steps = bar / 240;
                    for b in 0..bars.ceil() as u32 {
                        println!("    bar {}", b + 1);
                        for (pitch, ns) in &by {
                            let mut row = vec!['.'; steps as usize];
                            let mut any = false;
                            for n in ns {
                                let rel = n.pos as i64 - origin as i64 - (b * bar) as i64;
                                if rel >= -120 && rel < bar as i64 - 120 {
                                    let s = ((rel as f32) / 240.0).round().max(0.0) as usize;
                                    if s < row.len() { row[s] = if n.velocity < 40 { 'g' } else if n.velocity < 80 { 'o' } else { 'X' }; any = true; }
                                }
                            }
                            if any { println!("      {:>3} {}", pitch, row.iter().collect::<String>()); }
                        }
                    }
                }
                if std::env::var("RAW").is_ok() {
                    for n in &r.notes { println!("      {:>6} {:>3} v{:>3} l{}", n.pos as i64 - origin as i64, n.pitch, n.velocity, n.len); }
                }
            }
        }
    }
}
