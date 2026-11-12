//! The Score's own data (note attributes, symbols, bar and repeat signs, line and page breaks,
//! text styles, staff styles per track) kept with the project: Logic stores it inside the
//! region's score data, which Studio can't write into the .logicx yet, so it lives in a file
//! beside Studio's other project extras and comes back when the project opens.

use crate::app::App;

fn path(app: &App) -> Option<std::path::PathBuf> {
    let side = crate::hosting::sidecar_path(&app.file.as_ref()?.0)?;
    Some(side.parent()?.parent()?.join("score-extras").join(side.file_name()?))
}

fn text(p: &lg_core::Project) -> String {
    let body = p.score.to_text(|id| p.tracks.iter().position(|t| t.extra.id == id));
    format!("# Bramble Studio: Score data\n{body}")
}

/// On Save (removed when the score has nothing of its own).
pub fn save_sidecar(app: &App) {
    let (Some(path), Some(p)) = (path(app), app.project.as_ref()) else { return };
    let t = text(p);
    if t.lines().all(|l| l.starts_with('#')) {
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(path, t);
}

/// After a project opens: its score data back.
pub fn after_open(app: &mut App) {
    let Some(path) = path(app) else { return };
    let Ok(t) = std::fs::read_to_string(path) else { return };
    if let Some(p) = app.project.as_mut() {
        let ids: Vec<u64> = p.tracks.iter().map(|t| t.extra.id).collect();
        let mut s = lg_core::score::Score::default();
        if s.apply_text(&t, |i| ids.get(i).copied()) {
            p.score = s;
        }
    }
}

/// Import Project Settings: the Score data another project keeps, its tracks by index.
pub fn score_of(project: &std::path::Path) -> Option<lg_core::score::Score> {
    let side = crate::hosting::sidecar_path(project)?;
    let f = side.parent()?.parent()?.join("score-extras").join(side.file_name()?);
    let t = std::fs::read_to_string(f).ok()?;
    let mut s = lg_core::score::Score::default();
    s.apply_text(&t, |i| Some(i as u64 + 1_000_000)).then_some(s)
}

/// The Import Settings dialog's Score checkboxes.
pub const IMPORT_PANES: [&str; 3] = ["Score Sets", "Staff Styles", "Text Styles"];

/// Imports the ticked Score panes of `other` (from `score_of`): every score set (its
/// instruments by track position here), every staff style and every text style, a same-named
/// one replaced. One undo step; false when nothing was ticked.
pub fn import(app: &mut App, other: &lg_core::score::Score, panes: &[&str]) -> bool {
    if !panes.iter().any(|p| IMPORT_PANES.contains(p)) || app.project.is_none() {
        return false;
    }
    app.checkpoint("Import Project Settings");
    let Some(p) = app.project.as_mut() else { return false };
    let ids: Vec<u64> = p.tracks.iter().map(|t| t.extra.id).collect();
    if panes.contains(&"Score Sets") {
        for set in &other.sets {
            let mut set = set.clone();
            set.entries.retain_mut(|e| match ids.get((e.track - 1_000_000) as usize) {
                Some(&id) => {
                    e.track = id;
                    true
                }
                None => false,
            });
            p.score.sets.retain(|x| x.name != set.name);
            p.score.sets.push(set);
        }
    }
    if panes.contains(&"Staff Styles") {
        for d in &other.staff_styles {
            p.score.staff_styles.retain(|x| x.name != d.name);
            p.score.staff_styles.push(d.clone());
        }
    }
    if panes.contains(&"Text Styles") {
        for t in &other.text_styles {
            p.score.set_style(t.clone());
        }
    }
    app.after_edit();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_text_keeps_tracks_by_index() {
        let mut p = lg_core::Project::default();
        p.tracks.push(lg_core::Track::new("x", lg_core::TrackKind::Instrument));
        p.tracks.push(lg_core::Track::new("x", lg_core::TrackKind::Instrument));
        p.tracks[0].extra.id = 11;
        p.tracks[1].extra.id = 22;
        p.score.track_mut(22).style = "Bass".into();
        p.score.breaks.insert(3, lg_core::score::Break::Line);
        let t = text(&p);
        let mut s = lg_core::score::Score::default();
        assert!(s.apply_text(&t, |i| [7u64, 8][..].get(i).copied()));
        assert_eq!(s.tracks[&8].style, "Bass");
        assert_eq!(s.breaks.get(&3), Some(&lg_core::score::Break::Line));
    }

    #[test]
    fn import_brings_sets_and_styles() {
        let mut app = crate::tm::test_app();
        let mut other = lg_core::score::Score::default();
        other.sets.push(lg_core::score::ScoreSet::new("Winds", &[1_000_000, 1_000_099]));
        other.staff_styles.push(lg_core::staffstyle::StaffStyleDef::dual("Imported Duo"));
        other.set_style(lg_core::score::TextStyle { name: "Big".into(), family: "Serif".into(), size: 30.0, bold: true, italic: false, underline: false });
        assert!(!import(&mut app, &other, &["MIDI"]));
        assert!(import(&mut app, &other, &["Score Sets", "Staff Styles", "Text Styles"]));
        let p = app.project.as_ref().unwrap();
        let set = p.score.sets.iter().find(|s| s.name == "Winds").unwrap();
        // the first track here takes the first entry; a track it doesn't have is left out
        assert_eq!(set.entries.len(), 1);
        assert_eq!(set.entries[0].track, p.tracks[0].extra.id);
        assert!(p.score.staff_styles.iter().any(|d| d.name == "Imported Duo"));
        assert!(p.score.text_styles.iter().any(|t| t.name == "Big"));
    }
}
