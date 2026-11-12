//! The Step Sequencer inspector (Logic Pro 13's, at the left of the Step Sequencer: View ▸ Show
//! Local Inspector or the toolbar's Inspector button). Pattern, Row and Step buttons at the top
//! choose what it edits; with several rows or steps selected a value that differs shows "*", and
//! changing it keeps their differences (each moves by the same amount).
//!
//! It also holds the pattern, row and step settings the inspector edits that the grid itself
//! doesn't (legato, swing, the pattern's key and scale, MIDI channels, automation mode, step
//! conditions, per-step rate, articulation, the note-repeat velocity ramp / curve / distribution)
//! and the parts of playback that use them (`stepseq::StepPattern::render` calls in here).

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::look;
use crate::stepseq::{Play, Row, StepPattern, MAX_STEPS, RATES};
use crate::widgets;

/// A step's play condition (Logic's Condition pop-up).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Cond {
    #[default]
    Always,
    Recurrence,
    FirstLoop,
    LastLoop,
}

impl Cond {
    pub const ALL: [Cond; 4] = [Cond::Always, Cond::Recurrence, Cond::FirstLoop, Cond::LastLoop];
    pub fn name(self) -> &'static str {
        match self {
            Cond::Always => "Always",
            Cond::Recurrence => "Recurrence",
            Cond::FirstLoop => "First Loop",
            Cond::LastLoop => "Last Loop",
        }
    }
}

/// How automation rows move between steps (Latch holds, Slide glides).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AutoMode {
    #[default]
    Latch,
    Slide,
}

/// Scales of the Scale pop-up: name and the semitones in it.
pub const SCALES: [(&str, &[u8]); 12] = [
    ("Major (Ionian)", &[0, 2, 4, 5, 7, 9, 11]),
    ("Minor (Aeolian)", &[0, 2, 3, 5, 7, 8, 10]),
    ("Dorian", &[0, 2, 3, 5, 7, 9, 10]),
    ("Phrygian", &[0, 1, 3, 5, 7, 8, 10]),
    ("Lydian", &[0, 2, 4, 6, 7, 9, 11]),
    ("Mixolydian", &[0, 2, 4, 5, 7, 9, 10]),
    ("Locrian", &[0, 1, 3, 5, 6, 8, 10]),
    ("Harmonic Minor", &[0, 2, 3, 5, 7, 8, 11]),
    ("Melodic Minor", &[0, 2, 3, 5, 7, 9, 11]),
    ("Major Pentatonic", &[0, 2, 4, 7, 9]),
    ("Minor Pentatonic", &[0, 3, 5, 7, 10]),
    ("Chromatic", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
];
pub const KEYS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// The pattern's own settings.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternOpts {
    /// A 100% gate lasts one tick longer, so it overlaps the next step.
    pub legato: bool,
    /// 0–100: how far every second division is pushed late.
    pub swing: u8,
    /// The swing's note division in ticks (480: 1/8, 240: 1/16).
    pub swing_div: u32,
    /// The pattern's key (root 0–11, index into SCALES); None follows the project's key.
    pub key: Option<(u8, usize)>,
    pub no_transpose: bool,
    pub scale_quantize: bool,
    /// MIDI channel 1–16 for all rows (and new rows).
    pub channel: u8,
    pub auto_mode: AutoMode,
}

impl Default for PatternOpts {
    fn default() -> Self {
        PatternOpts { legato: false, swing: 0, swing_div: 240, key: None, no_transpose: false, scale_quantize: false, channel: 1, auto_mode: AutoMode::Latch }
    }
}

/// A row's own settings (else the pattern's).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RowOpts {
    pub channel: Option<u8>,
    pub auto_mode: Option<AutoMode>,
}

// ---- playback helpers (used by StepPattern::render) ------------------------------------------

/// Whether a step with this condition plays on loop pass `pass` (0-based) of `passes`.
pub fn cond_plays(cond: Cond, iter: u8, cycles: u8, pass: usize, passes: usize) -> bool {
    match cond {
        Cond::Always => true,
        Cond::FirstLoop => pass == 0,
        Cond::LastLoop => pass + 1 == passes.max(1),
        Cond::Recurrence => {
            let c = cycles.max(1) as usize;
            pass % c + 1 == (iter.max(1) as usize).min(c)
        }
    }
}

/// The velocity of repeat `k` of `reps`: a negative ramp fades out, a positive one swells in;
/// the curve bends it (−100…100, 0 straight).
pub fn ramp_velocity(vel: u8, k: u32, reps: u32, ramp: i8, curve: i8) -> u8 {
    if reps <= 1 || ramp == 0 {
        return vel;
    }
    let x = k as f32 / (reps - 1) as f32;
    let x = x.powf(2f32.powf(curve as f32 / 50.0));
    let amt = ramp.unsigned_abs() as f32 / 100.0;
    let f = if ramp < 0 { 1.0 - amt * x } else { 1.0 - amt * (1.0 - x) };
    ((vel as f32 * f).round() as i32).clamp(1, 127) as u8
}

/// Where repeat `k` of `reps` starts, as a fraction of the step: evenly spaced at 0; positive
/// distribution speeds up (accelerando), negative slows down (decelerando).
pub fn repeat_at(k: u32, reps: u32, dist: i8) -> f32 {
    let x = k as f32 / reps.max(1) as f32;
    x.powf(2f32.powf(-(dist as f32) / 50.0))
}

/// Swing: every second `div` is pushed late by up to half a division.
pub fn swing(pos: u32, swing: u8, div: u32) -> u32 {
    if swing == 0 || div == 0 {
        return pos;
    }
    if (pos / div) % 2 == 1 && pos % div == 0 {
        pos + div * swing.min(100) as u32 / 200
    } else {
        pos
    }
}

/// The step's MIDI channel (step, else row, else pattern), 0-based for a Note.
pub fn channel(p: &StepPattern, r: &Row, step_ch: Option<u8>) -> u8 {
    step_ch.or(r.opts.channel).unwrap_or(p.opts.channel).clamp(1, 16) - 1
}

// ---- key and scale ----------------------------------------------------------------------------

/// The project key ("C major", "A minor") as (root, scale index).
pub fn project_key(key: &str) -> (u8, usize) {
    let mut it = key.split_whitespace();
    let root = it.next().unwrap_or("C");
    let minor = it.next().is_some_and(|m| m.starts_with("min"));
    let r = KEYS.iter().position(|k| *k == root).or_else(|| ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"].iter().position(|k| *k == root)).unwrap_or(0);
    (r as u8, minor as usize)
}

pub fn key_label(root: u8, scale: usize) -> String {
    format!("{} - {}", KEYS[root as usize % 12], SCALES.get(scale).map_or("Major (Ionian)", |s| s.0))
}

pub fn in_scale(pitch: u8, root: u8, scale: usize) -> bool {
    let deg = (pitch as i32 - root as i32).rem_euclid(12) as u8;
    SCALES.get(scale).is_none_or(|s| s.1.contains(&deg))
}

/// The nearest pitch in the scale (down first on a tie).
pub fn conform(pitch: u8, root: u8, scale: usize) -> u8 {
    for d in 0..12i32 {
        for p in [pitch as i32 - d, pitch as i32 + d] {
            if (0..=127).contains(&p) && in_scale(p as u8, root, scale) {
                return p as u8;
            }
        }
    }
    pitch
}

/// The pitches the pattern plays (rows' notes with their steps' Note / Octave offsets).
fn played_pitches(p: &StepPattern) -> Vec<u8> {
    let mut out = Vec::new();
    for r in &p.rows {
        out.push(r.pitch);
        for s in r.steps.iter().filter(|s| s.on) {
            out.push((r.pitch as i32 + s.note as i32 + 12 * s.octave as i32).clamp(0, 127) as u8);
        }
    }
    out
}

/// Some row or step note is outside the pattern's scale (the Q button lights).
pub fn out_of_scale(p: &StepPattern, root: u8, scale: usize) -> bool {
    played_pitches(p).into_iter().any(|x| !in_scale(x, root, scale))
}

/// Moves every row and step note into the scale (the Q button).
pub fn quantize_to_scale(p: &mut StepPattern, root: u8, scale: usize) {
    for r in &mut p.rows {
        let named = r.name == crate::app::note_name(r.pitch);
        r.pitch = conform(r.pitch, root, scale);
        if named {
            r.name = crate::app::note_name(r.pitch);
        }
        for s in r.steps.iter_mut() {
            if s.note != 0 {
                let full = (r.pitch as i32 + s.note as i32).clamp(0, 127) as u8;
                s.note = (conform(full, root, scale) as i32 - r.pitch as i32).clamp(-24, 24) as i8;
            }
        }
    }
}

/// Changes the pattern's key: rows transpose by the root's move (nearest way) unless No
/// Transpose is on; with Scale Quantize on they're conformed to the new scale too.
pub fn set_key(p: &mut StepPattern, from: (u8, usize), to: (u8, usize)) {
    if !p.opts.no_transpose {
        let mut d = (to.0 as i32 - from.0 as i32).rem_euclid(12);
        if d > 6 {
            d -= 12;
        }
        if d != 0 {
            for r in &mut p.rows {
                let named = r.name == crate::app::note_name(r.pitch);
                r.pitch = (r.pitch as i32 + d).clamp(0, 127) as u8;
                if named {
                    r.name = crate::app::note_name(r.pitch);
                }
            }
        }
        if p.opts.scale_quantize {
            quantize_to_scale(p, to.0, to.1);
        }
    }
    p.opts.key = Some(to);
}

// ---- the inspector -----------------------------------------------------------------------------

/// Which part the inspector edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Pattern,
    Row,
    Step,
}

const ROW: f32 = 24.0;
pub const WIDTH: f32 = 236.0;

/// Mixed values: the first value, and whether they differ.
fn first_mixed(v: &[f32]) -> (f32, bool) {
    let f = v.first().copied().unwrap_or(0.0);
    (f, v.iter().any(|x| (x - f).abs() > f32::EPSILON))
}

/// A number row over several values ("*" when they differ). Returns the change (new − first).
fn number(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, id: egui::Id, name: &str, vals: &[f32], min: f32, max: f32, def: f32, fmt: &dyn Fn(f32) -> String) -> Option<f32> {
    p.text(pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, name, look::regular(12.0), look::text_dim());
    let slot = Rect::from_min_max(pos2(r.min.x + r.width() * 0.52, r.min.y + 2.0), pos2(r.max.x - 8.0, r.max.y - 2.0));
    let (f, mixed) = first_mixed(vals);
    let mut v = f;
    let shown = |x: f32| if mixed && x == f { "*".to_string() } else { fmt(x) };
    if widgets::value_slot(ui, slot, id, &mut v, min, max, def, 1.0, &shown) {
        return Some(v - f);
    }
    None
}

fn check(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, id: egui::Id, name: &str, vals: &[bool]) -> Option<bool> {
    p.text(pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, name, look::regular(12.0), look::text_dim());
    let mixed = vals.iter().any(|x| *x != vals[0]);
    let mut on = vals.first().copied().unwrap_or(false);
    let slot = Rect::from_min_max(pos2(r.min.x + r.width() * 0.52, r.min.y + 2.0), pos2(r.max.x - 8.0, r.max.y - 2.0));
    if mixed {
        p.text(pos2(slot.max.x - 26.0, slot.center().y), Align2::RIGHT_CENTER, "*", look::regular(12.0), look::text());
    }
    widgets::check_slot(ui, slot, id, &mut on).then_some(on)
}

fn choice(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, id: egui::Id, name: &str, cur: Option<usize>, names: &[&str]) -> Option<usize> {
    p.text(pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, name, look::regular(12.0), look::text_dim());
    let slot = Rect::from_min_max(pos2(r.min.x + r.width() * 0.40, r.min.y + 2.0), pos2(r.max.x - 8.0, r.max.y - 2.0));
    match cur {
        Some(c) => widgets::choice_slot(ui, slot, id, c, names),
        None => {
            // mixed: "*" shown, any choice sets all
            let mut list: Vec<&str> = vec!["*"];
            list.extend_from_slice(names);
            widgets::choice_slot(ui, slot, id, 0, &list).and_then(|i| i.checked_sub(1))
        }
    }
}

fn same<T: PartialEq + Copy>(v: &[T]) -> Option<T> {
    let f = *v.first()?;
    v.iter().all(|x| *x == f).then_some(f)
}

/// The seven playback buttons (Forward, Backward, Ping-Pong, Pendulum, Wander, Shuffle, Random).
const PLAY_ORDER: [Play; 7] = [Play::Forward, Play::Reverse, Play::PingPong, Play::Pendulum, Play::Wander, Play::Shuffle, Play::Random];

fn play_buttons(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, id: egui::Id, cur: Option<Play>, inherited: bool) -> Option<Play> {
    p.text(pos2(r.min.x + 12.0, r.min.y + 9.0), Align2::LEFT_CENTER, "Playback Mode", look::regular(12.0), look::text_dim());
    let row = Rect::from_min_max(pos2(r.min.x + 12.0, r.min.y + 20.0), pos2(r.max.x - 8.0, r.max.y - 2.0));
    p.rect_filled(row, 6.0, look::group_bg());
    let w = row.width() / 7.0;
    let mut out = None;
    for (k, m) in PLAY_ORDER.iter().enumerate() {
        let b = Rect::from_min_size(pos2(row.min.x + k as f32 * w, row.min.y), vec2(w, row.height()));
        let on = cur == Some(*m);
        if on {
            p.rect_filled(b.shrink(2.0), 5.0, if inherited { look::text_dim().gamma_multiply(0.4) } else { look::accent() });
        }
        let icon = match m {
            Play::Forward => crate::glyphs::ph::ARROW_RIGHT,
            Play::Reverse => crate::glyphs::ph::ARROW_LEFT,
            Play::PingPong => crate::glyphs::ph::ARROWS_LEFT_RIGHT,
            Play::Pendulum => crate::glyphs::ph::ARROW_U_UP_LEFT,
            Play::Wander => crate::glyphs::ph::PATH,
            Play::Shuffle => crate::glyphs::ph::SHUFFLE,
            Play::Random => crate::glyphs::ph::DICE_FIVE,
        };
        crate::glyphs::paint(p, Rect::from_center_size(b.center(), vec2(14.0, 14.0)), icon, 13.0, if on { Color32::WHITE } else { look::text() });
        if ui.interact(b, id.with(k), Sense::click()).on_hover_text(m.name()).clicked() {
            out = Some(*m);
        }
    }
    out
}

fn rate_names() -> Vec<&'static str> {
    RATES.iter().map(|r| r.0).collect()
}

fn rate_index(v: u32) -> usize {
    RATES.iter().position(|r| r.1 == v).unwrap_or(4)
}

/// Draws the inspector into `area` and edits `pat` (rows and steps selected as given). Returns
/// the undo name of a change.
pub fn show(ui: &mut egui::Ui, area: Rect, tab: &mut Tab, pat: &mut StepPattern, rows: &[usize], cells: &[(usize, usize)], project_key: &str) -> Option<&'static str> {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.vline(area.max.x, area.y_range(), Stroke::new(1.0, look::line()));
    let id = ui.scope_id().with("lg-ss-insp");
    // Pattern | Row | Step
    let seg = Rect::from_min_size(pos2(area.min.x + 12.0, area.min.y + 8.0), vec2(area.width() - 24.0, 24.0));
    p.rect_filled(seg, 7.0, look::group_bg());
    for (k, (name, tb)) in [("Pattern", Tab::Pattern), ("Row", Tab::Row), ("Step", Tab::Step)].iter().enumerate() {
        let w = seg.width() / 3.0;
        let b = Rect::from_min_size(pos2(seg.min.x + k as f32 * w, seg.min.y), vec2(w, seg.height()));
        if *tab == *tb {
            p.rect_filled(b.shrink(2.0), 6.0, look::button_on());
        }
        p.text(b.center(), Align2::CENTER_CENTER, *name, if *tab == *tb { look::semibold(12.0) } else { look::regular(12.0) }, look::text());
        if ui.interact(b, id.with(("tab", k)), Sense::click()).clicked() {
            *tab = *tb;
        }
    }
    // the rows scroll under the tabs (wheel)
    let rows_clip = Rect::from_min_max(pos2(area.min.x, seg.max.y + 4.0), area.max);
    ui.set_clip_rect(rows_clip);
    let p = ui.painter_at(rows_clip);
    let (scroll_id, total_id) = (id.with("scroll"), id.with("total"));
    let total: f32 = ui.data(|d| d.get_temp(total_id)).unwrap_or(0.0);
    let mut scroll: f32 = ui.data(|d| d.get_temp(scroll_id)).unwrap_or(0.0);
    if ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| rows_clip.contains(h)) {
        scroll -= ui.input(|i| i.smooth_scroll_delta.y);
    }
    scroll = scroll.clamp(0.0, (total - rows_clip.height() + 16.0).max(0.0));
    ui.data_mut(|d| d.insert_temp(scroll_id, scroll));
    let top = seg.max.y + 10.0;
    let mut y = top - scroll;
    let mut line = |h: f32| {
        let r = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), h));
        y += h;
        r
    };
    let mut changed: Option<&'static str> = None;
    match *tab {
        Tab::Pattern => {
            let o = pat.opts.clone();
            if let Some(d) = number(ui, &p, line(ROW), id.with("len"), "Pattern Length", &[pat.steps as f32], 1.0, MAX_STEPS as f32, 16.0, &|v| format!("{v:.0} Steps")) {
                pat.steps = (pat.steps as f32 + d).clamp(1.0, MAX_STEPS as f32) as u8;
                changed = Some("Pattern Length");
            }
            let ends: Vec<f32> = pat.rows.iter().map(|r| r.start as f32 + 1.0).collect();
            if let Some(d) = number(ui, &p, line(ROW), id.with("ls"), "Loop Start", &ends, 1.0, pat.steps as f32, 1.0, &|v| format!("{v:.0}")) {
                for r in &mut pat.rows {
                    r.start = (r.start as f32 + d).clamp(0.0, pat.steps as f32 - 1.0) as u8;
                }
                changed = Some("Loop Start");
            }
            let lens: Vec<f32> = pat.rows.iter().map(|r| pat.row_len(r) as f32).collect();
            if let Some(d) = number(ui, &p, line(ROW), id.with("le"), "Loop End", &lens, 1.0, pat.steps as f32, pat.steps as f32, &|v| format!("{v:.0}")) {
                let n = pat.steps;
                for r in &mut pat.rows {
                    let l = (pat_len(n, r) as f32 + d).clamp(1.0, n as f32) as u8;
                    r.len = (l != n).then_some(l);
                }
                changed = Some("Loop End");
            }
            if let Some(i) = choice(ui, &p, line(ROW), id.with("rate"), "Step Rate", Some(rate_index(pat.rate)), &rate_names()) {
                pat.rate = RATES[i].1;
                changed = Some("Step Rate");
            }
            if let Some(m) = play_buttons(ui, &p, line(46.0), id.with("play"), Some(pat.play), false) {
                pat.play = m;
                changed = Some("Playback Mode");
            }
            if let Some(on) = check(ui, &p, line(ROW), id.with("legato"), "Legato", &[o.legato]) {
                pat.opts.legato = on;
                changed = Some("Legato");
            }
            if let Some(d) = number(ui, &p, line(ROW), id.with("swing"), "Swing", &[o.swing as f32], 0.0, 100.0, 0.0, &|v| format!("{v:.0} %")) {
                pat.opts.swing = (o.swing as f32 + d).clamp(0.0, 100.0) as u8;
                changed = Some("Swing");
            }
            if let Some(i) = choice(ui, &p, line(ROW), id.with("swdiv"), "Swing Note", Some((o.swing_div == 240) as usize), &["1/8 Note", "1/16 Note"]) {
                pat.opts.swing_div = if i == 0 { 480 } else { 240 };
                changed = Some("Swing Note Division");
            }
            let cur = o.key.unwrap_or_else(|| self::project_key(project_key));
            if let Some(i) = choice(ui, &p, line(ROW), id.with("key"), "Key", Some(cur.0 as usize), &KEYS) {
                set_key(pat, cur, (i as u8, cur.1));
                changed = Some("Pattern Key");
            }
            let scale_names: Vec<&str> = SCALES.iter().map(|s| s.0).collect();
            if let Some(i) = choice(ui, &p, line(ROW), id.with("scale"), "Scale", Some(cur.1), &scale_names) {
                set_key(pat, cur, (cur.0, i));
                changed = Some("Pattern Scale");
            }
            // Scale Quantize, with its Q (lit while some note is outside the scale)
            let sr = line(ROW);
            if let Some(on) = check(ui, &p, sr, id.with("sq"), "Scale Quantize", &[o.scale_quantize]) {
                pat.opts.scale_quantize = on;
                changed = Some("Scale Quantize");
            }
            let q = Rect::from_center_size(pos2(sr.max.x - 48.0, sr.center().y), vec2(22.0, 18.0));
            let lit = out_of_scale(pat, cur.0, cur.1);
            p.rect_filled(q, 4.0, if lit { look::accent() } else { look::group_bg() });
            p.text(q.center(), Align2::CENTER_CENTER, "Q", look::semibold(11.0), if lit { Color32::WHITE } else { look::text_dim() });
            if lit && ui.interact(q, id.with("q"), Sense::click()).on_hover_text("Quantize notes to the scale").clicked() {
                quantize_to_scale(pat, cur.0, cur.1);
                changed = Some("Scale Quantize");
            }
            if let Some(on) = check(ui, &p, line(ROW), id.with("nt"), "No Transpose", &[o.no_transpose]) {
                pat.opts.no_transpose = on;
                changed = Some("No Transpose");
            }
            if let Some(d) = number(ui, &p, line(ROW), id.with("ch"), "MIDI Channel", &[o.channel as f32], 1.0, 16.0, 1.0, &|v| format!("{v:.0}")) {
                pat.opts.channel = (o.channel as f32 + d).clamp(1.0, 16.0) as u8;
                changed = Some("MIDI Channel");
            }
            if let Some(i) = choice(ui, &p, line(ROW), id.with("am"), "Automation", Some((o.auto_mode == AutoMode::Slide) as usize), &["Latch", "Slide"]) {
                pat.opts.auto_mode = if i == 0 { AutoMode::Latch } else { AutoMode::Slide };
                changed = Some("Automation Mode");
            }
        }
        Tab::Row => {
            let rows: Vec<usize> = rows.iter().copied().filter(|&k| k < pat.rows.len()).collect();
            if rows.is_empty() {
                p.text(pos2(area.center().x, y + 20.0), Align2::CENTER_TOP, "No row selected", look::regular(12.0), look::text_dim());
                return None;
            }
            let sel: Vec<Row> = rows.iter().map(|&k| pat.rows[k].clone()).collect();
            // Row Assignment: the note (Chord Degree and Automation rows aren't in Studio)
            let pitches: Vec<f32> = sel.iter().map(|r| r.pitch as f32).collect();
            if let Some(d) = number(ui, &p, line(ROW), id.with("assign"), "Assignment", &pitches, 0.0, 127.0, 60.0, &|v| crate::app::note_name(v as u8)) {
                for &k in &rows {
                    let r = &mut pat.rows[k];
                    let named = r.name == crate::app::note_name(r.pitch);
                    r.pitch = (r.pitch as f32 + d).clamp(0.0, 127.0) as u8;
                    if named {
                        r.name = crate::app::note_name(r.pitch);
                    }
                }
                changed = Some("Row Assignment");
            }
            // Name: typed (one row)
            let nr = line(ROW);
            p.text(pos2(nr.min.x + 12.0, nr.center().y), Align2::LEFT_CENTER, "Name", look::regular(12.0), look::text_dim());
            let slot = Rect::from_min_max(pos2(nr.min.x + nr.width() * 0.40, nr.min.y + 2.0), pos2(nr.max.x - 8.0, nr.max.y - 2.0));
            if rows.len() == 1 {
                let mut name = pat.rows[rows[0]].name.clone();
                let te = ui.put(slot, egui::TextEdit::singleline(&mut name).font(look::regular(12.0)).align(egui::Align2::RIGHT_CENTER));
                if te.changed() {
                    pat.rows[rows[0]].name = name;
                    changed = Some("Rename Row");
                }
            } else {
                p.text(pos2(slot.max.x - 4.0, slot.center().y), Align2::RIGHT_CENTER, "*", look::regular(12.0), look::text());
            }
            let n = pat.steps;
            let starts: Vec<f32> = sel.iter().map(|r| r.start as f32 + 1.0).collect();
            if let Some(d) = number(ui, &p, line(ROW), id.with("rls"), "Loop Start", &starts, 1.0, n as f32, 1.0, &|v| format!("{v:.0}")) {
                for &k in &rows {
                    pat.rows[k].start = (pat.rows[k].start as f32 + d).clamp(0.0, n as f32 - 1.0) as u8;
                }
                changed = Some("Row Loop Start");
            }
            let ends: Vec<f32> = sel.iter().map(|r| pat_len(n, r) as f32).collect();
            if let Some(d) = number(ui, &p, line(ROW), id.with("rle"), "Loop End", &ends, 1.0, n as f32, n as f32, &|v| format!("{v:.0}")) {
                for &k in &rows {
                    let l = (pat_len(n, &pat.rows[k]) as f32 + d).clamp(1.0, n as f32) as u8;
                    pat.rows[k].len = (l != n).then_some(l);
                }
                changed = Some("Row Loop End");
            }
            let rates: Vec<u32> = sel.iter().map(|r| r.rate.unwrap_or(pat.rate)).collect();
            if let Some(i) = choice(ui, &p, line(ROW), id.with("rrate"), "Step Rate", same(&rates).map(rate_index), &rate_names()) {
                for &k in &rows {
                    pat.rows[k].rate = Some(RATES[i].1).filter(|&v| v != pat.rate);
                }
                changed = Some("Row Step Rate");
            }
            let plays: Vec<Option<Play>> = sel.iter().map(|r| r.play).collect();
            let shown = same(&plays).map(|m| m.unwrap_or(pat.play));
            if let Some(m) = play_buttons(ui, &p, line(46.0), id.with("rplay"), shown, same(&plays).is_some_and(|m| m.is_none())) {
                for &k in &rows {
                    pat.rows[k].play = Some(m);
                }
                changed = Some("Row Playback Mode");
            }
            let chs: Vec<f32> = sel.iter().map(|r| r.opts.channel.unwrap_or(pat.opts.channel) as f32).collect();
            if let Some(d) = number(ui, &p, line(ROW), id.with("rch"), "MIDI Channel", &chs, 1.0, 16.0, 1.0, &|v| format!("{v:.0}")) {
                for &k in &rows {
                    let c = pat.rows[k].opts.channel.unwrap_or(pat.opts.channel);
                    pat.rows[k].opts.channel = Some((c as f32 + d).clamp(1.0, 16.0) as u8);
                }
                changed = Some("Row MIDI Channel");
            }
            let ams: Vec<AutoMode> = sel.iter().map(|r| r.opts.auto_mode.unwrap_or(pat.opts.auto_mode)).collect();
            if let Some(i) = choice(ui, &p, line(ROW), id.with("ram"), "Automation", same(&ams).map(|m| (m == AutoMode::Slide) as usize), &["Latch", "Slide"]) {
                for &k in &rows {
                    pat.rows[k].opts.auto_mode = Some(if i == 0 { AutoMode::Latch } else { AutoMode::Slide });
                }
                changed = Some("Row Automation Mode");
            }
        }
        Tab::Step => {
            let cells: Vec<(usize, usize)> = cells.iter().copied().filter(|&(r, k)| r < pat.rows.len() && k < MAX_STEPS).collect();
            if cells.is_empty() {
                p.text(pos2(area.center().x, y + 20.0), Align2::CENTER_TOP, "No step selected", look::regular(12.0), look::text_dim());
                return None;
            }
            let steps: Vec<crate::stepseq::Step> = cells.iter().map(|&(r, k)| pat.rows[r].steps[k]).collect();
            let bools = |f: fn(&crate::stepseq::Step) -> bool| steps.iter().map(f).collect::<Vec<_>>();
            let nums = |f: fn(&crate::stepseq::Step) -> f32| steps.iter().map(f).collect::<Vec<_>>();
            if let Some(on) = check(ui, &p, line(ROW), id.with("on"), "On/Off", &bools(|s| s.on)) {
                each(pat, &cells, &mut |s, _| s.on = on);
                changed = Some("Step On/Off");
            }
            if let Some(d) = number(ui, &p, line(ROW), id.with("chance"), "Chance", &nums(|s| s.chance as f32), 0.0, 100.0, 100.0, &|v| format!("{v:.0} %")) {
                each(pat, &cells, &mut |s, _| s.chance = (s.chance as f32 + d).clamp(0.0, 100.0) as u8);
                changed = Some("Step Chance");
            }
            let conds: Vec<Cond> = steps.iter().map(|s| s.cond).collect();
            let cond_names: Vec<&str> = Cond::ALL.iter().map(|c| c.name()).collect();
            if let Some(i) = choice(ui, &p, line(ROW), id.with("cond"), "Condition", same(&conds).map(|c| Cond::ALL.iter().position(|x| *x == c).unwrap_or(0)), &cond_names) {
                each(pat, &cells, &mut |s, _| s.cond = Cond::ALL[i]);
                changed = Some("Step Condition");
            }
            if same(&conds) == Some(Cond::Recurrence) {
                if let Some(d) = number(ui, &p, line(ROW), id.with("iter"), "Iteration", &nums(|s| s.iter as f32), 1.0, 16.0, 1.0, &|v| format!("{v:.0}")) {
                    each(pat, &cells, &mut |s, _| s.iter = (s.iter as f32 + d).clamp(1.0, 16.0) as u8);
                    changed = Some("Recurrence Iteration");
                }
                if let Some(d) = number(ui, &p, line(ROW), id.with("cyc"), "Cycles", &nums(|s| s.cycles as f32), 1.0, 16.0, 2.0, &|v| format!("{v:.0}")) {
                    each(pat, &cells, &mut |s, _| s.cycles = (s.cycles as f32 + d).clamp(1.0, 16.0) as u8);
                    changed = Some("Recurrence Cycles");
                }
            }
            let srates: Vec<u32> = cells.iter().map(|&(r, k)| pat.rows[r].steps[k].rate.unwrap_or(pat.rows[r].rate.unwrap_or(pat.rate))).collect();
            if let Some(i) = choice(ui, &p, line(ROW), id.with("srate"), "Step Rate", same(&srates).map(rate_index), &rate_names()) {
                each(pat, &cells, &mut |s, row_rate| s.rate = Some(RATES[i].1).filter(|&v| v as usize != row_rate));
                changed = Some("Step Rate");
            }
            if let Some(on) = check(ui, &p, line(ROW), id.with("skip"), "Skip", &bools(|s| s.skip)) {
                each(pat, &cells, &mut |s, _| s.skip = on);
                changed = Some("Step Skip");
            }
            let notes: Vec<f32> = cells.iter().map(|&(r, k)| (pat.rows[r].pitch as i32 + pat.rows[r].steps[k].note as i32) as f32).collect();
            if let Some(d) = number(ui, &p, line(ROW), id.with("note"), "Note", &notes, 0.0, 127.0, 60.0, &|v| crate::app::note_name(v as u8)) {
                each(pat, &cells, &mut |s, _| s.note = (s.note as f32 + d).clamp(-24.0, 24.0) as i8);
                changed = Some("Step Note");
            }
            for (key, name, f, set, min, max, def, unit) in [
                ("vel", "Velocity", (|s: &crate::stepseq::Step| s.velocity as f32) as fn(&crate::stepseq::Step) -> f32, (|s: &mut crate::stepseq::Step, v: f32| s.velocity = v as u8) as fn(&mut crate::stepseq::Step, f32), 1.0, 127.0, 100.0, ""),
                ("gate", "Gate", |s| (s.gate as f32 * 1.27).round(), |s, v| s.gate = ((v / 1.27).round() as u8).clamp(1, 100), 1.0, 127.0, 127.0, ""),
            ] {
                if let Some(d) = number(ui, &p, line(ROW), id.with(key), name, &nums(f), min, max, def, &|v| format!("{v:.0}{unit}")) {
                    each(pat, &cells, &mut |s, _| {
                        let v = (f(s) + d).clamp(min, max);
                        set(s, v);
                        s.on = true;
                    });
                    changed = Some(if key == "vel" { "Step Velocity" } else { "Step Gate" });
                }
            }
            if let Some(on) = check(ui, &p, line(ROW), id.with("tie"), "Tie", &bools(|s| s.tie)) {
                each(pat, &cells, &mut |s, _| s.tie = on);
                changed = Some("Step Tie");
            }
            if let Some(d) = number(ui, &p, line(ROW), id.with("off"), "Offset", &nums(|s| s.offset as f32), -50.0, 50.0, 0.0, &|v| format!("{v:+.0} %")) {
                each(pat, &cells, &mut |s, _| s.offset = (s.offset as f32 + d).clamp(-50.0, 50.0) as i8);
                changed = Some("Step Offset");
            }
            let chs: Vec<f32> = cells.iter().map(|&(r, k)| pat.rows[r].steps[k].channel.or(pat.rows[r].opts.channel).unwrap_or(pat.opts.channel) as f32).collect();
            if let Some(d) = number(ui, &p, line(ROW), id.with("sch"), "MIDI Channel", &chs, 1.0, 16.0, 1.0, &|v| format!("{v:.0}")) {
                let pc = pat.opts.channel;
                for &(r, k) in &cells {
                    let c = pat.rows[r].steps[k].channel.or(pat.rows[r].opts.channel).unwrap_or(pc);
                    pat.rows[r].steps[k].channel = Some((c as f32 + d).clamp(1.0, 16.0) as u8);
                }
                changed = Some("Step MIDI Channel");
            }
            if let Some(d) = number(ui, &p, line(ROW), id.with("art"), "Articulation", &nums(|s| s.articulation as f32), 0.0, 255.0, 0.0, &|v| format!("{v:.0}")) {
                each(pat, &cells, &mut |s, _| s.articulation = (s.articulation as f32 + d).clamp(0.0, 255.0) as u8);
                changed = Some("Step Articulation");
            }
            if let Some(d) = number(ui, &p, line(ROW), id.with("rep"), "Repeat", &nums(|s| s.repeat as f32), 1.0, 16.0, 1.0, &|v| format!("{v:.0}")) {
                each(pat, &cells, &mut |s, _| s.repeat = (s.repeat as f32 + d).clamp(1.0, 16.0) as u8);
                changed = Some("Step Repeat");
            }
            if let Some(d) = number(ui, &p, line(ROW), id.with("ramp"), "Velocity Ramp", &nums(|s| s.ramp as f32), -100.0, 100.0, 0.0, &|v| format!("{v:+.0}")) {
                each(pat, &cells, &mut |s, _| s.ramp = (s.ramp as f32 + d).clamp(-100.0, 100.0) as i8);
                changed = Some("Velocity Ramp");
            }
            if let Some(d) = number(ui, &p, line(ROW), id.with("curve"), "Velocity Curve", &nums(|s| s.curve as f32), -100.0, 100.0, 0.0, &|v| format!("{v:+.0}")) {
                each(pat, &cells, &mut |s, _| s.curve = (s.curve as f32 + d).clamp(-100.0, 100.0) as i8);
                changed = Some("Velocity Curve");
            }
            if let Some(d) = number(ui, &p, line(ROW), id.with("dist"), "Distribution", &nums(|s| s.dist as f32), -100.0, 100.0, 0.0, &|v| format!("{v:+.0}")) {
                each(pat, &cells, &mut |s, _| s.dist = (s.dist as f32 + d).clamp(-100.0, 100.0) as i8);
                changed = Some("Distribution");
            }
        }
    }
    ui.data_mut(|d| d.insert_temp(total_id, y + scroll - top));
    changed
}

/// Runs `f` on each selected step (with its row's rate).
fn each(pat: &mut StepPattern, cells: &[(usize, usize)], f: &mut dyn FnMut(&mut crate::stepseq::Step, usize)) {
    for &(r, k) in cells {
        let rate = pat.rows[r].rate.unwrap_or(pat.rate);
        f(&mut pat.rows[r].steps[k], rate as usize);
    }
}

fn pat_len(steps: u8, r: &Row) -> u8 {
    r.len.unwrap_or(steps).clamp(1, MAX_STEPS as u8)
}

/// The commands this file runs (build.rs and tool/checklist.py read the list).
pub const COMMANDS: &[&str] = &["Show/Hide Local Inspector", "Sort Rows > By Type", "Sort Rows > By Type and Note", "Sort Rows > By Type and Name", "Sort Rows > Ascending", "Sort Rows > Descending"];

/// View ▸ Sort Rows: by type and note (0), type (1) or type and name (2), ascending or not.
/// (Studio's rows are all note rows, so "type" keeps their order.)
pub fn sort_rows(p: &mut StepPattern, by: u8, ascending: bool) {
    match by {
        0 => p.rows.sort_by_key(|r| r.pitch),
        2 => p.rows.sort_by_key(|r| r.name.to_lowercase()),
        _ => return,
    }
    if !ascending {
        p.rows.reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions() {
        assert!(cond_plays(Cond::FirstLoop, 1, 1, 0, 4) && !cond_plays(Cond::FirstLoop, 1, 1, 1, 4));
        assert!(cond_plays(Cond::LastLoop, 1, 1, 3, 4) && !cond_plays(Cond::LastLoop, 1, 1, 2, 4));
        // iteration 2 of 3: the second of every three passes
        let plays: Vec<bool> = (0..6).map(|k| cond_plays(Cond::Recurrence, 2, 3, k, 6)).collect();
        assert_eq!(plays, [false, true, false, false, true, false]);
    }

    #[test]
    fn ramps_and_distribution() {
        // fade: last repeat quieter; swell: first quieter
        assert!(ramp_velocity(100, 3, 4, -50, 0) < 100 && ramp_velocity(100, 0, 4, -50, 0) == 100);
        assert!(ramp_velocity(100, 0, 4, 50, 0) < 100 && ramp_velocity(100, 3, 4, 50, 0) == 100);
        // accelerando: the gaps get shorter
        let a: Vec<f32> = (0..4).map(|k| repeat_at(k, 4, 60)).collect();
        assert!(a[1] - a[0] > a[3] - a[2]);
        let d: Vec<f32> = (0..4).map(|k| repeat_at(k, 4, -60)).collect();
        assert!(d[1] - d[0] < d[3] - d[2]);
    }

    #[test]
    fn keys_and_scales() {
        assert_eq!(project_key("A minor"), (9, 1));
        assert_eq!(conform(61, 0, 0), 60);
        let mut p = StepPattern::melodic();
        let top = p.rows[0].pitch;
        set_key(&mut p, (0, 0), (2, 0));
        assert_eq!(p.rows[0].pitch, top + 2);
        p.opts.no_transpose = true;
        set_key(&mut p, (2, 0), (4, 0));
        assert_eq!(p.rows[0].pitch, top + 2);
        p.opts.no_transpose = false;
        p.opts.scale_quantize = true;
        set_key(&mut p, (4, 0), (0, 1));
        assert!(!out_of_scale(&p, 0, 1));
    }

    #[test]
    fn swing_moves_offbeats() {
        assert_eq!(swing(240, 100, 240), 360);
        assert_eq!(swing(480, 100, 240), 480);
        assert_eq!(swing(240, 0, 240), 240);
    }

    fn one_row() -> StepPattern {
        let mut p = StepPattern::melodic();
        p.rows.truncate(1);
        p.steps = 4;
        p
    }

    #[test]
    fn render_uses_the_inspector_settings() {
        // First Loop: only in the first of four passes (4 steps of 1/16 over a bar)
        let mut p = one_row();
        p.rows[0].steps[0].on = true;
        p.rows[0].steps[0].cond = Cond::FirstLoop;
        assert_eq!(p.render(3840, 1).len(), 1);
        p.rows[0].steps[0].cond = Cond::LastLoop;
        assert_eq!(p.render(3840, 1)[0].pos, 3 * 960);
        // legato: a full gate one tick longer
        p.rows[0].steps[0].cond = Cond::Always;
        p.opts.legato = true;
        assert_eq!(p.render(960, 1)[0].len, 241);
        // channels: step, else row, else pattern
        p.opts.channel = 3;
        assert_eq!(p.render(960, 1)[0].channel, 2);
        p.rows[0].opts.channel = Some(5);
        assert_eq!(p.render(960, 1)[0].channel, 4);
        p.rows[0].steps[0].channel = Some(9);
        assert_eq!(p.render(960, 1)[0].channel, 8);
        // swing pushes the second 1/16 late
        let mut p = one_row();
        p.rows[0].steps[1].on = true;
        p.opts.swing = 50;
        assert_eq!(p.render(960, 1)[0].pos, 240 + 60);
        // a step with its own rate moves the steps after it
        let mut p = one_row();
        p.rows[0].steps[0].on = true;
        p.rows[0].steps[0].rate = Some(480);
        p.rows[0].steps[1].on = true;
        let n = p.render(960, 1);
        assert_eq!(n[1].pos, 480);
        // note repeats with a fade
        let mut p = one_row();
        p.rows[0].steps[0] = crate::stepseq::Step { on: true, repeat: 4, ramp: -80, ..Default::default() };
        let n = p.render(240, 1);
        assert_eq!(n.len(), 4);
        assert!(n[3].velocity < n[0].velocity);
    }

    #[test]
    fn settings_survive_pattern_files() {
        let mut p = one_row();
        p.opts = PatternOpts { legato: true, swing: 30, swing_div: 480, key: Some((2, 1)), no_transpose: true, scale_quantize: true, channel: 4, auto_mode: AutoMode::Slide };
        p.rows[0].opts = RowOpts { channel: Some(7), auto_mode: Some(AutoMode::Latch) };
        p.rows[0].steps[2] = crate::stepseq::Step { on: true, cond: Cond::Recurrence, iter: 2, cycles: 3, rate: Some(480), channel: Some(2), articulation: 200, ramp: -20, curve: 10, dist: 40, ..Default::default() };
        let back = crate::stepseq::from_text(&crate::stepseq::to_text(&p)).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn rows_sort() {
        let mut p = StepPattern::melodic();
        sort_rows(&mut p, 0, true);
        assert!(p.rows.windows(2).all(|w| w[0].pitch <= w[1].pitch));
        sort_rows(&mut p, 0, false);
        assert!(p.rows.windows(2).all(|w| w[0].pitch >= w[1].pitch));
        sort_rows(&mut p, 2, true);
        assert!(p.rows.windows(2).all(|w| w[0].name.to_lowercase() <= w[1].name.to_lowercase()));
    }
}
