//! Session Players beyond the first editor (players.rs): the Preset pop-up (Studio's included
//! presets, user presets saved, deleted, Recall Default, Copy / Paste, Keep Settings when
//! Changing Style), the Session Player type pop-up with Change Patch, the Fills and Swing
//! locks, the Keyboard Player's hand buttons, the Manual view's custom pattern (sixteenth-note
//! steps, Length, pages, Copy / Paste Page and Pattern, Reset), and the region commands:
//! Regenerate, Convert to MIDI Region, Replace with Session Player Region, Create Session
//! Player Region, Populate Track with Session Player Regions.

use std::collections::HashMap;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_players::bass::BassParams;
use lg_players::keys::KeysParams;

use crate::app::App;
use crate::look;
use crate::players::Player;

pub const COMMANDS: &[&str] = &[
    "Regenerate", "Convert to MIDI Region", "Convert to MIDI Region/Cell", "To MIDI Region", "Replace with Session Player Region", "Replace with Session Player Region/Cell", "To Session Player Region",
    "Create Session Player Region", "Create Session Player Region/Cell", "Populate Track with Session Player Regions",
    "Apply Region Chords to Chord Track", "Cut Region Chords", "Copy Region Chords", "Delete Region Chords", "Paste Region Chords",
];

/// A custom pattern: sixteenth-note steps (16 a bar), looped over the region.
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub steps: Vec<bool>,
    pub page: usize,
}

#[derive(Default)]
pub struct State {
    /// User presets: name, the player they're for, its settings.
    pub user: Vec<(String, Player)>,
    loaded: bool,
    pub clip: Option<Player>,
    pub keep_settings: bool,
    pub change_patch: Option<bool>,
    pub lock_fills: bool,
    pub lock_swing: bool,
    /// Keyboard Player hands muted (left, right), per region.
    pub hands: HashMap<(usize, usize), (bool, bool)>,
    pub patterns: HashMap<(usize, usize), Pattern>,
    page_clip: Option<Vec<bool>>,
    pattern_clip: Option<Pattern>,
    save_name: Option<String>,
    /// Region chords: (the region follows them, the chords one per bar from the region's start).
    pub region_chords: HashMap<(usize, usize), (bool, String)>,
    /// Follow Rhythm Of ▸ Track: the track a region follows.
    pub follow_track: HashMap<(usize, usize), usize>,
    /// New Tracks ▸ Use Default Chord Progression for New Regions is off (new regions start on
    /// the key's tonic chord).
    pub tonic_only: bool,
    /// The editor's Main view: 0 the main controls, 1 LFO, 2 Slides / Envelope.
    pub view: u8,
    /// The Patterns dialog / More pop-over / region-chord editor open.
    pub patterns_open: bool,
    pub more_open: bool,
    pub chord_edit: Option<String>,
    /// Cut / Copy Region Chords
    pub chord_clip: Option<String>,
    /// The preview Play button's saved state (region solo, soloed regions, cycle on, cycle).
    pub preview: Option<(bool, Vec<(usize, usize)>, bool, (u32, u32))>,
}

/// The project key: tonic pitch class and minor.
pub fn project_key(app: &App) -> (u8, bool) {
    let k = app.project.as_ref().map(|p| p.key.clone()).unwrap_or_default();
    let pc = lg_players::chords::pitch_class(k.trim()).map_or(0, |x| x.0);
    let low = k.to_ascii_lowercase();
    (pc, low.contains("min") || (low.len() > 1 && low.trim_end().ends_with('m')))
}

/// The style's default chord progression in the project key, as chord names (one per bar).
pub fn default_progression(app: &App) -> String {
    let (key, minor) = project_key(app);
    // a minor key: the relative major's progression
    let major = if minor { (key + 3) % 12 } else { key };
    lg_players::chords::Progression::Default.chords(major, 0, 1, 4).iter().map(|c| c.chord.name()).collect::<Vec<_>>().join(" ")
}

/// The chords a player region plays over: its region chords when it follows them, else the
/// Chord track's; with neither, the tonic chord of the project key.
pub fn region_chords(app: &App, t: usize, r: usize) -> Vec<lg_players::chords::ChordAt> {
    use lg_players::chords::{parse_bars, Chord, ChordAt};
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)) else { return Vec::new() };
    let (start, end) = (rg.start, rg.start + rg.length);
    let bar = app.bar_ticks().max(1);
    if let Some((true, text)) = app.spx.region_chords.get(&(t, r)) {
        let one = parse_bars(text, 0, bar);
        if !one.is_empty() {
            let span = one.len() as u32 * bar;
            let mut out = Vec::new();
            let mut off = 0;
            while start + off < end {
                out.extend(one.iter().map(|c| ChordAt { pos: start + off + c.pos, chord: c.chord }));
                off += span;
            }
            return out;
        }
    }
    if !app.chord_text.trim().is_empty() {
        return crate::players::chords(app, end + 4 * bar);
    }
    let (key, minor) = project_key(app);
    vec![ChordAt { pos: start, chord: if minor { Chord::minor(key) } else { Chord::major(key) } }]
}

/// New player regions get the default progression as region chords (unless the New Tracks
/// checkbox is off: then the tonic chord).
pub fn init_region_chords(app: &mut App, t: usize, r: usize) {
    if !app.spx.tonic_only && app.chord_text.trim().is_empty() {
        let text = default_progression(app);
        app.spx.region_chords.insert((t, r), (true, text));
    }
}

/// The onsets of the track a region follows (Follow Rhythm Of ▸ Track), inside the region.
pub fn follow_onsets(app: &App, t: usize, r: usize) -> Vec<u32> {
    let Some(&src) = app.spx.follow_track.get(&(t, r)) else { return Vec::new() };
    let Some(p) = app.project.as_ref() else { return Vec::new() };
    let Some(rg) = p.tracks.get(t).and_then(|x| x.regions.get(r)) else { return Vec::new() };
    let (a, b) = (rg.start, rg.start + rg.length);
    let mut v: Vec<u32> = p.tracks.get(src).map(|tr| tr.regions.iter().filter(|x| !x.muted).flat_map(|x| x.notes.iter().filter(|n| !n.muted).map(|n| n.pos)).filter(|&pos| pos >= a && pos < b).collect()).unwrap_or_default();
    v.sort();
    v.dedup();
    v
}

/// Studio's included presets (they can't be deleted): settings over the style's defaults.
pub fn included(p: &Player) -> Vec<(&'static str, Player)> {
    let make = |c: f32, i: f32, f: f32, s: f32| {
        let mut x = p.clone();
        match &mut x {
            Player::Bass(b) => {
                let st = b.style;
                *b = BassParams { style: st, complexity: c, intensity: i, fills: f, swing: s, ..BassParams::default() };
            }
            Player::Keys(k) => {
                let st = k.style;
                *k = KeysParams { style: st, complexity: c, intensity: i, fills: f, swing: s, ..KeysParams::default() };
            }
        }
        x
    };
    vec![("Default", default_for(p)), ("Sparse", make(0.15, 0.35, 0.0, 0.0)), ("Steady", make(0.4, 0.6, 0.2, 0.0)), ("Driving", make(0.65, 0.85, 0.4, 0.0)), ("Busy", make(0.9, 0.8, 0.7, 0.1)), ("Swung", make(0.5, 0.6, 0.3, 0.6))]
}

/// The style's own default settings (Recall Default).
pub fn default_for(p: &Player) -> Player {
    match p {
        Player::Bass(b) => Player::Bass(BassParams { style: b.style, seed: b.seed, ..BassParams::default() }),
        Player::Keys(k) => Player::Keys(KeysParams { style: k.style, seed: k.seed, ..KeysParams::default() }),
    }
}

fn presets_file() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("session-player-presets.txt"))
}

/// A player's knob settings as text ("kind name=value…").
fn fields(p: &Player) -> Vec<(&'static str, f32)> {
    match p {
        Player::Bass(b) => vec![("complexity", b.complexity), ("intensity", b.intensity), ("fills", b.fills), ("swing", b.swing), ("feel", b.feel), ("dead", b.dead_notes), ("slides", b.slides), ("double", b.double_stops), ("muteoff", b.mute_offset), ("dynamics", b.dynamics), ("humanize", b.humanize), ("lowest", b.lowest_note as f32), ("pickup", b.pickup_hits as u8 as f32), ("dynmute", b.dynamic_mute as u8 as f32), ("alignroot", b.align_root as u8 as f32)],
        Player::Keys(k) => vec![("complexity", k.complexity), ("intensity", k.intensity), ("fills", k.fills), ("swing", k.swing), ("feel", k.feel), ("grace", k.grace_notes), ("dynamics", k.dynamics), ("humanize", k.humanize), ("upper", k.upper_position as f32), ("lower", k.lower_position as f32), ("voicing", k.upper_voicing as f32), ("arp", k.arp_direction as f32)],
    }
}

fn set_field(p: &mut Player, name: &str, v: f32) {
    match p {
        Player::Bass(b) => match name {
            "complexity" => b.complexity = v,
            "intensity" => b.intensity = v,
            "fills" => b.fills = v,
            "swing" => b.swing = v,
            "feel" => b.feel = v,
            "dead" => b.dead_notes = v,
            "slides" => b.slides = v,
            "double" => b.double_stops = v,
            "muteoff" => b.mute_offset = v,
            "dynamics" => b.dynamics = v,
            "humanize" => b.humanize = v,
            "lowest" => b.lowest_note = v as u8,
            "pickup" => b.pickup_hits = v > 0.5,
            "dynmute" => b.dynamic_mute = v > 0.5,
            "alignroot" => b.align_root = v > 0.5,
            _ => {}
        },
        Player::Keys(k) => match name {
            "complexity" => k.complexity = v,
            "intensity" => k.intensity = v,
            "fills" => k.fills = v,
            "swing" => k.swing = v,
            "feel" => k.feel = v,
            "grace" => k.grace_notes = v,
            "dynamics" => k.dynamics = v,
            "humanize" => k.humanize = v,
            "upper" => k.upper_position = v as u8,
            "lower" => k.lower_position = v as u8,
            "voicing" => k.upper_voicing = v as u8,
            "arp" => k.arp_direction = v as u8,
            _ => {}
        },
    }
}

pub fn to_line(name: &str, p: &Player) -> String {
    let kind = if matches!(p, Player::Bass(_)) { "bass" } else { "keys" };
    let f: Vec<String> = fields(p).into_iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{kind}\t{}\t{}", name.replace('\t', " "), f.join(" "))
}

pub fn from_line(l: &str) -> Option<(String, Player)> {
    let mut it = l.split('\t');
    let kind = it.next()?;
    let name = it.next()?.to_string();
    let mut p = match kind {
        "bass" => Player::Bass(BassParams::default()),
        "keys" => Player::Keys(KeysParams::default()),
        _ => return None,
    };
    for kv in it.next().unwrap_or("").split(' ') {
        if let Some((k, v)) = kv.split_once('=') {
            if let Ok(v) = v.parse() {
                set_field(&mut p, k, v);
            }
        }
    }
    Some((name, p))
}

fn load(app: &mut App) {
    if app.spx.loaded {
        return;
    }
    app.spx.loaded = true;
    if let Some(t) = presets_file().and_then(|p| std::fs::read_to_string(p).ok()) {
        app.spx.user = t.lines().filter_map(from_line).collect();
    }
}

fn save(app: &App) {
    let Some(p) = presets_file() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let text: String = app.spx.user.iter().map(|(n, p)| to_line(n, p) + "\n").collect();
    let _ = std::fs::write(p, text);
}

fn same_kind(a: &Player, b: &Player) -> bool {
    matches!((a, b), (Player::Bass(_), Player::Bass(_)) | (Player::Keys(_), Player::Keys(_)))
}

/// A preset's (or another style's) settings on `p`: the style kept, the locked Fills and Swing
/// kept.
pub fn apply_settings(app: &App, p: &Player, from: &Player) -> Player {
    let mut out = p.clone();
    for (k, v) in fields(from) {
        if (app.spx.lock_fills && k == "fills") || (app.spx.lock_swing && k == "swing") {
            continue;
        }
        set_field(&mut out, k, v);
    }
    out
}

/// A new style chosen in the editor: its defaults, unless Keep Settings when Changing Style
/// (the locks keep Fills and Swing either way).
pub fn style_changed(app: &App, before: &Player, now: &Player) -> Player {
    if app.spx.keep_settings {
        return now.clone();
    }
    let mut d = default_for(now);
    for (k, v) in fields(before) {
        if (app.spx.lock_fills && k == "fills") || (app.spx.lock_swing && k == "swing") {
            set_field(&mut d, k, v);
        }
    }
    d
}

/// The editor's menu bar extras (players.rs draws the rest): the Session Player type pop-up
/// (Keyboard / Bass Player, Change Patch), the Preset pop-up, the Fills and Swing locks.
/// Returns the player changed.
pub fn header(app: &mut App, ui: &mut egui::Ui, head: Rect, t: usize, player: &mut Player) {
    load(app);
    let p = ui.painter_at(head);
    let x0 = head.min.x + 330.0;
    // the type pop-up
    let tr = Rect::from_min_size(pos2(x0, head.min.y + 6.0), vec2(118.0, 22.0));
    let tresp = ui.interact(tr, ui.scope_id().with(("spx-type", t)), Sense::click()).on_hover_text("Session Player: the type");
    p.rect_filled(tr, 5.0, look::group_bg());
    let tname = if matches!(player, Player::Bass(_)) { "Bass Player" } else { "Keyboard Player" };
    p.text(pos2(tr.min.x + 8.0, tr.center().y), Align2::LEFT_CENTER, format!("{tname} ⌄"), look::regular(12.0), look::text());
    let change_patch = app.spx.change_patch.unwrap_or(true);
    egui::Popup::menu(&tresp).show(|ui| {
        ui.set_min_width(200.0);
        for keys in [true, false] {
            let on = matches!(player, Player::Keys(_)) == keys;
            if ui.add(egui::Button::selectable(on, if keys { "Keyboard Player" } else { "Bass Player" })).clicked() && !on {
                *player = if keys { Player::Keys(KeysParams::default()) } else { Player::Bass(BassParams::default()) };
                if change_patch {
                    app.instrument_choice.insert(t, (if keys { "Steinway Grand Piano" } else { "Fingerstyle Bass" }).into());
                    if let Some(p) = app.programs.get_mut(t) {
                        *p = if keys { 0 } else { 33 };
                    }
                }
                ui.close();
            }
        }
        ui.separator();
        if ui.add(egui::Button::selectable(change_patch, "Change Patch")).on_hover_text("Off: choosing another player or style keeps the track's patch").clicked() {
            app.spx.change_patch = Some(!change_patch);
        }
    });
    // the Preset pop-up
    let pr = Rect::from_min_size(pos2(tr.max.x + 8.0, tr.min.y), vec2(120.0, 22.0));
    let presp = ui.interact(pr, ui.scope_id().with(("spx-preset", t)), Sense::click()).on_hover_text("Session Player presets");
    p.rect_filled(pr, 5.0, look::group_bg());
    p.text(pos2(pr.min.x + 8.0, pr.center().y), Align2::LEFT_CENTER, "Preset ⌄", look::regular(12.0), look::text());
    let mut pick: Option<Player> = None;
    egui::Popup::menu(&presp).show(|ui| {
        ui.set_min_width(240.0);
        for (n, x) in included(player) {
            if ui.button(n).clicked() {
                pick = Some(x);
                ui.close();
            }
        }
        let mine: Vec<(usize, String, Player)> = app.spx.user.iter().enumerate().filter(|(_, (_, x))| same_kind(x, player)).map(|(i, (n, x))| (i, n.clone(), x.clone())).collect();
        if !mine.is_empty() {
            ui.separator();
            for (_, n, x) in &mine {
                if ui.button(n).clicked() {
                    pick = Some(x.clone());
                    ui.close();
                }
            }
        }
        ui.separator();
        if ui.button("Save…").clicked() {
            app.spx.save_name = Some(format!("My {tname}"));
            ui.close();
        }
        ui.menu_button("Delete", |ui| {
            if mine.is_empty() {
                ui.add_enabled(false, egui::Button::new("No user presets"));
            }
            for (i, n, _) in &mine {
                if ui.button(n).clicked() {
                    app.spx.user.remove(*i);
                    save(app);
                    ui.close();
                }
            }
        });
        if ui.button("Recall Default").clicked() {
            pick = Some(default_for(player));
            ui.close();
        }
        ui.separator();
        if ui.button("Copy").clicked() {
            app.spx.clip = Some(player.clone());
            ui.close();
        }
        if ui.add_enabled(app.spx.clip.as_ref().is_some_and(|c| same_kind(c, player)), egui::Button::new("Paste")).clicked() {
            pick = app.spx.clip.clone();
            ui.close();
        }
        ui.separator();
        if ui.add(egui::Button::selectable(app.spx.keep_settings, "Keep Settings when Changing Style")).clicked() {
            app.spx.keep_settings = !app.spx.keep_settings;
        }
    });
    if let Some(x) = pick {
        *player = apply_settings(app, player, &x);
    }
    // Save…: the name
    if let Some(mut name) = app.spx.save_name.clone() {
        let mut done = None;
        egui::Modal::new(egui::Id::unique("spx-save")).show(ui.ctx(), |ui| {
            ui.label(egui::RichText::new("Save Session Player Preset").strong());
            ui.text_edit_singleline(&mut name);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
                if ui.button("Save").clicked() {
                    done = Some(true);
                }
            });
        });
        app.spx.save_name = Some(name.clone());
        if let Some(ok) = done {
            app.spx.save_name = None;
            if ok && !name.trim().is_empty() {
                app.spx.user.retain(|(n, x)| !(n == name.trim() && same_kind(x, player)));
                app.spx.user.push((name.trim().to_string(), player.clone()));
                save(app);
            }
        }
    }
}

/// The Keyboard Player's Left Hand and Right Hand buttons (lit: playing; dimmed: muted).
pub fn hand_buttons(app: &mut App, ui: &mut egui::Ui, at: egui::Pos2, t: usize, r: usize) -> bool {
    let (l, rr) = app.spx.hands.get(&(t, r)).copied().unwrap_or_default();
    let p = ui.painter().clone();
    let mut changed = false;
    for (i, (name, muted)) in [("Left Hand", l), ("Right Hand", rr)].into_iter().enumerate() {
        let b = Rect::from_min_size(at + vec2(i as f32 * 98.0 - 96.0, -12.0), vec2(92.0, 24.0));
        let resp = ui.interact(b, ui.scope_id().with(("spx-hand", t, r, i)), Sense::click()).on_hover_text(format!("{name}: click to mute or unmute"));
        p.rect_filled(b, 6.0, if muted { Color32::from_gray(0x36) } else { crate::spedit::yellow() });
        p.text(b.center(), Align2::CENTER_CENTER, name, look::regular(13.0), if muted { look::text_dim() } else { Color32::BLACK });
        if resp.clicked() {
            let e = app.spx.hands.entry((t, r)).or_default();
            if i == 0 {
                e.0 = !e.0;
            } else {
                e.1 = !e.1;
            }
            changed = true;
        }
    }
    changed
}

/// After a performance is made: the custom pattern's rhythm and the hands muted.
pub fn after_generate(app: &mut App, t: usize, r: usize) {
    let pat = app.spx.patterns.get(&(t, r)).cloned();
    let hands = app.spx.hands.get(&(t, r)).copied().unwrap_or_default();
    let split = match app.players.get(&(t, r)) {
        Some(Player::Keys(k)) => Some(k.lower_position),
        _ => None,
    };
    let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) else { return };
    if let Some(sp) = split {
        if hands.0 {
            rg.notes.retain(|n| n.pitch > sp);
        }
        if hands.1 {
            rg.notes.retain(|n| n.pitch <= sp);
        }
    }
    if let Some(p) = pat.filter(|p| p.steps.iter().any(|s| *s)) {
        rg.notes = apply_pattern(&rg.notes, &p.steps, rg.start, rg.length);
    }
}

pub const STEP: u32 = 240;

/// The custom pattern's rhythm on a performance: at each lit step the notes sounding there
/// (else the last ones played before it), held to the next lit step; the pattern loops.
pub fn apply_pattern(notes: &[lg_core::events::Note], steps: &[bool], start: u32, length: u32) -> Vec<lg_core::events::Note> {
    if steps.is_empty() || notes.is_empty() {
        return notes.to_vec();
    }
    let n = steps.len() as u32;
    let mut out = Vec::new();
    let mut i = 0u32;
    while i * STEP < length {
        if steps[(i % n) as usize] {
            let at = start + i * STEP;
            // the length: to the next lit step
            let mut k = i + 1;
            while k * STEP < length && !steps[(k % n) as usize] && k - i < n {
                k += 1;
            }
            let len = ((k - i) * STEP).min(start + length - at).max(60) - 20;
            let mut sounding: Vec<&lg_core::events::Note> = notes.iter().filter(|x| x.pos <= at && x.pos + x.len > at).collect();
            if sounding.is_empty() {
                let last = notes.iter().filter(|x| x.pos <= at).map(|x| x.pos).max().or_else(|| notes.iter().map(|x| x.pos).min());
                sounding = notes.iter().filter(|x| Some(x.pos) == last).collect();
            }
            let mut seen = Vec::new();
            for s in sounding {
                if seen.contains(&s.pitch) {
                    continue;
                }
                seen.push(s.pitch);
                out.push(lg_core::events::Note { pos: at, len, ..*s });
            }
        }
        i += 1;
    }
    out
}

/// The Manual view: the custom pattern's step row (sixteen sixteenth-note steps a page), the
/// page overviews above it, the Length pop-up and the More menu. A click on a step turns the
/// custom pattern on.
pub fn manual(app: &mut App, ui: &mut egui::Ui, body: Rect, t: usize, r: usize) -> bool {
    let mut pat = app.spx.patterns.get(&(t, r)).cloned().unwrap_or(Pattern { steps: vec![false; 16], page: 0 });
    let before = pat.clone();
    let p = ui.painter_at(body);
    let green = Color32::from_rgb(0x52, 0xc0, 0x3c);
    let pages = pat.steps.len().div_ceil(16).max(1);
    pat.page = pat.page.min(pages - 1);
    // the bar: Length, More
    let lr = Rect::from_min_size(body.min, vec2(130.0, 22.0));
    let lresp = ui.interact(lr, ui.scope_id().with(("spx-len", t, r)), Sense::click());
    p.rect_filled(lr, 5.0, look::group_bg());
    let bars = pat.steps.len() as f32 / 16.0;
    p.text(pos2(lr.min.x + 8.0, lr.center().y), Align2::LEFT_CENTER, if pat.steps.len() % 16 == 0 { format!("Length: {bars} Bar{} ⌄", if bars == 1.0 { "" } else { "s" }) } else { format!("Length: {} Steps ⌄", pat.steps.len()) }, look::regular(11.5), look::text());
    egui::Popup::menu(&lresp).show(|ui| {
        for b in [1usize, 2, 4] {
            if ui.button(format!("{b} Bar{}", if b == 1 { "" } else { "s" })).clicked() {
                pat.steps.resize(b * 16, false);
                ui.close();
            }
        }
        ui.separator();
        for s in [4usize, 8, 12, 24, 32, 48, 64] {
            if ui.button(format!("{s} Steps")).clicked() {
                pat.steps.resize(s, false);
                ui.close();
            }
        }
    });
    let mr = Rect::from_min_size(pos2(lr.max.x + 8.0, lr.min.y), vec2(70.0, 22.0));
    let mresp = ui.interact(mr, ui.scope_id().with(("spx-more", t, r)), Sense::click());
    p.rect_filled(mr, 5.0, look::group_bg());
    p.text(mr.center(), Align2::CENTER_CENTER, "More ⌄", look::regular(11.5), look::text());
    egui::Popup::menu(&mresp).show(|ui| {
        let page = pat.page;
        let span = page * 16..((page + 1) * 16).min(pat.steps.len());
        if ui.button("Copy Page").clicked() {
            app.spx.page_clip = Some(pat.steps[span.clone()].to_vec());
            ui.close();
        }
        if ui.add_enabled(app.spx.page_clip.is_some(), egui::Button::new("Paste Page")).clicked() {
            if let Some(c) = app.spx.page_clip.clone() {
                for (k, v) in span.clone().zip(c) {
                    pat.steps[k] = v;
                }
            }
            ui.close();
        }
        ui.separator();
        if ui.button("Copy Pattern").clicked() {
            app.spx.pattern_clip = Some(pat.clone());
            ui.close();
        }
        if ui.add_enabled(app.spx.pattern_clip.is_some(), egui::Button::new("Paste Pattern")).clicked() {
            if let Some(c) = app.spx.pattern_clip.clone() {
                pat = c;
            }
            ui.close();
        }
        ui.separator();
        if ui.button("Reset").clicked() {
            pat.steps.iter_mut().for_each(|s| *s = false);
            ui.close();
        }
    });
    let on = pat.steps.iter().any(|s| *s);
    p.text(pos2(mr.max.x + 12.0, mr.center().y), Align2::LEFT_CENTER, if on { "Custom pattern on" } else { "Click a step to make a custom pattern" }, look::regular(11.0), if on { green } else { look::text_dim() });
    // the page overviews (more than sixteen steps)
    let mut y = lr.max.y + 10.0;
    if pages > 1 {
        for pg in 0..pages {
            let pr = Rect::from_min_size(pos2(body.min.x + pg as f32 * 70.0, y), vec2(64.0, 14.0));
            let resp = ui.interact(pr, ui.scope_id().with(("spx-page", t, r, pg)), Sense::click());
            p.rect_filled(pr, 3.0, if pg == pat.page { look::group_bg().gamma_multiply(1.6) } else { look::group_bg() });
            for k in 0..16 {
                let i = pg * 16 + k;
                if i < pat.steps.len() && pat.steps[i] {
                    p.rect_filled(Rect::from_min_size(pos2(pr.min.x + 2.0 + k as f32 * 3.75, pr.min.y + 3.0), vec2(3.0, 8.0)), 0.0, green);
                }
            }
            if resp.clicked() {
                pat.page = pg;
            }
        }
        y += 22.0;
    }
    // the steps
    let sw = ((body.width() - 10.0) / 16.0).clamp(14.0, 40.0);
    for k in 0..16 {
        let i = pat.page * 16 + k;
        if i >= pat.steps.len() {
            break;
        }
        let sr = Rect::from_min_size(pos2(body.min.x + k as f32 * sw, y), vec2(sw - 4.0, 34.0));
        let resp = ui.interact(sr, ui.scope_id().with(("spx-step", t, r, i)), Sense::click());
        let lit = pat.steps[i];
        p.rect_filled(sr, 4.0, if lit { green } else if k % 4 == 0 { look::group_bg().gamma_multiply(1.4) } else { look::group_bg() });
        p.rect_stroke(sr, 4.0, Stroke::new(1.0, look::line()), egui::StrokeKind::Inside);
        if resp.clicked() {
            pat.steps[i] = !lit;
        }
    }
    if pat != before {
        app.spx.patterns.insert((t, r), pat);
        return true;
    }
    false
}

// ---- region commands -------------------------------------------------------------------------

fn player_regions(app: &App) -> Vec<(usize, usize)> {
    app.region_selection().into_iter().filter(|x| app.players.contains_key(x)).collect()
}

/// The kind of player a track already has (else a Keyboard Player).
fn track_player(app: &App, t: usize) -> Player {
    app.players.iter().filter(|((tt, _), _)| *tt == t).map(|(_, p)| default_for(p)).next().unwrap_or(Player::Keys(KeysParams::default()))
}

/// A new Session Player region on track `t` over `start..start + len`.
fn new_region(app: &mut App, t: usize, start: u32, len: u32, name: Option<String>) -> Option<usize> {
    let player = track_player(app, t);
    let tr = app.project.as_mut()?.tracks.get_mut(t)?;
    if tr.kind != lg_core::TrackKind::Instrument {
        return None;
    }
    let region = lg_core::Region { name: name.unwrap_or_else(|| player.region_name()), start, length: len, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
    tr.regions.push(region);
    let r = tr.regions.len() - 1;
    app.players.insert((t, r), player);
    init_region_chords(app, t, r);
    crate::players::regenerate(app, t, r);
    Some(r)
}

/// The free span from `start` (up to `len`) before the next region on the track.
fn free_len(app: &App, t: usize, start: u32, len: u32) -> u32 {
    let next = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.iter().map(|r| r.start).filter(|&s| s > start).min());
    next.map_or(len, |n| len.min(n - start)).max(1)
}

pub fn command(app: &mut App, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
    }
    match name {
        "Apply Region Chords to Chord Track" | "Cut Region Chords" | "Copy Region Chords" | "Delete Region Chords" | "Paste Region Chords" => {
            let sel = player_regions(app);
            let Some(&first) = sel.first() else { return false };
            let text = app.spx.region_chords.get(&first).map(|x| x.1.clone()).unwrap_or_default();
            match name {
                "Apply Region Chords to Chord Track" if !text.is_empty() => {
                    app.checkpoint("Apply Region Chords to Chord Track");
                    app.chord_text = text;
                }
                "Copy Region Chords" => app.spx.chord_clip = Some(text),
                "Paste Region Chords" => {
                    let Some(c) = app.spx.chord_clip.clone() else { return false };
                    app.checkpoint("Paste Region Chords");
                    for k in &sel {
                        app.spx.region_chords.insert(*k, (true, c.clone()));
                    }
                }
                _ => {
                    app.checkpoint(name);
                    if name == "Cut Region Chords" {
                        app.spx.chord_clip = Some(text);
                    }
                    for k in &sel {
                        app.spx.region_chords.remove(k);
                    }
                }
            }
            crate::players::regenerate_all(app);
            app.dirty = true;
        }
        "Regenerate" => {
            let sel = player_regions(app);
            let drums: Vec<(usize, usize)> = app.region_selection().into_iter().filter(|(t, _)| app.project.as_ref().and_then(|p| p.tracks.get(*t)).is_some_and(|tr| tr.kind == lg_core::TrackKind::Drummer)).collect();
            if sel.is_empty() && drums.is_empty() {
                return false;
            }
            app.checkpoint("Regenerate");
            for (t, r) in sel {
                if let Some(p) = app.players.get_mut(&(t, r)) {
                    *p = match p {
                        Player::Bass(b) => Player::Bass(b.regenerated()),
                        Player::Keys(k) => Player::Keys(k.regenerated()),
                    };
                }
                crate::players::regenerate(app, t, r);
            }
            for (t, r) in drums {
                let p = app.drum_params.get(&(t, r)).cloned().unwrap_or_default().regenerated();
                app.drum_params.insert((t, r), p);
                crate::drummer::regenerate(app, t, r);
            }
            app.dirty = true;
        }
        "Convert to MIDI Region" | "Convert to MIDI Region/Cell" | "To MIDI Region" => {
            let sel = player_regions(app);
            if sel.is_empty() {
                return false;
            }
            app.checkpoint("Convert to MIDI Region");
            for x in sel {
                app.players.remove(&x);
                app.spx.patterns.remove(&x);
                app.spx.hands.remove(&x);
            }
            app.dirty = true;
        }
        "Replace with Session Player Region" | "Replace with Session Player Region/Cell" | "To Session Player Region" => {
            let sel: Vec<(usize, usize)> = app.region_selection().into_iter().filter(|x| !app.players.contains_key(x) && app.project.as_ref().and_then(|p| p.tracks.get(x.0)).is_some_and(|tr| tr.kind == lg_core::TrackKind::Instrument && tr.regions.get(x.1).is_some_and(|r| r.audio.is_none()))).collect();
            if sel.is_empty() {
                return false;
            }
            // (the MIDI edits are discarded: the player plays the region anew)
            app.checkpoint("Replace with Session Player Region");
            for (t, r) in sel {
                let p = track_player(app, t);
                app.players.insert((t, r), p);
                crate::players::regenerate(app, t, r);
            }
            app.dirty = true;
        }
        "Create Session Player Region" | "Create Session Player Region/Cell" => {
            let Some(t) = app.selected_track else { return false };
            let bar = app.bar_ticks();
            // a marquee, else the cycle area, else eight bars at the playhead's bar
            let (start, len) = if let Some(m) = app.ta.regions.marquee.clone().filter(|m| m.tracks.contains(&t) && m.to > m.from) {
                (m.from, m.to - m.from)
            } else if app.cycle_on() && app.cycle.1 > app.cycle.0 {
                (app.cycle.0, app.cycle.1 - app.cycle.0)
            } else {
                let at = (app.position() as u32 / bar) * bar;
                (at, free_len(app, t, at, 8 * bar))
            };
            app.checkpoint("Create Session Player Region");
            let Some(r) = new_region(app, t, start, len, None) else { return false };
            app.selected_region = Some((t, r));
            app.dirty = true;
        }
        "Populate Track with Session Player Regions" => {
            let Some(t) = app.selected_track else { return false };
            let arr: Vec<(u32, u32, String)> = app.project.as_ref().map_or(Vec::new(), |p| p.arrangement.clone());
            app.checkpoint("Populate Track with Session Player Regions");
            let bar = app.bar_ticks();
            let spans: Vec<(u32, u32, Option<String>)> = if app.cycle_on() && app.cycle.1 > app.cycle.0 {
                vec![(app.cycle.0, app.cycle.1 - app.cycle.0, None)]
            } else if !arr.is_empty() {
                arr.into_iter().map(|(s, l, n)| (s, l, Some(n))).collect()
            } else {
                vec![(0, 8 * bar, None)]
            };
            let mut made = 0;
            for (s, l, n) in spans {
                // (the gaps only: regions already there stay)
                let busy = app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.regions.iter().any(|r| r.start < s + l && r.start + r.length > s));
                if !busy && new_region(app, t, s, l, n).is_some() {
                    made += 1;
                }
            }
            if made == 0 {
                app.tell("No free span to fill on this track");
            }
            app.dirty = true;
        }
        _ => return false,
    }
    app.load_engine();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(pos: u32, len: u32, pitch: u8) -> lg_core::events::Note {
        lg_core::events::Note { channel: 0, pitch, velocity: 90, pos, len, muted: false }
    }

    #[test]
    fn custom_pattern_rhythm() {
        let notes = vec![note(0, 3840, 48), note(0, 3840, 52), note(3840, 3840, 50)];
        let mut steps = vec![false; 16];
        steps[0] = true;
        steps[6] = true;
        let out = apply_pattern(&notes, &steps, 0, 7680);
        // two hits a bar, two notes each in bar 1, one in bar 2
        assert_eq!(out.iter().filter(|n| n.pos < 3840).count(), 4);
        assert_eq!(out.iter().filter(|n| n.pos >= 3840).count(), 2);
        assert!(out.iter().any(|n| n.pos == 6 * STEP && n.pitch == 52));
        assert!(out.iter().all(|n| n.pos % STEP == 0));
        // held to the next lit step
        assert_eq!(out[0].len, 6 * STEP - 20);
    }

    #[test]
    fn presets_round_trip() {
        let mut b = BassParams { complexity: 0.3, pickup_hits: true, lowest_note: 23, ..BassParams::default() };
        b.fills = 0.8;
        let p = Player::Bass(b);
        let (n, q) = from_line(&to_line("Mine", &p)).unwrap();
        assert_eq!(n, "Mine");
        assert_eq!(fields(&q), fields(&p));
        assert!(included(&p).iter().any(|x| x.0 == "Default"));
    }
}
