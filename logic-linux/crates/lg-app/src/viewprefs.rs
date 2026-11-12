//! Settings ▸ View ▸ Tracks, the parts that act on new tracks: Track Color (Static, or
//! Auto-assign 24 / 96 Colors: each new track takes the next palette colour, within one row of
//! 24 or through all 96), and Region Color ▸ As Track Color turning off Project Settings ▸
//! Recording ▸ Automatically colorize takes.

use std::collections::HashSet;

use lg_core::trackmix::{PALETTE_COLS, PALETTE_ROWS};

use crate::app::App;

const TRACK_COLOR: &str = "View / Tracks / Track Color:";
const REGION_COLOR: &str = "View / Tracks / Region Color:";
const NEXT: &str = "State / Tracks / Next color";
const COLORIZE: &str = "Recording / MIDI / Automatically colorize takes";

#[derive(Default)]
pub struct State {
    known: HashSet<u64>,
    project: Option<std::path::PathBuf>,
    len: usize,
}

fn state() -> &'static std::sync::Mutex<State> {
    static S: std::sync::OnceLock<std::sync::Mutex<State>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

/// The palette colour after `cur` (None: the start): Auto-assign 24 moves right within the
/// row (row 1 to begin with), 96 runs through all the rows.
pub fn next_color(cur: Option<u8>, all: bool) -> u8 {
    let n = (PALETTE_COLS * PALETTE_ROWS) as u8;
    match (cur, all) {
        (None, _) => 0,
        (Some(c), true) => (c + 1) % n,
        (Some(c), false) => {
            let row = c / PALETTE_COLS as u8;
            row * PALETTE_COLS as u8 + (c % PALETTE_COLS as u8 + 1) % PALETTE_COLS as u8
        }
    }
}

const MARKER_COLOR: &str = "View / Tracks / Marker Color:";
const MARKER_NEXT: &str = "State / Markers / Next color";

/// Marker Color: Auto-assign 24 starts at the palette's second row's first colour and moves
/// right along that row; 96 starts at the first colour and runs through all of them.
pub fn next_marker_color(cur: Option<u8>, all: bool) -> u8 {
    match (cur, all) {
        (None, false) => PALETTE_COLS as u8,
        (cur, all) => next_color(cur, all),
    }
}

/// A new marker's colour (None: Static, dark grey).
pub fn marker_color(app: &mut App) -> Option<u8> {
    let mode = app.logic_settings.get(MARKER_COLOR);
    if !mode.starts_with("Auto") {
        return None;
    }
    let cur: Option<u8> = app.proj.0.get(MARKER_NEXT).and_then(|v| v.parse().ok());
    let c = next_marker_color(cur, mode.contains("96"));
    app.proj.0.insert(MARKER_NEXT.into(), c.to_string());
    Some(c)
}

pub fn frame(app: &mut App) {
    if app.logic_settings.get(REGION_COLOR) == "As Track Color" && app.proj.on(COLORIZE) {
        crate::projsettings::set(&mut app.proj, COLORIZE, "0");
    }
    let mode = app.logic_settings.get(TRACK_COLOR);
    let Some(p) = app.project.as_mut() else { return };
    let path = Some(p.path.clone());
    let Ok(mut s) = state().lock() else { return };
    // (a project opened or switched: what's there isn't new)
    if s.project != path {
        s.project = path;
        s.known = p.tracks.iter().map(|t| t.extra.id).filter(|&i| i != 0).collect();
        s.len = p.tracks.len();
        return;
    }
    if p.tracks.len() <= s.len {
        s.len = p.tracks.len();
        return;
    }
    s.len = p.tracks.len();
    let mut next = p.tracks.iter().map(|t| t.extra.id).max().unwrap_or(0);
    lg_core::arrange::assign_ids(&mut p.tracks, &mut next);
    let fresh: Vec<usize> = p.tracks.iter().enumerate().filter(|(_, t)| !s.known.contains(&t.extra.id)).map(|(i, _)| i).collect();
    for t in &p.tracks {
        s.known.insert(t.extra.id);
    }
    drop(s);
    if mode == "Static" || mode.is_empty() {
        return;
    }
    let all = mode.contains("96");
    let mut cur: Option<u8> = app.proj.0.get(NEXT).and_then(|v| v.parse().ok());
    for t in fresh {
        let c = next_color(cur, all);
        cur = Some(c);
        if let Some(x) = crate::tm::set_mut(app, t) {
            x.strip.color = Some(c);
        }
    }
    if let Some(c) = cur {
        app.proj.0.insert(NEXT.into(), c.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_colors() {
        assert_eq!(next_color(None, false), 0);
        assert_eq!(next_color(Some(3), false), 4);
        // 24: stays in its row
        assert_eq!(next_color(Some(23), false), 0);
        assert_eq!(next_color(Some(47), false), 24);
        // 96: on to the next row, then round
        assert_eq!(next_color(Some(23), true), 24);
        assert_eq!(next_color(Some(95), true), 0);
    }

    #[test]
    fn marker_auto_colors() {
        // 24: from the second row's first colour, along that row
        assert_eq!(next_marker_color(None, false), 24);
        assert_eq!(next_marker_color(Some(24), false), 25);
        assert_eq!(next_marker_color(Some(47), false), 24);
        // 96: from the first colour through all
        assert_eq!(next_marker_color(None, true), 0);
        assert_eq!(next_marker_color(Some(40), true), 41);
    }
}
