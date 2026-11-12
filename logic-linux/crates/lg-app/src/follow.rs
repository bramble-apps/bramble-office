//! Live Loops follow actions (a Bramble extra; Logic 13 has none): each cell can say what plays
//! after it has played a number of times: the next or previous cell of its row, the first or the
//! last, a random one, or stop. The follow is queued during the cell's last pass (its ring shows
//! the next cell queued, as a click would), so a click on another cell or a stop still wins.
//! Set in the cell's shortcut menu (Follow Action ▸ …, Play Count ▸ …); kept next to the project
//! in Studio's settings (the Logic file has no place for them).

use std::path::PathBuf;

use crate::app::App;
use crate::liveloops::{Cell, State};

/// What happens after a cell has played its count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Action {
    #[default]
    Off,
    Next,
    Previous,
    First,
    Last,
    Random,
    Stop,
}

pub const ACTIONS: [Action; 7] = [Action::Off, Action::Next, Action::Previous, Action::First, Action::Last, Action::Random, Action::Stop];
pub const COUNTS: [u32; 8] = [1, 2, 3, 4, 6, 8, 12, 16];

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Off => "Off (Keep Looping)",
            Action::Next => "Next Cell",
            Action::Previous => "Previous Cell",
            Action::First => "First Cell",
            Action::Last => "Last Cell",
            Action::Random => "Random Cell",
            Action::Stop => "Stop",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Action::Off => "off",
            Action::Next => "next",
            Action::Previous => "previous",
            Action::First => "first",
            Action::Last => "last",
            Action::Random => "random",
            Action::Stop => "stop",
        }
    }
    fn from_key(k: &str) -> Option<Action> {
        ACTIONS.iter().copied().find(|a| a.key() == k)
    }
}

/// The menu items this file runs.
pub const COMMANDS: &[&str] = &[];

/// A cell's loop length in song ticks (its speed applied).
fn loop_len(c: &Cell, bar: u32) -> u32 {
    ((c.length.max(1) as f32 / c.speed.max(0.01)) as u32).max(1).min(bar.max(1) * 1024)
}

/// The scene the action leads to from `current`, among the row's filled scenes (sorted). None:
/// stop (or nothing to play). `rnd` picks the random one.
pub fn target(scenes: &[usize], current: usize, a: Action, rnd: u64) -> Option<usize> {
    if scenes.is_empty() {
        return None;
    }
    let at = scenes.iter().position(|s| *s == current);
    match a {
        Action::Off | Action::Stop => None,
        Action::First => scenes.first().copied(),
        Action::Last => scenes.last().copied(),
        Action::Next => Some(at.map_or(scenes[0], |i| scenes[(i + 1) % scenes.len()])),
        Action::Previous => Some(at.map_or(scenes[0], |i| scenes[(i + scenes.len() - 1) % scenes.len()])),
        Action::Random => {
            // another cell when there is one
            let others: Vec<usize> = scenes.iter().copied().filter(|s| *s != current).collect();
            let pool = if others.is_empty() { scenes } else { &others[..] };
            Some(pool[(rnd % pool.len() as u64) as usize])
        }
    }
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

/// Each frame (liveloops::update): queues the follow of every playing cell in its last pass.
pub fn step(s: &mut State, pos: f64, bar: u32) {
    let tracks: Vec<usize> = s.plays.keys().copied().collect();
    for t in tracks {
        let Some(p) = s.plays.get(&t).copied() else { continue };
        let Some((sc, st, None)) = p.current else { continue };
        if p.next.is_some() || s.follow_queued.contains(&(t, sc, st)) {
            continue;
        }
        let Some(c) = s.cells.get(&(t, sc)) else { continue };
        if c.follow == Action::Off {
            continue;
        }
        let len = loop_len(c, bar);
        let end = st.saturating_add(len.saturating_mul(c.follow_count.max(1)));
        // in the last pass (at least a bar ahead, so the engine hears the next cell in time)
        if pos + f64::from(len.max(bar)) < f64::from(end) {
            continue;
        }
        let action = c.follow;
        let scenes: Vec<usize> = s.cells.keys().filter(|k| k.0 == t).map(|k| k.1).collect();
        let next = target(&scenes, sc, action, mix(st as u64 ^ ((t as u64) << 32) ^ sc as u64));
        s.follow_queued.insert((t, sc, st));
        let Some(p) = s.plays.get_mut(&t) else { continue };
        match next {
            Some(n) => p.next = Some((n, Some(end))),
            None => {
                if let Some(cur) = p.current.as_mut() {
                    cur.2 = Some(end);
                }
            }
        }
    }
    // forget the cells that are no longer playing from that start
    let playing: Vec<(usize, usize, u32)> = s.plays.iter().filter_map(|(t, p)| p.current.map(|c| (*t, c.0, c.1))).collect();
    s.follow_queued.retain(|k| playing.contains(k));
}

// ---- the cell menu

/// Follow Action ▸ and Play Count ▸ in a cell's shortcut menu (the selected cells, or this one).
pub fn menu(app: &mut App, ui: &mut egui::Ui, cell: (usize, usize)) {
    if !app.live_loops.cells.contains_key(&cell) {
        return;
    }
    let mut cells: Vec<(usize, usize)> = app.live_loops.selected.iter().copied().filter(|k| app.live_loops.cells.contains_key(k)).collect();
    if !cells.contains(&cell) {
        cells = vec![cell];
    }
    let cur = app.live_loops.cells.get(&cell).map(|c| (c.follow, c.follow_count)).unwrap_or_default();
    ui.separator();
    let mut pick_a = None;
    let mut pick_n = None;
    ui.menu_button("Follow Action", |ui| {
        for a in ACTIONS {
            let label = if cur.0 == a { format!("✓ {}", a.label()) } else { a.label().to_string() };
            if ui.button(label).clicked() {
                pick_a = Some(a);
                ui.close();
            }
        }
    });
    ui.add_enabled_ui(cur.0 != Action::Off, |ui| {
        ui.menu_button("Follow After", |ui| {
            for n in COUNTS {
                let txt = if n == 1 { "1 Play".to_string() } else { format!("{n} Plays") };
                let label = if cur.1.max(1) == n { format!("✓ {txt}") } else { txt };
                if ui.button(label).clicked() {
                    pick_n = Some(n);
                    ui.close();
                }
            }
        });
    });
    if pick_a.is_some() || pick_n.is_some() {
        for k in cells {
            if let Some(c) = app.live_loops.cells.get_mut(&k) {
                if let Some(a) = pick_a {
                    c.follow = a;
                }
                if let Some(n) = pick_n {
                    c.follow_count = n;
                }
            }
        }
        app.dirty = true;
    }
}

// ---- kept with the project

fn path_for(project: &std::path::Path) -> Option<PathBuf> {
    let side = crate::hosting::sidecar_path(project)?;
    Some(side.parent()?.parent()?.join("live-loops-follow").join(side.file_name()?))
}

pub fn text(s: &State) -> String {
    let mut out = String::from("# Bramble Studio: Live Loops follow actions (track, scene, action, plays)\n");
    for ((t, sc), c) in &s.cells {
        if c.follow != Action::Off {
            out += &format!("follow\t{t}\t{sc}\t{}\t{}\n", c.follow.key(), c.follow_count.max(1));
        }
    }
    out
}

pub fn apply(s: &mut State, text: &str) {
    for l in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = l.split('\t').collect();
        if let ["follow", t, sc, a, n] = w.as_slice() {
            let (Ok(t), Ok(sc), Some(a)) = (t.parse::<usize>(), sc.parse::<usize>(), Action::from_key(a)) else { continue };
            if let Some(c) = s.cells.get_mut(&(t, sc)) {
                c.follow = a;
                c.follow_count = n.parse().unwrap_or(1).max(1);
            }
        }
    }
}

/// At save: the project's follow actions into Studio's settings (none: the file removed).
pub fn save(app: &App) {
    let Some(path) = app.file.as_ref().and_then(|f| path_for(&f.0)) else { return };
    let t = text(&app.live_loops);
    if t.lines().all(|l| l.starts_with('#')) {
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(path, t);
}

/// When the grid is read: the follow actions back.
pub fn load(app: &App, s: &mut State) {
    let Some(path) = app.file.as_ref().and_then(|f| path_for(&f.0)) else { return };
    if let Ok(t) = std::fs::read_to_string(path) {
        apply(s, &t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::liveloops::Play;

    #[test]
    fn targets_wrap_and_pick_others() {
        let sc = [0, 2, 5];
        assert_eq!(target(&sc, 2, Action::Next, 0), Some(5));
        assert_eq!(target(&sc, 5, Action::Next, 0), Some(0));
        assert_eq!(target(&sc, 0, Action::Previous, 0), Some(5));
        assert_eq!(target(&sc, 2, Action::First, 0), Some(0));
        assert_eq!(target(&sc, 2, Action::Last, 0), Some(5));
        assert_eq!(target(&sc, 2, Action::Stop, 0), None);
        for r in 0..20 {
            assert_ne!(target(&sc, 2, Action::Random, r), Some(2));
        }
        assert_eq!(target(&[3], 3, Action::Random, 7), Some(3));
    }

    fn grid() -> State {
        let mut s = State::default();
        for sc in 0..3 {
            s.cells.insert((0, sc), Cell { length: 3840, follow: Action::Next, follow_count: 2, ..Default::default() });
        }
        s.plays.insert(0, Play { current: Some((0, 0, None)), ..Default::default() });
        s
    }

    #[test]
    fn follow_queues_in_the_last_pass() {
        let mut s = grid();
        step(&mut s, 100.0, 3840);
        assert_eq!(s.plays[&0].next, None, "first of two passes: nothing yet");
        step(&mut s, 3840.0 + 10.0, 3840);
        assert_eq!(s.plays[&0].next, Some((1, Some(7680))));
        // un-queued by hand: not queued again for the same start
        s.plays.get_mut(&0).unwrap().next = None;
        step(&mut s, 3840.0 + 20.0, 3840);
        assert_eq!(s.plays[&0].next, None);
    }

    #[test]
    fn stop_ends_the_cell_and_off_loops() {
        let mut s = grid();
        s.cells.get_mut(&(0, 0)).unwrap().follow = Action::Stop;
        s.cells.get_mut(&(0, 0)).unwrap().follow_count = 1;
        step(&mut s, 10.0, 3840);
        assert_eq!(s.plays[&0].current, Some((0, 0, Some(3840))));
        let mut s = grid();
        s.cells.get_mut(&(0, 0)).unwrap().follow = Action::Off;
        step(&mut s, 7000.0, 3840);
        assert_eq!(s.plays[&0].next, None);
        assert_eq!(s.plays[&0].current, Some((0, 0, None)));
    }

    #[test]
    fn saved_text_round_trips() {
        let mut s = grid();
        s.cells.get_mut(&(0, 1)).unwrap().follow = Action::Random;
        let t = text(&s);
        let mut back = grid();
        for c in back.cells.values_mut() {
            c.follow = Action::Off;
            c.follow_count = 1;
        }
        apply(&mut back, &t);
        assert_eq!(back.cells[&(0, 1)].follow, Action::Random);
        assert_eq!(back.cells[&(0, 2)].follow_count, 2);
    }
}
