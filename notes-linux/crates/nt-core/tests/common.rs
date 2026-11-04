//! Shared test helpers: where the NO1 research samples are.
#![allow(dead_code)]

use std::path::PathBuf;

/// The newest `~/CascadeProjects/notes-research-src/partial-N/Notes-Research` (or $NT_RESEARCH).
pub fn research() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NT_RESEARCH") {
        return Some(PathBuf::from(p));
    }
    let base = PathBuf::from(std::env::var("HOME").ok()?).join("CascadeProjects/notes-research-src");
    let mut best: Option<(u32, PathBuf)> = None;
    for e in std::fs::read_dir(&base).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(n) = name.strip_prefix("partial-").and_then(|n| n.parse::<u32>().ok()) else { continue };
        let p = e.path().join("Notes-Research");
        if p.join("sample-files").is_dir() && best.as_ref().is_none_or(|(b, _)| n > *b) {
            best = Some((n, p));
        }
    }
    best.map(|(_, p)| p)
}

/// The research dir, or skip the test (prints why) when it isn't on this machine.
#[macro_export]
macro_rules! research_or_skip {
    () => {
        match common::research() {
            Some(p) => p,
            None => {
                eprintln!("skipped: NO1 research samples not found");
                return;
            }
        }
    };
}

pub fn json(p: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// Scratch dir for one test (under target/, removed first).
pub fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A group.com.apple.notes-like folder in `scratch(name)`: the research store, plus the media files
/// where Notes keeps them (originals taken from the research's Markdown export, named by attachment UUID).
pub fn group_container(r: &std::path::Path, name: &str) -> PathBuf {
    let d = scratch(name);
    std::fs::copy(r.join("sample-files/store/NoteStore-BrambleResearch-only.sqlite"), d.join("NoteStore.sqlite")).unwrap();
    let exp = r.join("sample-files/exports/markdown/Attachments");
    let acct = d.join("Accounts/LocalAccount");
    // ICMedia identifier / generation / filename, from the store
    for (src, id, generation, file) in [
        ("EB5DCE00-5ED3-4664-ACDD-19C1D0C64547.png", "81316241-55AD-42C2-9271-46236604104F", "1_8F7F79D8-BEFB-43A8-9C55-1CBF6EB1E4D6", "bramble-gradient.png"),
        ("0C2A7602-08D5-4697-9FF3-7B2EE9373434.pdf", "635B6E47-FD4E-440F-8BAF-4E1C567758B8", "1_E4E8460C-38BA-49B9-973B-8007560F364F", "bramble-doc.pdf"),
        ("F23FE9B9-D517-485E-B87D-D60529CE2089.txt", "618DBA50-A810-4429-8A0C-41086942DAE0", "1_87F5D1C3-A4BB-41B9-8F1A-EA78426EC9F4", "bramble-text.txt"),
    ] {
        let dir = acct.join("Media").join(id).join(generation);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(exp.join(src), dir.join(file)).unwrap();
    }
    // the drawing's fallback image
    let fb = acct.join("FallbackImages/AFF6AA77-04CC-47C7-A9EB-6FD50AA7A563/1_95D2053F-C60B-4C80-84CA-8EF1E7DFFF37");
    std::fs::create_dir_all(&fb).unwrap();
    std::fs::copy(exp.join("AFF6AA77-04CC-47C7-A9EB-6FD50AA7A563.png"), fb.join("FallbackImage.png")).unwrap();
    d
}

/// The NO3 drawing samples (`out-NO3/drawings`), or $NT_NO3_DRAWINGS.
pub fn no3_drawings() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NT_NO3_DRAWINGS") {
        return Some(PathBuf::from(p));
    }
    let p = PathBuf::from(std::env::var("HOME").ok()?).join("CascadeProjects/notes-research-src/no3/Notes-Research/out-NO3/drawings");
    p.join("DRAWINGS-FORMAT.txt").is_file().then_some(p)
}

/// A NO3 sample's attachment folder (the one holding Paper.bundle) and attachment id.
pub fn no3_sample(dir: &std::path::Path, sample: &str) -> (PathBuf, String) {
    let s = std::fs::read_dir(dir).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with(sample)).unwrap().path();
    let a = std::fs::read_dir(&s).unwrap().flatten().find(|e| e.path().join("Paper.bundle").is_dir()).unwrap();
    (a.path(), a.file_name().to_string_lossy().into_owned())
}
