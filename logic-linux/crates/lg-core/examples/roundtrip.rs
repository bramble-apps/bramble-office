//! roundtrip <project.logicx>…: saving unchanged gives the same bytes; edits come back.
use lg_core::{project, save};

fn main() {
    for a in std::env::args().skip(1) {
        let pkg = project::package_of(std::path::Path::new(&a));
        let Some((_, alt)) = project::alternatives(&pkg).into_iter().next() else { continue };
        let orig = std::fs::read(alt.join("ProjectData")).unwrap();
        let p = project::read_project_data(&orig).unwrap();
        let same = save::save(&p, &orig).unwrap();
        let identical = same.data == orig;
        // edits: every note up a semitone, the first region a bar later, the last note gone
        let mut e = p.clone();
        let mut moved = None;
        for t in &mut e.tracks {
            for r in &mut t.regions {
                for n in &mut r.notes {
                    n.pitch = (n.pitch + 1).min(127);
                }
            }
        }
        if let Some(r) = e.tracks.iter_mut().flat_map(|t| t.regions.iter_mut()).find(|r| r.audio.is_none() && !r.notes.is_empty()) {
            r.start += 3840;
            for n in &mut r.notes {
                n.pos += 3840;
            }
            r.notes.pop();
            moved = Some((r.name.clone(), r.start, r.notes.len()));
        }
        e.tempo = 97.5;
        let saved = save::save(&e, &orig).unwrap();
        let back = project::read_project_data(&saved.data).unwrap();
        let ok_walk = lg_core::chunks::walk(&saved.data).is_ok();
        let want: Vec<u8> = e.tracks.iter().flat_map(|t| t.regions.iter()).flat_map(|r| r.notes.iter().map(|n| n.pitch)).collect();
        let got: Vec<u8> = back.tracks.iter().flat_map(|t| t.regions.iter()).flat_map(|r| r.notes.iter().map(|n| n.pitch)).collect();
        let region_ok = moved.as_ref().is_none_or(|(name, start, n)| back.tracks.iter().flat_map(|t| t.regions.iter()).any(|r| &r.name == name && r.start == *start && r.notes.len() == *n));
        let tempo_ok = (back.tempo - 97.5).abs() < 0.001;
        println!("{:<40} unchanged-identical={identical} walk={ok_walk} tempo={tempo_ok} notes-transposed={} region-moved={region_ok} ({} bytes → {})", pkg.file_name().unwrap().to_string_lossy(), want == got, orig.len(), saved.data.len());
    }
}
