//! Makes a test package: the first MIDI region copied N bars later (as an Option-drag in Studio
//! would), saved into a copy of the package. `copy_region <in.logicx> <out.logicx> [bars]`
use std::path::Path;

fn copy_dir(a: &Path, b: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(b)?;
    for e in std::fs::read_dir(a)? {
        let e = e?;
        let to = b.join(e.file_name());
        if e.file_type()?.is_dir() { copy_dir(&e.path(), &to)? } else { std::fs::copy(e.path(), to).map(|_| ())? }
    }
    Ok(())
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (src, dst) = (Path::new(&a[1]), Path::new(&a[2]));
    let bars: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(4);
    copy_dir(src, dst).expect("copy");
    let (_, alt) = lg_core::project::alternatives(dst).into_iter().next().expect("alternative");
    let pd = alt.join("ProjectData");
    let original = std::fs::read(&pd).unwrap();
    let mut p = lg_core::project::read_project_data(&original).unwrap();
    let t = p.tracks.iter_mut().find(|t| t.regions.iter().any(|r| r.audio.is_none() && r.source.is_some())).expect("a MIDI region");
    let mut c = t.regions.iter().find(|r| r.audio.is_none() && r.source.is_some()).unwrap().clone();
    let step = bars * lg_core::project::bar_ticks(p.signature);
    c.origin = c.source.take();
    c.start += step;
    for n in &mut c.notes {
        n.pos += step;
    }
    println!("copied '{}' ({} notes) to tick {}", c.name, c.notes.len(), c.start);
    t.regions.push(c);
    let saved = lg_core::save::save(&p, &original).unwrap();
    std::fs::write(&pd, &saved.data).unwrap();
    lg_core::save::update_metadata(&alt, &p);
    let back = lg_core::project::read_project_data(&saved.data).unwrap();
    for t in &back.tracks {
        for r in &t.regions {
            println!("  {} / {} at {} ({} notes)", t.name, r.name, r.start, r.notes.len());
        }
    }
    println!("unsaved: {:?}", saved.unsaved);
}
