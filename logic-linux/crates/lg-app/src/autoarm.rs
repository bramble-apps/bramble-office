//! Settings ▸ Recording ▸ Auto Record Enable: with no track record-enabled, the focused track
//! (The Focused Track) or every selected track (All Selected Tracks) records, shown by a red R
//! on its Record Enable button. The focused track is the selected one, or the first of several.

use crate::app::App;

pub const COMMANDS: &[&str] = &[];

pub const KEY: &str = "Recording /  / Auto Record Enable:";

/// The focused track: the one selected, or the first of a multiple selection.
pub fn focused(selected_track: Option<usize>, selection: &[usize]) -> Option<usize> {
    selected_track.or_else(|| selection.iter().copied().min())
}

/// The tracks auto record-enabled (sorted): none while any track is record-enabled.
pub fn implied_for(setting: &str, any_armed: bool, selected_track: Option<usize>, selection: &[usize]) -> Vec<usize> {
    if any_armed {
        return Vec::new();
    }
    let mut v: Vec<usize> = if setting == "All Selected Tracks" { selection.to_vec() } else { Vec::new() };
    v.extend(focused(selected_track, selection));
    v.sort();
    v.dedup();
    v
}

pub fn implied(app: &App) -> Vec<usize> {
    implied_for(&app.logic_settings.get(KEY), !app.armed.is_empty(), app.selected_track, &app.ta.selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_or_all_selected() {
        assert_eq!(focused(None, &[5, 2, 7]), Some(2));
        assert_eq!(focused(Some(7), &[5, 2, 7]), Some(7));
        assert_eq!(implied_for("The Focused Track", false, Some(7), &[2, 5, 7]), vec![7]);
        assert_eq!(implied_for("All Selected Tracks", false, Some(7), &[2, 5, 7]), vec![2, 5, 7]);
        assert_eq!(implied_for("All Selected Tracks", true, Some(7), &[2, 5, 7]), Vec::<usize>::new());
        assert_eq!(implied_for("The Focused Track", false, None, &[4, 3]), vec![3]);
    }
}
