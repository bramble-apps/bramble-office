//! dump <project.logicx>…: what the reader makes of each project.
fn main() {
    for a in std::env::args().skip(1) {
        match lg_core::open(std::path::Path::new(&a)) {
            Ok(p) => {
                println!("== {} — {:.2} BPM, {}/{}, {}, {} Hz, {} tracks, {} plug-ins, tempo map {:?}", p.name, p.tempo, p.signature.0, p.signature.1, p.key, p.sample_rate, p.tracks.len(), p.plugins.len(), p.tempo_map);
                for t in &p.tracks {
                    println!("  {:>2} {:<24} {:?}  [{}]", t.row, t.name, t.kind, t.plugins.iter().map(|u| u.name.as_str()).collect::<Vec<_>>().join(", "));
                    for a in &t.automation {
                        println!("       automation param {}: {:?}", a.param, a.points);
                    }
                    for r in &t.regions {
                        let a = r.audio.as_ref().map(|a| format!(" audio {} ({:.2}s)", a.file.file_name().unwrap_or_default().to_string_lossy(), a.seconds)).unwrap_or_default();
                        println!("       [{:>6}+{:<6}] {:<26} {} notes{}", r.start, r.length, r.name, r.notes.len(), a);
                    }
                }
            }
            Err(e) => println!("== {a}: {e}"),
        }
    }
}
