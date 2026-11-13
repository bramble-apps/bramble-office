//! Drum notation from mapped instruments: a track routed to a Mapped Instrument (Environment)
//! shows its notes in a mapped staff style by the instrument's Head, Rel. Pos. and Group
//! columns; any other track by the General MIDI defaults. A note in no drum group isn't shown.
//! The drum groups are the project's (predefined Kick, Snare, Hi-Hat, Toms, Cymbals,
//! Percussion, and the ones New Group adds); each instrument keeps its own map.

use crate::app::App;
use lg_core::environment::{Kind, MapRow, DRUM_GROUPS};
use std::sync::Arc;

/// Where the project's own drum groups are kept (one per line).
const GROUPS_KEY: &str = "Score / Drum Groups";

/// The map a track's notes are drawn with: its mapped instrument's rows, else the GM defaults.
pub fn rows_for(app: &App, t: usize) -> Arc<Vec<MapRow>> {
    let obj = app.hub.env.track_obj.get(&t).and_then(|id| app.hub.env.env.obj(*id)).filter(|o| o.kind == Kind::MappedInstrument);
    Arc::new((0..128u8).map(|n| obj.map_or_else(|| MapRow::default_for(n), |o| o.map_row(n))).collect())
}

/// Each key's place on the staff, in half spaces up from the bottom line (lg-notation).
pub fn places(rows: &[MapRow]) -> Arc<Vec<i32>> {
    Arc::new(rows.iter().map(|r| 8 + r.relpos as i32).collect())
}

/// Whether a key shows (it's in a drum group).
pub fn shown(rows: &[MapRow], pitch: u8) -> bool {
    rows.get(pitch as usize).is_some_and(|r| !r.group.is_empty())
}

/// The key a click at a staff place inserts: the lowest key in a group drawn there.
pub fn pitch_at(rows: &[MapRow], pos: i32) -> Option<u8> {
    (0..128u8).find(|&n| shown(rows, n) && 8 + rows[n as usize].relpos as i32 == pos)
}

/// The project's drum groups: the predefined ones, then its own.
pub fn groups(app: &App) -> Vec<String> {
    let mut out: Vec<String> = DRUM_GROUPS.iter().map(|g| g.to_string()).collect();
    out.extend(own_groups(app));
    out
}

fn own_groups(app: &App) -> Vec<String> {
    app.proj.0.get(GROUPS_KEY).map(|t| t.lines().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect()).unwrap_or_default()
}

fn set_own(app: &mut App, v: &[String]) {
    if v.is_empty() {
        app.proj.0.remove(GROUPS_KEY);
    } else {
        app.proj.0.insert(GROUPS_KEY.into(), v.join("\n"));
    }
}

/// Group ▸ New Group: "Group 1", "Group 2" … (returns its name).
pub fn new_group(app: &mut App) -> String {
    let all = groups(app);
    let name = (1..).map(|k| format!("Group {k}")).find(|n| !all.contains(n)).unwrap_or_default();
    let mut own = own_groups(app);
    own.push(name.clone());
    set_own(app, &own);
    name
}

/// Renames a drum group everywhere: the project's list and every mapped instrument's rows.
/// False when the name is empty or taken.
pub fn rename_group(app: &mut App, old: &str, new: &str) -> bool {
    let new = new.trim().replace('\t', " ");
    if new.is_empty() || new == old || groups(app).contains(&new) {
        return false;
    }
    let mut own = own_groups(app);
    match own.iter_mut().find(|g| *g == old) {
        Some(g) => *g = new.clone(),
        // (a predefined group renamed: the project keeps the new name as its own)
        None => own.push(new.clone()),
    }
    set_own(app, &own);
    let ids: Vec<_> = app.hub.env.env.objects.iter().filter(|o| o.kind == Kind::MappedInstrument).map(|o| o.id).collect();
    for id in ids {
        let Some(o) = app.hub.env.env.obj_mut(id) else { continue };
        for n in 0..128u8 {
            let mut r = o.map_row(n);
            if r.group == old {
                r.group = new.clone();
                o.set_map_row(n, &r);
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gm_map_places_and_groups() {
        let rows: Vec<MapRow> = (0..128u8).map(MapRow::default_for).collect();
        let p = places(&rows);
        // the kick in the bottom space, the snare in the third, the closed hi-hat above the staff
        assert_eq!((p[36], p[38], p[42]), (1, 5, 9));
        // they agree with lg-notation's General MIDI drum places
        for n in 35..=81u8 {
            assert_eq!(p[n as usize], lg_notation::drum_position(n).0, "{n}");
        }
        assert!(shown(&rows, 36) && !shown(&rows, 20));
        assert_eq!(pitch_at(&rows, 1), Some(35));
        assert_eq!(pitch_at(&rows, 5), Some(37));
    }

    #[test]
    fn new_and_renamed_groups_are_project_wide() {
        let mut app = crate::tm::test_app();
        assert_eq!(groups(&app)[..5], ["Kick", "Snare", "Hi-Hat", "Toms", "Cymbals"]);
        let g = new_group(&mut app);
        assert_eq!(g, "Group 1");
        assert_eq!(new_group(&mut app), "Group 2");
        let id = app.hub.env.env.add(Kind::MappedInstrument, 0, 10.0, 10.0);
        let o = app.hub.env.env.obj_mut(id).unwrap();
        let mut r = o.map_row(39);
        r.group = "Group 1".into();
        o.set_map_row(39, &r);
        assert!(rename_group(&mut app, "Group 1", "Claps"));
        assert!(!rename_group(&mut app, "Claps", "Snare"));
        assert!(groups(&app).contains(&"Claps".to_string()) && !groups(&app).contains(&"Group 1".to_string()));
        assert_eq!(app.hub.env.env.obj(id).unwrap().map_row(39).group, "Claps");
    }
}
