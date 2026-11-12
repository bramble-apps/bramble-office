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
}
