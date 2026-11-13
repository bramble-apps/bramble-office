//! The Chord track as Logic edits it: chords with a position and length (`Project::global`
//! `chords`), chord groups and loops. A click selects (a chord inside a group selects the group;
//! double-click picks the chord in it), Shift-click adds, a drag moves (the neighbours make room:
//! a chord dropped inside another splits it), the lower edges trim (lengthening from the left adds
//! another instance of the chord), the boundary of a chord inside a group is a Junction, the upper
//! right edge loops. The (+) button adds a chord at the playhead, the Pencil adds one at the click,
//! Tab adds the next one. A double-click opens Edit Chord (root, type, extensions, bass, preview).
//! The shortcut menu has Create/Edit Chord, Chord Progressions, Cut/Copy/Paste/Delete, Group and
//! Ungroup, Halve/Double Chord Rhythm, Transpose, Analyze Key Signature and the selection
//! commands. The players follow the chords (`players::chords`).

use std::collections::BTreeSet;

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Rect, Sense, Stroke};
use lg_players::chords::{self as lc, Chord, Progression};

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Create Chord",
    "Edit Chord…",
    "Cut Chords",
    "Copy Chords",
    "Paste Chords",
    "Delete Chords",
    "Cut All Chords",
    "Copy All Chords",
    "Delete All Chords",
    "Group Chords",
    "Ungroup Chords",
    "Select All Chord Groups",
    "Select All Chord Groups In Cycle Range",
    "Select All Chords In Cycle Range",
    "Select All Chords",
    "Halve Chord Rhythm",
    "Double Chord Rhythm",
    "Global Chords: Analyze Key Signature",
];

const NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

// ---------------------------------------------------------------- the model

/// The Chord track's contents: chords (tick, length, name), groups (start, length) and loops
/// (the looped unit's start, the loop's end).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ct {
    pub chords: Vec<(u32, u32, String)>,
    pub groups: Vec<(u32, u32)>,
    pub loops: Vec<(u32, u32)>,
}

impl Ct {
    pub fn load(p: &lg_core::Project) -> Ct {
        Ct { chords: p.global.chords.clone(), groups: p.global.chord_groups.clone(), loops: p.global.chord_loops.clone() }
    }

    pub fn store(self, p: &mut lg_core::Project) {
        p.global.chords = self.chords;
        p.global.chord_groups = self.groups;
        p.global.chord_loops = self.loops;
    }

    /// Sorted, no empty chords; groups shrink to the chords inside and vanish when empty.
    pub fn norm(&mut self) {
        self.chords.retain(|c| c.1 > 0);
        self.chords.sort_by_key(|c| c.0);
        let chords = &self.chords;
        self.groups = self
            .groups
            .iter()
            .filter_map(|&(a, l)| {
                let inside: Vec<_> = chords.iter().filter(|c| c.0 >= a && c.0 < a + l).collect();
                let s = inside.first()?.0;
                let e = inside.iter().map(|c| c.0 + c.1).max()?;
                Some((s, e - s))
            })
            .collect();
        self.groups.sort();
        self.groups.dedup();
        let starts: Vec<u32> = self.chords.iter().map(|c| c.0).collect();
        self.loops.retain(|l| starts.contains(&l.0));
    }

    /// Makes room for something at `a..b`: chords inside go, the ones overlapping are trimmed, one
    /// spanning the whole range is split around it.
    pub fn carve(&mut self, a: u32, b: u32) {
        let mut out = Vec::new();
        for (s, l, n) in self.chords.drain(..) {
            let e = s + l;
            if e <= a || s >= b {
                out.push((s, l, n));
                continue;
            }
            if s < a {
                out.push((s, a - s, n.clone()));
            }
            if e > b {
                out.push((b, e - b, n));
            }
        }
        self.chords = out;
        self.norm();
    }

    /// Puts a chord at `pos` (overwriting what is there); its index.
    pub fn insert(&mut self, pos: u32, len: u32, name: &str) -> usize {
        self.carve(pos, pos + len);
        self.chords.push((pos, len.max(1), name.to_string()));
        self.norm();
        self.index_at(pos).unwrap_or(0)
    }

    pub fn index_at(&self, pos: u32) -> Option<usize> {
        self.chords.iter().position(|c| c.0 == pos)
    }

    /// The chord sounding at `tick`.
    pub fn at(&self, tick: u32) -> Option<usize> {
        self.chords.iter().position(|c| c.0 <= tick && tick < c.0 + c.1)
    }

    /// A new chord's length: one bar, or up to the next chord within that bar.
    pub fn default_len(&self, pos: u32, bar: u32) -> u32 {
        let next = self.chords.iter().map(|c| c.0).filter(|&s| s > pos).min().unwrap_or(u32::MAX);
        bar.min(next - pos).max(1)
    }

    /// The group chord `i` is in.
    pub fn group_of(&self, i: usize) -> Option<usize> {
        let s = self.chords.get(i)?.0;
        self.groups.iter().position(|&(a, l)| s >= a && s < a + l)
    }

    /// The chords of group `g`.
    pub fn in_group(&self, g: usize) -> Vec<usize> {
        let Some(&(a, l)) = self.groups.get(g) else { return Vec::new() };
        (0..self.chords.len()).filter(|&i| self.chords[i].0 >= a && self.chords[i].0 < a + l).collect()
    }

    /// Removes chords (Edit ▸ Delete): their place stays empty.
    pub fn delete(&mut self, sel: &BTreeSet<usize>) {
        let mut k = 0;
        self.chords.retain(|_| {
            k += 1;
            !sel.contains(&(k - 1))
        });
        self.norm();
    }

    /// Cuts chords: the chord before extends into the gap; when none touches it (the first of a
    /// group), a new instance of the next chord fills it.
    pub fn cut(&mut self, sel: &BTreeSet<usize>) {
        for &i in sel.iter().rev() {
            if i >= self.chords.len() {
                continue;
            }
            let (s, l, _) = self.chords.remove(i);
            let prev = (0..self.chords.len()).find(|&j| self.chords[j].0 + self.chords[j].1 == s);
            let next = (0..self.chords.len()).find(|&j| self.chords[j].0 == s + l);
            if let Some(j) = prev {
                self.chords[j].1 += l;
            } else if let Some(j) = next {
                let n = self.chords[j].2.clone();
                self.chords.push((s, l, n));
            }
        }
        self.norm();
    }

    /// Moves the selected chords by `delta` ticks (at most back to 0): they keep their length,
    /// the gaps they leave are filled like a cut, the neighbours where they land make room.
    pub fn move_by(&mut self, sel: &BTreeSet<usize>, delta: i64) -> BTreeSet<usize> {
        let moved: Vec<(u32, u32, String)> = sel.iter().filter_map(|&i| self.chords.get(i).cloned()).collect();
        if moved.is_empty() || delta == 0 {
            return sel.clone();
        }
        let min = moved.iter().map(|c| c.0 as i64).min().unwrap_or(0);
        let delta = delta.max(-min);
        let groups: Vec<(u32, u32)> = self.groups.iter().copied().filter(|&(a, l)| moved.iter().any(|c| c.0 == a) && moved.iter().any(|c| c.0 + c.1 == a + l)).collect();
        let loops: Vec<(u32, u32)> = self.loops.iter().copied().filter(|l| moved.iter().any(|c| c.0 == l.0)).collect();
        self.groups.retain(|g| !groups.contains(g));
        self.loops.retain(|l| !loops.contains(l));
        self.cut(sel);
        for (s, l, n) in &moved {
            let ns = (*s as i64 + delta) as u32;
            self.carve(ns, ns + l);
            self.chords.push((ns, *l, n.clone()));
        }
        for (a, l) in groups {
            self.groups.push(((a as i64 + delta) as u32, l));
        }
        for (a, e) in loops {
            self.loops.push(((a as i64 + delta) as u32, (e as i64 + delta) as u32));
        }
        self.norm();
        moved.iter().filter_map(|c| self.index_at((c.0 as i64 + delta) as u32)).collect()
    }

    /// Moves chord `i`'s end: lengthening overwrites what follows; shortening lets an adjoining
    /// chord follow the end (the Junction).
    pub fn resize_end(&mut self, i: usize, end: u32) {
        let Some((s, l, _)) = self.chords.get(i).cloned() else { return };
        let end = end.max(s + 1);
        let old = s + l;
        if end > old {
            let name = self.chords[i].2.clone();
            self.carve(old, end);
            if let Some(j) = self.index_at(s) {
                self.chords[j].1 = end - s;
            } else {
                self.chords.push((s, end - s, name));
            }
            for g in &mut self.groups {
                if s >= g.0 && s < g.0 + g.1 && g.0 + g.1 < end {
                    g.1 = end - g.0;
                }
            }
        } else if end < old {
            self.chords[i].1 = end - s;
            if let Some(j) = self.index_at(old) {
                let (_, nl, _) = self.chords[j];
                self.chords[j].0 = end;
                self.chords[j].1 = nl + (old - end);
            }
        }
        self.norm();
    }

    /// Moves chord `i`'s start: lengthening to the left keeps the chord and adds another instance
    /// of it before; shortening lets the chord before follow.
    pub fn resize_start(&mut self, i: usize, start: u32) {
        let Some((s, l, n)) = self.chords.get(i).cloned() else { return };
        let start = start.min(s + l - 1);
        if start < s {
            self.carve(start, s);
            self.chords.push((start, s - start, n));
        } else if start > s {
            self.chords[i] = (start, l - (start - s), n);
            if let Some(j) = (0..self.chords.len()).find(|&j| self.chords[j].0 + self.chords[j].1 == s) {
                self.chords[j].1 += start - s;
            }
        }
        self.norm();
    }

    pub fn transpose(&mut self, sel: &BTreeSet<usize>, by: i32) {
        for &i in sel {
            if let Some(c) = self.chords.get_mut(i) {
                c.2 = crate::keyextra::transpose_chords(&c.2, by);
            }
        }
    }

    /// Halve (factor 2: each chord twice as long) or Double (factor ½) Chord Rhythm on the
    /// selected chords, from the first one's start.
    pub fn rhythm(&mut self, sel: &BTreeSet<usize>, num: u32, den: u32) -> BTreeSet<usize> {
        let picked: Vec<(u32, u32, String)> = sel.iter().filter_map(|&i| self.chords.get(i).cloned()).collect();
        let Some(s0) = picked.first().map(|c| c.0) else { return sel.clone() };
        let scaled: Vec<(u32, u32, String)> = picked.iter().map(|(s, l, n)| (s0 + (s - s0) * num / den, (l * num / den).max(1), n.clone())).collect();
        let old_end = picked.iter().map(|c| c.0 + c.1).max().unwrap_or(s0);
        let new_end = scaled.iter().map(|c| c.0 + c.1).max().unwrap_or(s0);
        for g in &mut self.groups {
            if g.0 == s0 && g.0 + g.1 == old_end {
                g.1 = new_end - s0;
            }
        }
        let mut k = 0;
        self.chords.retain(|_| {
            k += 1;
            !sel.contains(&(k - 1))
        });
        // (the groups and loops kept through the carve)
        let groups = std::mem::take(&mut self.groups);
        let loops = std::mem::take(&mut self.loops);
        self.carve(s0, new_end);
        self.chords.extend(scaled.iter().cloned());
        self.groups = groups;
        self.loops = loops;
        self.norm();
        scaled.iter().filter_map(|c| self.index_at(c.0)).collect()
    }

    /// Group Chords: one group from the first selected chord to the last.
    pub fn group(&mut self, sel: &BTreeSet<usize>) -> Option<usize> {
        let a = self.chords.get(*sel.first()?)?.0;
        let last = self.chords.get(*sel.last()?)?;
        let e = last.0 + last.1;
        self.groups.retain(|&(s, l)| s + l <= a || s >= e);
        self.groups.push((a, e - a));
        self.norm();
        self.groups.iter().position(|g| g.0 == a)
    }

    /// Ungroup Chords: the groups of the selected chords.
    pub fn ungroup(&mut self, sel: &BTreeSet<usize>) {
        let gs: BTreeSet<usize> = sel.iter().filter_map(|&i| self.group_of(i)).collect();
        let mut k = 0;
        self.groups.retain(|_| {
            k += 1;
            !gs.contains(&(k - 1))
        });
    }

    /// The looped unit of chord `i`: its group's span, or the chord's.
    pub fn unit(&self, i: usize) -> Option<(u32, u32)> {
        if let Some(g) = self.group_of(i) {
            let (a, l) = self.groups[g];
            return Some((a, a + l));
        }
        self.chords.get(i).map(|c| (c.0, c.0 + c.1))
    }

    /// Loop on/off (L): a loop runs to the next chord after the unit, or to `end`.
    pub fn toggle_loop(&mut self, i: usize, end: u32) {
        let Some((a, e)) = self.unit(i) else { return };
        if self.loops.iter().any(|l| l.0 == a) {
            self.loops.retain(|l| l.0 != a);
            return;
        }
        let to = self.chords.iter().map(|c| c.0).filter(|&s| s >= e).min().unwrap_or(end.max(e));
        if to > e {
            self.loops.push((a, to));
        }
    }

    /// Sets where a loop of chord `i`'s unit ends (the Loop tool); at the unit's end: no loop.
    pub fn set_loop(&mut self, i: usize, to: u32) {
        let Some((a, e)) = self.unit(i) else { return };
        self.loops.retain(|l| l.0 != a);
        let limit = self.chords.iter().map(|c| c.0).filter(|&s| s >= e).min().unwrap_or(u32::MAX);
        let to = to.min(limit);
        if to > e {
            self.loops.push((a, to));
        }
    }

    /// The repetitions of the loops: (start, length, name) of each looped copy.
    pub fn repeats(&self) -> Vec<(u32, u32, String)> {
        let mut out = Vec::new();
        for &(a, to) in &self.loops {
            let Some(i) = self.index_at(a) else { continue };
            let Some((_, e)) = self.unit(i) else { continue };
            let span = e - a;
            if span == 0 {
                continue;
            }
            let unit: Vec<_> = self.chords.iter().filter(|c| c.0 >= a && c.0 < e).cloned().collect();
            let mut off = span;
            while a + off < to {
                for (s, l, n) in &unit {
                    let ns = s + off;
                    if ns >= to {
                        break;
                    }
                    out.push((ns, (*l).min(to - ns), n.clone()));
                }
                off += span;
            }
        }
        out
    }

    /// The chords as they play: with the loops' repetitions.
    pub fn expanded(&self) -> Vec<(u32, u32, String)> {
        let mut v = self.chords.clone();
        v.extend(self.repeats());
        v.sort_by_key(|c| c.0);
        v
    }

    /// One chord per bar (the chord sounding at each downbeat) up to the last chord's end.
    pub fn per_bar(&self, bar: u32) -> String {
        let all = self.expanded();
        let end = all.iter().map(|c| c.0 + c.1).max().unwrap_or(0);
        let mut out = Vec::new();
        let mut last = String::new();
        let mut t = 0;
        while t < end {
            if let Some(c) = all.iter().rev().find(|c| c.0 <= t && t < c.0 + c.1).or_else(|| all.iter().rev().find(|c| c.0 <= t)).or(all.first()) {
                last = c.2.clone();
            }
            out.push(last.clone());
            t += bar.max(1);
        }
        out.join(" ")
    }

    /// Replaces the chord at `pos` (or the empty place there) with a progression in the key,
    /// one chord per bar, as a chord group. The new chords' indices.
    pub fn progression(&mut self, pos: u32, prog: Progression, key: u8, minor: bool, bar: u32) -> BTreeSet<usize> {
        let tonic = if minor { (key + 3) % 12 } else { key };
        let n = prog.degrees().len() as u32;
        let chords = prog.chords(tonic, pos, bar, n);
        let end = pos + n * bar;
        self.carve(pos, end);
        for c in &chords {
            self.chords.push((c.pos, bar, c.chord.name()));
        }
        self.groups.retain(|&(s, l)| s + l <= pos || s >= end);
        self.groups.push((pos, end - pos));
        self.norm();
        chords.iter().filter_map(|c| self.index_at(c.pos)).collect()
    }
}

/// The key that best fits the chords (each weighted by its length): its name as
/// `Project::key` ("G major", "E minor").
pub fn analyze_key(chords: &[(u32, u32, String)]) -> Option<String> {
    let parsed: Vec<(u32, Chord)> = chords.iter().filter_map(|c| Some((c.1, Chord::parse(&c.2)?))).collect();
    if parsed.is_empty() {
        return None;
    }
    const MAJ: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
    let mut best = (f32::MIN, 0u8, false);
    for minor in [false, true] {
        for k in 0..12u8 {
            // a minor key: its relative major's notes plus the raised 7th (harmonic minor's V)
            let rel = if minor { (k + 3) % 12 } else { k };
            let mut scale: Vec<u8> = MAJ.iter().map(|s| (rel + s) % 12).collect();
            if minor {
                scale.push((k + 11) % 12);
            }
            let mut score = 0.0f32;
            for (len, c) in &parsed {
                let w = *len as f32;
                let pcs = c.pitch_classes();
                let inside = pcs.iter().filter(|p| scale.contains(p)).count() as f32;
                score += w * (inside - 2.0 * (pcs.len() as f32 - inside));
                // the tonic chord: start and end weigh most
                if c.root == k && c.is_minor() == minor {
                    score += w * 1.5;
                }
            }
            if let Some((len, c)) = parsed.first() {
                if c.root == k && c.is_minor() == minor {
                    score += *len as f32;
                }
            }
            if let Some((len, c)) = parsed.last() {
                if c.root == k && c.is_minor() == minor {
                    score += 2.0 * *len as f32;
                }
            }
            if score > best.0 {
                best = (score, k, minor);
            }
        }
    }
    Some(format!("{} {}", NAMES[best.1 as usize], if best.2 { "minor" } else { "major" }))
}

/// The tonic chord of a key name ("A minor" → "Am").
pub fn tonic_chord(key: &str) -> String {
    let pc = lc::pitch_class(key.trim()).map_or(0, |x| x.0);
    let low = key.to_ascii_lowercase();
    let minor = low.contains("min") || (low.len() > 1 && low.trim_end().ends_with('m'));
    format!("{}{}", NAMES[pc as usize], if minor { "m" } else { "" })
}

// ---------------------------------------------------------------- app state

#[derive(Clone, Debug)]
enum Drag {
    Move { grab: u32, from: u32 },
    Start { i: usize },
    End { i: usize },
    Loop { i: usize },
}

/// The Edit Chord window.
#[derive(Clone, Debug, Default)]
pub struct EditDlg {
    pub at: u32,
    pub text: String,
    pub select_all: bool,
    /// Made by Tab: when its chord stays the same as the one before, that one is lengthened.
    pub by_tab: bool,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub sel: BTreeSet<usize>,
    /// The last click was on the Chord track (its keys and Edit commands apply).
    pub focus: bool,
    pub clip: Vec<(u32, u32, String)>,
    drag: Option<Drag>,
    pub edit: Option<EditDlg>,
    /// The chord just added (Tab adds the next one).
    pub just_added: Option<u32>,
    /// `App::chord_text` as last derived from the chords (text set elsewhere is taken over).
    pub seen: String,
    menu_tick: u32,
}

fn ct(app: &App) -> Ct {
    app.project.as_ref().map(Ct::load).unwrap_or_default()
}

/// One undoable edit of the chords; the players follow.
pub fn edit<R>(app: &mut App, what: &str, f: impl FnOnce(&mut Ct) -> R) -> Option<R> {
    app.project.as_ref()?;
    app.checkpoint(what);
    let mut c = ct(app);
    let r = f(&mut c);
    if let Some(p) = app.project.as_mut() {
        c.store(p);
    }
    sync(app);
    crate::players::regenerate_all(app);
    Some(r)
}

/// Keeps `App::chord_text` (one chord per bar, read by the players' other paths) in step with
/// the chords; text set elsewhere (Session Player Editor, older projects) becomes the chords.
pub fn sync(app: &mut App) {
    let bar = app.bar_ticks();
    let Some(p) = app.project.as_mut() else { return };
    if app.chord_text != app.ct.seen {
        let chords: Vec<(u32, u32, String)> = app.chord_text.split_whitespace().filter(|w| Chord::parse(w).is_some()).enumerate().map(|(i, n)| (i as u32 * bar, bar, n.to_string())).collect();
        p.global.chords = chords;
        p.global.chord_groups.clear();
        p.global.chord_loops.clear();
    } else {
        app.chord_text = Ct::load(p).per_bar(bar);
    }
    app.ct.seen = app.chord_text.clone();
    let n = p.global.chords.len();
    app.ct.sel.retain(|&i| i < n);
}

/// The chords the players follow, when the Chord track has its own (None: use `chord_text`).
pub fn playing(app: &App) -> Option<Vec<lc::ChordAt>> {
    if app.chord_text != app.ct.seen {
        return None;
    }
    let v: Vec<lc::ChordAt> = ct(app).expanded().iter().filter_map(|c| Some(lc::ChordAt { pos: c.0, chord: Chord::parse(&c.2)? })).collect();
    (!v.is_empty()).then_some(v)
}

fn key_name_at(app: &App, tick: u32) -> String {
    let bar = tick / app.bar_ticks().max(1);
    app.project.as_ref().map_or("C major".into(), |p| lg_core::timeline::key_at(p, bar))
}

/// Create Chord: the key's tonic at the downbeat of `tick`'s bar, one bar long (or to the next
/// chord); it is selected.
pub fn create_at(app: &mut App, tick: u32) {
    let bar = app.bar_ticks();
    let at = tick / bar * bar;
    let name = tonic_chord(&key_name_at(app, at));
    if let Some(i) = edit(app, "Create Chord", |c| {
        let len = c.default_len(at, bar);
        c.insert(at, len, &name)
    }) {
        app.ct.sel = [i].into();
        app.ct.focus = true;
        app.ct.just_added = Some(at);
    }
}

/// Tab: the next chord a bar on, the same as this one, with Edit Chord open on it.
fn tab_next(app: &mut App, from: u32) {
    let c = ct(app);
    let bar = app.bar_ticks();
    let Some(i) = c.at(from).or_else(|| c.index_at(from)) else { return };
    let (s, l, n) = c.chords[i].clone();
    let at = (s + l).max((s / bar + 1) * bar);
    if let Some(j) = c.index_at(at) {
        // a chord is there already: edit it
        app.ct.sel = [j].into();
        app.ct.edit = Some(EditDlg { at, text: c.chords[j].2.clone(), select_all: true, by_tab: false });
        return;
    }
    let len = c.default_len(at, bar);
    if let Some(j) = edit(app, "Create Chord", |c| c.insert(at, len, &n)) {
        app.ct.sel = [j].into();
        app.ct.just_added = Some(at);
        app.ct.edit = Some(EditDlg { at, text: n, select_all: true, by_tab: true });
    }
}

fn select_click(app: &mut App, i: usize, shift: bool) {
    let c = ct(app);
    app.ct.focus = true;
    app.selected_region = None;
    let unit: Vec<usize> = match c.group_of(i) {
        Some(g) => c.in_group(g),
        None => vec![i],
    };
    // (a chord picked inside its group stays picked, so a double-click on it opens Edit Chord)
    if !shift && app.ct.sel.len() == 1 && app.ct.sel.contains(&i) {
        return;
    }
    if shift {
        if unit.iter().all(|k| app.ct.sel.contains(k)) {
            for k in unit {
                app.ct.sel.remove(&k);
            }
        } else {
            app.ct.sel.extend(unit);
        }
    } else if !unit.iter().all(|k| app.ct.sel.contains(k)) || app.ct.sel.len() != unit.len() {
        app.ct.sel = unit.into_iter().collect();
    }
}

fn in_cycle(app: &App) -> (u32, u32) {
    app.cycle
}

/// The Chord track's commands (key commands, the Edit menu while it has focus, its menu).
pub fn command(app: &mut App, name: &str) -> bool {
    let focus = app.ct.focus && app.project.is_some();
    let c = ct(app);
    let sel = app.ct.sel.clone();
    let any = !sel.is_empty();
    match name {
        "Create Chord" => create_at(app, app.position() as u32),
        "Edit Chord…" => {
            let Some(&i) = sel.first() else { return false };
            open_edit(app, c.chords[i].0);
        }
        "Cut Chords" | "Cut" if (name == "Cut Chords" || focus) && any => {
            app.ct.clip = clip_of(&c, &sel);
            edit(app, "Cut Chords", |c| c.cut(&sel));
            app.ct.sel.clear();
        }
        "Copy Chords" | "Copy" if (name == "Copy Chords" || focus) && any => app.ct.clip = clip_of(&c, &sel),
        "Paste Chords" | "Paste" if (name == "Paste Chords" || focus) && !app.ct.clip.is_empty() => paste(app, app.position() as u32),
        "Delete Chords" | "Delete" if (name == "Delete Chords" || focus) && any => {
            edit(app, "Delete Chords", |c| c.delete(&sel));
            app.ct.sel.clear();
        }
        "Cut All Chords" | "Copy All Chords" | "Delete All Chords" => {
            if c.chords.is_empty() {
                return false;
            }
            let all: BTreeSet<usize> = (0..c.chords.len()).collect();
            if !name.starts_with("Delete") {
                app.ct.clip = clip_of(&c, &all);
            }
            if !name.starts_with("Copy") {
                edit(app, name, |c| *c = Ct::default());
                app.ct.sel.clear();
            }
        }
        "Group Chords" if any => {
            edit(app, "Group Chords", |c| c.group(&sel));
        }
        "Ungroup Chords" if any => {
            edit(app, "Ungroup Chords", |c| c.ungroup(&sel));
        }
        "Select All Chords" => {
            app.ct.sel = (0..c.chords.len()).collect();
            app.ct.focus = true;
        }
        "Select All Chords In Cycle Range" => {
            let (a, b) = in_cycle(app);
            app.ct.sel = (0..c.chords.len()).filter(|&i| c.chords[i].0 >= a && c.chords[i].0 < b).collect();
            app.ct.focus = true;
        }
        "Select All Chord Groups" | "Select All Chord Groups In Cycle Range" => {
            let (a, b) = if name.ends_with("Range") { in_cycle(app) } else { (0, u32::MAX) };
            app.ct.sel = (0..c.groups.len()).filter(|&g| c.groups[g].0 >= a && c.groups[g].0 < b).flat_map(|g| c.in_group(g)).collect();
            app.ct.focus = true;
        }
        "Halve Chord Rhythm" | "Double Chord Rhythm" if any => {
            let (n, d) = if name.starts_with("Halve") { (2, 1) } else { (1, 2) };
            if let Some(s) = edit(app, name, |c| c.rhythm(&sel, n, d)) {
                app.ct.sel = s;
            }
        }
        "Transpose Chords +1 Semitone" | "Transpose Chords -1 Semitone" if !c.chords.is_empty() => {
            let by = if name.contains('+') { 1 } else { -1 };
            let s = if any { sel } else { (0..c.chords.len()).collect() };
            edit(app, "Transpose Chords", |c| c.transpose(&s, by));
        }
        "Global Chords: Analyze Key Signature" | "Analyze Key Signature" if any => return analyze(app),
        "Loop Regions/Cells/Folders On/Off" | "Loop" if focus && any => {
            let end = crate::projend::end(app);
            let i = *sel.first().unwrap_or(&0);
            edit(app, "Loop Chords", |c| c.toggle_loop(i, end));
        }
        _ => return false,
    }
    true
}

fn clip_of(c: &Ct, sel: &BTreeSet<usize>) -> Vec<(u32, u32, String)> {
    let s0 = sel.iter().filter_map(|&i| c.chords.get(i)).map(|c| c.0).min().unwrap_or(0);
    sel.iter().filter_map(|&i| c.chords.get(i)).map(|(s, l, n)| (s - s0, *l, n.clone())).collect()
}

fn paste(app: &mut App, at: u32) {
    let clip = app.ct.clip.clone();
    if let Some(s) = edit(app, "Paste Chords", |c| {
        for (o, l, n) in &clip {
            c.insert(at + o, *l, n);
        }
        clip.iter().filter_map(|x| c.index_at(at + x.0)).collect::<BTreeSet<_>>()
    }) {
        app.ct.sel = s;
        app.ct.focus = true;
    }
}

/// Analyze Key Signature on contiguous selected chords: no key change before the last one: the
/// project key changes; otherwise a key change at the first selected chord.
fn analyze(app: &mut App) -> bool {
    let c = ct(app);
    let picked: Vec<_> = app.ct.sel.iter().filter_map(|&i| c.chords.get(i).cloned()).collect();
    if picked.windows(2).any(|w| w[0].0 + w[0].1 != w[1].0) {
        app.tell("Analyze Key Signature needs contiguous chords");
        return true;
    }
    let Some(key) = analyze_key(&picked) else { return false };
    let bar_t = app.bar_ticks().max(1);
    let first_bar = picked[0].0 / bar_t;
    let last_bar = picked[picked.len() - 1].0 / bar_t;
    app.checkpoint("Analyze Key Signature");
    if let Some(p) = app.project.as_mut() {
        let change_before = p.global.keys.iter().any(|k| k.0 > 0 && k.0 <= last_bar);
        let bar = if change_before { first_bar } else { 0 };
        lg_core::timeline::set_key(p, bar, &key);
    }
    app.after_edit();
    app.tell(format!("Key Signature: {key}"));
    true
}

fn open_edit(app: &mut App, at: u32) {
    let c = ct(app);
    let Some(i) = c.index_at(at) else { return };
    app.ct.edit = Some(EditDlg { at, text: c.chords[i].2.clone(), select_all: true, by_tab: false });
}

/// Plays a chord on the selected (or first) instrument track.
fn preview_chord(app: &mut App, ch: Chord) {
    let Some(track) = app.selected_track.or_else(|| app.project.as_ref()?.tracks.iter().position(|t| t.kind == lg_core::TrackKind::Instrument)) else { return };
    let mut pitches: Vec<u8> = ch.intervals().iter().map(|i| 60 + ch.root + i).collect();
    if let Some(b) = ch.bass {
        pitches.push(48 + b);
    }
    let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
    if e.playing {
        return;
    }
    let pos = e.pos as u32;
    if let Some(t) = e.tracks.get_mut(track) {
        for &p in &pitches {
            t.notes.push((pos + 1, pos + 960, p, 99));
        }
        t.notes.sort_by_key(|n| n.0);
    }
    e.playing = true;
    drop(e);
    let engine = app.engine.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(700));
        let mut e = engine.lock().unwrap_or_else(|e| e.into_inner());
        e.playing = false;
        e.all_off();
        e.pos = pos as f64;
        if let Some(t) = e.tracks.get_mut(track) {
            t.notes.retain(|n| !(n.0 == pos + 1 && n.3 == 99));
        }
    });
}

// ---------------------------------------------------------------- drawing and gestures

const EDGE: f32 = 5.0;

/// Draws the Chord track lane and handles its gestures; `add` is the header's (+) button.
#[allow(clippy::too_many_arguments)]
pub fn lane(app: &mut App, ui: &mut egui::Ui, row: Rect, lane: Rect, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32, add: bool) {
    sync(app);
    if add {
        create_at(app, app.position() as u32);
    }
    let (shift, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
    if ui.input(|i| i.pointer.any_pressed()) && !ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| lane.contains(h)) {
        app.ct.focus = false;
        app.ct.just_added = None;
    }
    let lp = ui.painter_at(lane);
    let bar_t = app.bar_ticks() as f32;
    let ppt = (x_of(bar_t) - x_of(0.0)) / bar_t;
    let snap = |app: &App, t: f32| crate::arranging::snap_tick(app, t, ppt).max(0.0) as u32;
    let c = ct(app);
    // the background: Pencil adds a chord, a click deselects, the shortcut menu
    let bg = ui.interact(lane, ui.scope_id().with("chord-lane"), Sense::click());
    if bg.clicked() {
        let t = bg.interact_pointer_pos().map_or(0.0, |p| tick_of(p.x));
        if app.tool == crate::edit::Tool::Pencil {
            let at = snap(app, t);
            let name = tonic_chord(&key_name_at(app, at));
            let bar = app.bar_ticks();
            if let Some(i) = edit(app, "Create Chord", |c| {
                let len = c.default_len(at, bar);
                c.insert(at, len, &name)
            }) {
                app.ct.sel = [i].into();
                app.ct.just_added = Some(at);
            }
        } else {
            app.ct.sel.clear();
        }
        app.ct.focus = true;
    }
    if bg.secondary_clicked() {
        app.ct.menu_tick = bg.interact_pointer_pos().map_or(0, |p| tick_of(p.x).max(0.0) as u32);
        app.ct.focus = true;
    }
    if c.chords.is_empty() {
        // Logic: the key's chord, dimmed, until chords are added
        let key = key_name_at(app, 0);
        lp.text(pos2(x_of(0.0) + 6.0, row.center().y), Align2::LEFT_CENTER, tonic_chord(&key), look::regular(11.0), Color32::from_gray(0x70));
    }
    let y = row.y_range();
    let body_r = |s: u32, l: u32| Rect::from_min_max(pos2(x_of(s as f32), y.min + 1.0), pos2(x_of((s + l) as f32), y.max - 1.0));
    // the groups: one frame around their chords
    for &(a, l) in &c.groups {
        let r = body_r(a, l);
        lp.rect_filled(r, 2.0, Color32::from_gray(0x2c));
    }
    // loop repetitions: lighter copies with notches at each full repetition
    for (s, l, n) in c.repeats() {
        let r = body_r(s, l);
        lp.rect_filled(r.shrink2(vec2(0.5, 0.0)), 0.0, Color32::from_gray(0x2a));
        lp.with_clip_rect(r).text(pos2(r.min.x + 5.0, r.center().y), Align2::LEFT_CENTER, &n, look::regular(11.0), Color32::from_gray(0x80));
    }
    for &(a, to) in &c.loops {
        let Some(i) = c.index_at(a) else { continue };
        let Some((_, e)) = c.unit(i) else { continue };
        let mut t = e;
        while t <= to && e > a {
            let x = x_of(t as f32);
            lp.vline(x, y.min + 1.0..=y.min + 5.0, Stroke::new(1.0, Color32::from_gray(0xa0)));
            lp.vline(x, y.max - 5.0..=y.max - 1.0, Stroke::new(1.0, Color32::from_gray(0xa0)));
            t += e - a;
        }
    }
    let mut hovered_cursor = None;
    for (i, (s, l, n)) in c.chords.iter().enumerate() {
        let r = body_r(*s, *l);
        if r.max.x < lane.min.x || r.min.x > lane.max.x {
            continue;
        }
        let selected = app.ct.sel.contains(&i);
        let fill = if selected { Color32::from_gray(0x5a) } else { Color32::from_gray(0x3a) };
        lp.rect_filled(r.shrink2(vec2(0.5, 0.0)), 0.0, fill);
        lp.vline(r.min.x, y.min + 1.0..=y.max - 1.0, Stroke::new(1.0, Color32::from_gray(0x1e)));
        lp.with_clip_rect(r).text(pos2(r.min.x + 5.0, r.center().y), Align2::LEFT_CENTER, n, look::semibold(11.0), if selected { Color32::WHITE } else { Color32::from_gray(0xdc) });
        let body = ui.interact(r, ui.scope_id().with(("chord", i)), Sense::click_and_drag());
        let mid = r.center().y;
        let low_l = Rect::from_min_max(pos2(r.min.x - 1.0, mid), pos2(r.min.x + EDGE, r.max.y));
        let low_r = Rect::from_min_max(pos2(r.max.x - EDGE, mid), pos2(r.max.x + 1.0, r.max.y));
        let up_r = Rect::from_min_max(pos2(r.max.x - EDGE, r.min.y), pos2(r.max.x + 1.0, mid));
        let ll = ui.interact(low_l, ui.scope_id().with(("chord-l", i)), Sense::drag());
        let lr = ui.interact(low_r, ui.scope_id().with(("chord-r", i)), Sense::drag());
        let ur = ui.interact(up_r, ui.scope_id().with(("chord-loop", i)), Sense::drag());
        let in_group = c.group_of(i).is_some();
        // inside a group, a selected chord's edges are the Junction tool
        let junction = in_group && selected && app.ct.sel.len() == 1;
        if ur.hovered() && (!in_group || c.unit(i).is_some_and(|u| u.1 == s + l)) {
            hovered_cursor = Some(CursorIcon::Alias);
        } else if (ll.hovered() || lr.hovered()) && (!in_group || junction) {
            hovered_cursor = Some(if junction { CursorIcon::ResizeColumn } else { CursorIcon::ResizeHorizontal });
        }
        if ur.drag_started() && (!in_group || c.unit(i).is_some_and(|u| u.1 == s + l)) {
            app.checkpoint("Loop Chords");
            app.ct.drag = Some(Drag::Loop { i });
        } else if lr.drag_started() && (!in_group || junction) {
            app.checkpoint("Resize Chord");
            app.ct.drag = Some(Drag::End { i });
        } else if ll.drag_started() && (!in_group || junction) {
            app.checkpoint("Resize Chord");
            app.ct.drag = Some(Drag::Start { i });
        } else if body.drag_started() {
            if !selected {
                select_click(app, i, false);
            }
            let grab = body.interact_pointer_pos().map_or(*s, |p| tick_of(p.x).max(0.0) as u32);
            app.ct.drag = Some(Drag::Move { grab, from: *s });
        }
        if body.clicked() {
            if alt {
                app.locate(*s as f64);
            } else {
                select_click(app, i, shift);
            }
        }
        if body.double_clicked() {
            if in_group && !(selected && app.ct.sel.len() == 1) {
                // the chord inside its group (Shift: added to the selection)
                if shift {
                    app.ct.sel.insert(i);
                } else {
                    app.ct.sel = [i].into();
                }
            } else {
                app.ct.sel = [i].into();
                open_edit(app, *s);
            }
            app.ct.focus = true;
        }
        if body.secondary_clicked() {
            if !selected {
                select_click(app, i, false);
            }
            app.ct.menu_tick = *s;
        }
        body.context_menu(|ui| chord_menu(app, ui, true));
    }
    bg.context_menu(|ui| chord_menu(app, ui, false));
    // the groups' frames over their chords
    for &(a, l) in &c.groups {
        let r = body_r(a, l);
        lp.rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_gray(0x9a)), egui::StrokeKind::Inside);
    }
    if let Some(cur) = hovered_cursor {
        ui.ctx().set_cursor_icon(cur);
    }
    drag(app, ui, &lp, row, x_of, tick_of, ppt);
    // Tab after adding a chord: the next one, with Edit Chord
    if app.ct.focus && app.ct.edit.is_none() {
        if let Some(at) = app.ct.just_added {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
                tab_next(app, at);
            }
        }
    }
    edit_window(app, ui.ctx());
}

fn drag(app: &mut App, ui: &mut egui::Ui, lp: &egui::Painter, row: Rect, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32, ppt: f32) {
    let Some(d) = app.ct.drag.clone() else { return };
    let Some(pp) = ui.input(|i| i.pointer.latest_pos()) else { return };
    let down = ui.input(|i| i.pointer.primary_down());
    let t = crate::arranging::snap_tick(app, tick_of(pp.x), ppt).max(0.0) as u32;
    let c = ct(app);
    let ghost = |a: u32, b: u32| {
        let r = Rect::from_min_max(pos2(x_of(a as f32), row.min.y + 1.0), pos2(x_of(b as f32), row.max.y - 1.0));
        lp.rect_stroke(r, 0.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
    };
    match &d {
        Drag::Move { grab, from } => {
            let raw = tick_of(pp.x).max(0.0) as i64 - *grab as i64 + *from as i64;
            let ns = crate::arranging::snap_tick(app, raw.max(0) as f32, ppt) as i64;
            let delta = ns - *from as i64;
            for &i in &app.ct.sel {
                if let Some(ch) = c.chords.get(i) {
                    let a = (ch.0 as i64 + delta).max(0) as u32;
                    ghost(a, a + ch.1);
                }
            }
            if !down {
                let sel = app.ct.sel.clone();
                if delta != 0 {
                    if let Some(s) = edit(app, "Move Chords", |c| c.move_by(&sel, delta)) {
                        app.ct.sel = s;
                    }
                }
                app.ct.drag = None;
            }
        }
        Drag::Start { i } | Drag::End { i } => {
            let Some(ch) = c.chords.get(*i) else {
                app.ct.drag = None;
                return;
            };
            let start = matches!(d, Drag::Start { .. });
            if start { ghost(t.min(ch.0 + ch.1 - 1), ch.0 + ch.1) } else { ghost(ch.0, t.max(ch.0 + 1)) }
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            if !down {
                let i = *i;
                if let Some(p) = app.project.as_mut() {
                    let mut c = c.clone();
                    if start { c.resize_start(i, t) } else { c.resize_end(i, t) }
                    c.store(p);
                }
                sync(app);
                crate::players::regenerate_all(app);
                app.ct.drag = None;
            }
        }
        Drag::Loop { i } => {
            if let Some((a, _)) = c.unit(*i) {
                ghost(a, t.max(a));
            }
            ui.ctx().set_cursor_icon(CursorIcon::Alias);
            if !down {
                let i = *i;
                if let Some(p) = app.project.as_mut() {
                    let mut c = c.clone();
                    c.set_loop(i, t);
                    c.store(p);
                }
                sync(app);
                crate::players::regenerate_all(app);
                app.ct.drag = None;
            }
        }
    }
}

/// The chord (or Chord track) shortcut menu.
fn chord_menu(app: &mut App, ui: &mut egui::Ui, on_chord: bool) {
    let c = ct(app);
    let any = on_chord && !app.ct.sel.is_empty();
    let group = app.ct.sel.iter().find_map(|&i| c.group_of(i));
    let mut run: Option<&str> = None;
    if !on_chord && ui.button("Create Chord").clicked() {
        let t = app.ct.menu_tick;
        create_at(app, t);
        ui.close();
    }
    if on_chord && ui.button("Edit Chord…").clicked() {
        run = Some("Edit Chord…");
    }
    ui.menu_button("Chord Progressions", |ui| {
        for prog in Progression::ALL.iter().filter(|p| **p != Progression::Default) {
            if ui.button(prog.name()).clicked() {
                let at = if on_chord { app.ct.sel.first().and_then(|&i| c.chords.get(i)).map_or(app.ct.menu_tick, |ch| ch.0) } else { app.ct.menu_tick / app.bar_ticks().max(1) * app.bar_ticks() };
                let key = key_name_at(app, at);
                let pc = lc::pitch_class(key.trim()).map_or(0, |x| x.0);
                let minor = key.to_ascii_lowercase().contains("min");
                let bar = app.bar_ticks();
                let p = *prog;
                if let Some(s) = edit(app, "Chord Progression", |c| c.progression(at, p, pc, minor, bar)) {
                    app.ct.sel = s;
                }
                ui.close();
            }
        }
    });
    ui.separator();
    if ui.add_enabled(any, egui::Button::new("Cut")).clicked() {
        run = Some("Cut Chords");
    }
    if ui.add_enabled(any, egui::Button::new("Copy")).clicked() {
        run = Some("Copy Chords");
    }
    if ui.add_enabled(!app.ct.clip.is_empty(), egui::Button::new("Paste")).clicked() {
        let at = app.ct.menu_tick / app.bar_ticks().max(1) * app.bar_ticks();
        paste(app, if on_chord { app.ct.menu_tick } else { at });
        ui.close();
    }
    if ui.add_enabled(any, egui::Button::new("Delete")).clicked() {
        run = Some("Delete Chords");
    }
    ui.separator();
    if on_chord {
        if group.is_some() {
            if ui.button("Ungroup Chords").clicked() {
                run = Some("Ungroup Chords");
            }
            if ui.button("Halve Chord Rhythm").clicked() {
                run = Some("Halve Chord Rhythm");
            }
            if ui.button("Double Chord Rhythm").clicked() {
                run = Some("Double Chord Rhythm");
            }
        } else if ui.add_enabled(app.ct.sel.len() > 1, egui::Button::new("Group Chords")).clicked() {
            run = Some("Group Chords");
        }
        if ui.button("Transpose Chords +1 Semitone").clicked() {
            run = Some("Transpose Chords +1 Semitone");
        }
        if ui.button("Transpose Chords -1 Semitone").clicked() {
            run = Some("Transpose Chords -1 Semitone");
        }
        if ui.button("Analyze Key Signature").clicked() {
            run = Some("Analyze Key Signature");
        }
        ui.separator();
    }
    if ui.button("Select All Chords In Cycle Range").clicked() {
        run = Some("Select All Chords In Cycle Range");
    }
    if let Some(r) = run {
        command(app, r);
        ui.close();
    }
}

/// The type buttons: (label, the intervals of that type).
const TYPES: [(&str, u16); 6] = [("maj", lc::MAJOR), ("min", lc::MINOR), ("aug", lc::AUG), ("dim", lc::DIM), ("sus4", lc::SUS4), ("sus2", lc::SUS2)];
/// The extension buttons: (label, bits added, bit it replaces).
const EXTS: [(&str, u16, u16); 9] = [("6", 1 << 9, 0), ("7", 1 << 10, 0), ("maj7", 1 << 11, 0), ("9", 1 << 2, 0), ("b9", 1 << 1, 0), ("#9", 1 << 3, 0), ("11", 1 << 5, 0), ("13", 1 << 9 | 1 << 10 | 1 << 2, 0), ("b5", 1 << 6, 1 << 7)];

/// The bits a chord type sets (3rd, 5th; the sus types' 2nd and 4th too).
fn type_bits(sus: bool) -> u16 {
    1 << 3 | 1 << 4 | 1 << 6 | 1 << 7 | 1 << 8 | if sus { 1 << 2 | 1 << 5 } else { 0 }
}

/// A chord's type (one of `TYPES`' masks).
fn triad_of(mask: u16) -> u16 {
    let b = |n: u16| mask & (1 << n) != 0;
    if b(4) {
        if b(8) && !b(7) { lc::AUG } else { lc::MAJOR }
    } else if b(3) && !b(4) {
        if b(6) && !b(7) && !b(10) || b(6) && b(9) && !b(7) { lc::DIM } else { lc::MINOR }
    } else if b(5) {
        lc::SUS4
    } else if b(2) {
        lc::SUS2
    } else {
        lc::MAJOR
    }
}

/// The chord as type `m`.
fn with_type(ch: Chord, m: u16) -> Chord {
    let cur = triad_of(ch.mask);
    let sus = [lc::SUS2, lc::SUS4].contains(&cur) || [lc::SUS2, lc::SUS4].contains(&m);
    Chord { mask: (ch.mask & !type_bits(sus)) | m, ..ch }
}

/// Whether Studio can name (and the players read) a chord.
fn nameable(c: &Chord) -> bool {
    Chord::parse(&c.name()) == Some(*c)
}

/// Edit Chord: the chord field (typed names), Root Note, the type and extension buttons, Bass
/// Note, Preview; Tab goes to the next chord, Space plays.
fn edit_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut dlg) = app.ct.edit.clone() else { return };
    let c = ct(app);
    let Some(i) = c.index_at(dlg.at) else {
        app.ct.edit = None;
        return;
    };
    let cur = Chord::parse(&c.chords[i].2);
    let mut open = true;
    let mut set: Option<String> = None;
    let mut close = false;
    let mut tab = false;
    let mut preview = false;
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
        app.play_stop();
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
        tab = true;
    }
    egui::Window::new("Edit Chord").id(egui::Id::new("edit-chord")).open(&mut open).collapsible(false).resizable(false).default_width(330.0).show(ctx, |ui| {
        egui::Grid::new("ec-grid").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
            ui.label("Chord:");
            let te = ui.add(egui::TextEdit::singleline(&mut dlg.text).desired_width(140.0).font(look::semibold(13.0)));
            if dlg.select_all {
                // the chord's name is selected, ready to type over
                te.request_focus();
                if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), te.id) {
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(dlg.text.chars().count()))));
                    st.store(ui.ctx(), te.id);
                }
                dlg.select_all = false;
            }
            if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some(ch) = Chord::parse(dlg.text.trim()) {
                    set = Some(ch.name());
                }
            }
            ui.end_row();
            let Some(ch) = cur else {
                ui.end_row();
                return;
            };
            ui.label("Root Note:");
            egui::ComboBox::from_id_salt("ec-root").selected_text(NAMES[ch.root as usize % 12]).show_ui(ui, |ui| {
                for (k, n) in NAMES.iter().enumerate() {
                    if ui.selectable_label(ch.root as usize == k, *n).clicked() {
                        set = Some(Chord { root: k as u8, ..ch }.name());
                    }
                }
            });
            ui.end_row();
            ui.label("Chord Type:");
            ui.horizontal(|ui| {
                let tri = triad_of(ch.mask);
                for (label, m) in TYPES {
                    let next = with_type(ch, m);
                    let lit = tri == m;
                    if ui.add_enabled(lit || nameable(&next), egui::Button::selectable(lit, label)).clicked() && !lit {
                        set = Some(next.name());
                    }
                }
            });
            ui.end_row();
            ui.label("Extensions:");
            ui.horizontal_wrapped(|ui| {
                for (label, add, replaces) in EXTS {
                    let lit = ch.mask & add == add && (replaces == 0 || ch.mask & replaces == 0);
                    let next = if lit { Chord { mask: (ch.mask & !add) | replaces, ..ch } } else { Chord { mask: (ch.mask | add) & !replaces, ..ch } };
                    if ui.add_enabled(nameable(&next), egui::Button::selectable(lit, label)).clicked() {
                        set = Some(next.name());
                    }
                }
            });
            ui.end_row();
            ui.label("Bass Note:");
            let bass = ch.bass.map_or("Root", |b| NAMES[b as usize % 12]);
            egui::ComboBox::from_id_salt("ec-bass").selected_text(bass).show_ui(ui, |ui| {
                if ui.selectable_label(ch.bass.is_none(), "Root").clicked() {
                    set = Some(Chord { bass: None, ..ch }.name());
                }
                for (k, n) in NAMES.iter().enumerate() {
                    if ui.selectable_label(ch.bass == Some(k as u8), *n).clicked() {
                        set = Some(Chord { bass: (k as u8 != ch.root).then_some(k as u8), ..ch }.name());
                    }
                }
            });
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Preview").clicked() {
                preview = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").clicked() {
                    close = true;
                }
            });
        });
    });
    if let Some(n) = set {
        if n != c.chords[i].2 {
            dlg.text = n.clone();
            let at = dlg.at;
            edit(app, "Edit Chord", |c| {
                if let Some(j) = c.index_at(at) {
                    c.chords[j].2 = n;
                }
            });
            dlg.by_tab = false;
            if let Some(ch) = Chord::parse(&dlg.text) {
                preview_chord(app, ch);
            }
        }
    }
    if preview {
        if let Some(ch) = Chord::parse(&dlg.text).or(cur) {
            preview_chord(app, ch);
        }
    }
    app.ct.edit = Some(dlg.clone());
    if tab {
        finish_tab_chord(app, &dlg);
        app.ct.edit = None;
        tab_next(app, dlg.at);
        return;
    }
    if !open || close {
        finish_tab_chord(app, &dlg);
        app.ct.edit = None;
    }
}

/// A Tab-made chord left the same as the one before: that one is lengthened instead.
fn finish_tab_chord(app: &mut App, dlg: &EditDlg) {
    if !dlg.by_tab {
        return;
    }
    let c = ct(app);
    let Some(i) = c.index_at(dlg.at) else { return };
    let Some(prev) = (0..c.chords.len()).find(|&j| c.chords[j].0 + c.chords[j].1 == dlg.at) else { return };
    if c.chords[prev].2 == c.chords[i].2 {
        let len = c.chords[i].1;
        if let Some(p) = app.project.as_mut() {
            let mut c = c.clone();
            c.chords[prev].1 += len;
            c.chords.remove(i);
            c.norm();
            c.store(p);
        }
        sync(app);
        app.ct.sel = [prev].into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ct3() -> Ct {
        Ct { chords: vec![(0, 3840, "C".into()), (3840, 3840, "Am".into()), (7680, 3840, "F".into())], ..Default::default() }
    }

    fn names(c: &Ct) -> Vec<(u32, u32, &str)> {
        c.chords.iter().map(|x| (x.0, x.1, x.2.as_str())).collect()
    }

    #[test]
    fn insert_overwrites_and_splits() {
        let mut c = ct3();
        c.insert(4800, 960, "G");
        assert_eq!(names(&c), [(0, 3840, "C"), (3840, 960, "Am"), (4800, 960, "G"), (5760, 1920, "Am"), (7680, 3840, "F")]);
        assert_eq!(c.default_len(11520, 3840), 3840);
        assert_eq!(c.default_len(1920 + 3840 * 3, 3840), 3840);
        let mut d = ct3();
        assert_eq!(d.default_len(7000, 3840), 680);
        d.insert(0, 7680, "D");
        assert_eq!(names(&d), [(0, 7680, "D"), (7680, 3840, "F")]);
    }

    #[test]
    fn cut_fills_the_gap() {
        let mut c = ct3();
        c.cut(&[1].into());
        assert_eq!(names(&c), [(0, 7680, "C"), (7680, 3840, "F")]);
        let mut c = ct3();
        c.cut(&[0].into());
        assert_eq!(names(&c), [(0, 3840, "Am"), (3840, 3840, "Am"), (7680, 3840, "F")]);
        let mut d = ct3();
        d.delete(&[1].into());
        assert_eq!(names(&d), [(0, 3840, "C"), (7680, 3840, "F")]);
    }

    #[test]
    fn move_keeps_length_and_neighbours_make_room() {
        let mut c = ct3();
        let s = c.move_by(&[2].into(), -1920 - 3840);
        // F lands at 1920: C is trimmed, Am follows it, the gap F left is filled by Am
        assert_eq!(names(&c), [(0, 1920, "C"), (1920, 3840, "F"), (5760, 5760, "Am")]);
        assert_eq!(s, [1].into());
    }

    #[test]
    fn resizing_moves_the_neighbours() {
        let mut c = ct3();
        c.resize_end(0, 1920);
        assert_eq!(names(&c)[..2], [(0, 1920, "C"), (1920, 5760, "Am")]);
        let mut c = ct3();
        c.resize_end(0, 5760);
        assert_eq!(names(&c)[..2], [(0, 5760, "C"), (5760, 1920, "Am")]);
        c.resize_end(0, 7680);
        assert_eq!(names(&c), [(0, 7680, "C"), (7680, 3840, "F")]);
        let mut c = ct3();
        c.resize_start(1, 1920);
        assert_eq!(names(&c)[..3], [(0, 1920, "C"), (1920, 1920, "Am"), (3840, 3840, "Am")]);
        let mut c = ct3();
        c.resize_start(1, 4800);
        assert_eq!(names(&c)[..2], [(0, 4800, "C"), (4800, 2880, "Am")]);
    }

    #[test]
    fn groups_rhythm_loops() {
        let mut c = ct3();
        let g = c.group(&[0, 1].into()).unwrap();
        assert_eq!(c.groups[g], (0, 7680));
        assert_eq!(c.group_of(1), Some(0));
        assert_eq!(c.group_of(2), None);
        assert_eq!(c.in_group(0), [0, 1]);
        let s = c.rhythm(&[0, 1].into(), 1, 2);
        assert_eq!(names(&c), [(0, 1920, "C"), (1920, 1920, "Am"), (7680, 3840, "F")]);
        assert_eq!(c.groups, [(0, 3840)]);
        assert_eq!(s.len(), 2);
        c.toggle_loop(0, 20000);
        assert_eq!(c.loops, [(0, 7680)]);
        assert_eq!(c.repeats().iter().map(|r| (r.0, r.2.as_str())).collect::<Vec<_>>(), [(3840, "C"), (5760, "Am")]);
        assert!(c.per_bar(3840).starts_with("C C F"));
        c.toggle_loop(1, 20000);
        assert!(c.loops.is_empty());
        let mut d = ct3();
        d.rhythm(&[0].into(), 2, 1);
        assert_eq!(names(&d), [(0, 7680, "C"), (7680, 3840, "F")]);
        d.ungroup(&[0].into());
        let mut e = ct3();
        e.group(&[0, 1].into());
        e.ungroup(&[1].into());
        assert!(e.groups.is_empty());
    }

    #[test]
    fn progressions_keys_transpose() {
        let mut c = Ct::default();
        let s = c.progression(3840, Progression::TwoFiveOne, 7, false, 3840);
        assert_eq!(names(&c).iter().map(|x| x.2).collect::<Vec<_>>(), ["Am", "D", "G", "G"]);
        assert_eq!(c.groups, [(3840, 4 * 3840)]);
        assert_eq!(s.len(), 4);
        assert_eq!(analyze_key(&c.chords).as_deref(), Some("G major"));
        let mi: Vec<(u32, u32, String)> = ["Am", "Dm", "E", "Am"].iter().enumerate().map(|(i, n)| (i as u32 * 10, 10, n.to_string())).collect();
        assert_eq!(analyze_key(&mi).as_deref(), Some("A minor"));
        c.transpose(&[0, 1].into(), 1);
        assert_eq!(c.chords[0].2, "Bbm");
        assert_eq!(tonic_chord("A minor"), "Am");
        assert_eq!(tonic_chord("Eb major"), "Eb");
    }

    #[test]
    fn edit_buttons_name_chords() {
        let c = Chord::parse("C").unwrap();
        assert!(nameable(&Chord { mask: c.mask | 1 << 10, ..c }));
        assert_eq!(triad_of(Chord::parse("Am7").unwrap().mask), lc::MINOR);
        assert_eq!(triad_of(Chord::parse("Bdim").unwrap().mask), lc::DIM);
        assert_eq!(triad_of(Chord::parse("C9").unwrap().mask), lc::MAJOR);
    }

    #[test]
    fn app_chords_follow_and_undo() {
        let mut app = crate::tm::test_app();
        app.chord_text = "C G".into();
        sync(&mut app);
        assert_eq!(ct(&app).chords.len(), 2);
        app.ct.sel = [1].into();
        app.ct.focus = true;
        assert!(command(&mut app, "Cut"));
        assert_eq!(ct(&app).chords.len(), 1);
        assert_eq!(app.chord_text, "C C");
        assert!(command(&mut app, "Paste Chords"));
        app.undo();
        sync(&mut app);
        assert_eq!(app.chord_text, "C C");
        assert!(playing(&app).is_some());
        create_at(&mut app, 3840 * 5 + 100);
        assert_eq!(ct(&app).chords.last().map(|c| (c.0, c.2.clone())), Some((3840 * 5, "C".to_string())));
    }
}
