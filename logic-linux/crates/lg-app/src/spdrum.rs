//! The Session Player Editor for a Drummer region, laid out as Logic 13's (spedit.rs draws the
//! top and the controls): the Session Player button (Acoustic / Electronic Drummer over the
//! drummer), Complexity and Intensity, Regenerate, the Main / Details / Manual tabs and the dark
//! panel. Main: a row per kit piece group (Percussion, Hi-Hat, Cymbals, Toms, Kick & Snare) with
//! its kit-piece button (lit: plays, dimmed: muted), the pattern's dots (brighter: stronger
//! accents) and the pattern variation; Fill Amount and Fill Complexity with the lock, Swing with
//! 8th / 16th. Details: Ghost Notes, the Snare / Percussion / Hi-Hat pop-ups (Cymbals and Toms
//! when that piece is selected), electronic drummers' kit-piece complexity sliders and Phrase
//! Variation, then Feel / Dynamics / Tempo / Humanize. Manual: a step row per kit piece. The
//! kick and snare Patterns dialog: Follow Rhythm Of (Track, Chords) and Brush Sweep / Taps.

use std::collections::HashMap;

use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Shape};
use lg_players::character::{drummers_in, Genre};
use lg_players::drummer::{DrummerParams, KitSection, SnareMode, TempoFeel, CYMBAL_CHOICES, CYMBAL_VARIATIONS, KICK_SNARE_VARIATIONS, PERCUSSION_VARIATIONS, PERC_CHOICES, TOM_CHOICES};
use lg_players::kit::{Family, KitMap};

use crate::app::App;
use crate::glyphs;
use crate::look;
use crate::spedit::{self, yellow, Cx, PANEL_R, PANEL_W};

const GENRES: [Genre; 7] = [Genre::Rock, Genre::Alternative, Genre::Songwriter, Genre::RnB, Genre::Electronic, Genre::HipHop, Genre::Percussion];

/// The kit piece groups (Main view rows, Manual rows).
pub const GROUPS: [(&str, &[Family]); 5] = [("Percussion", &[Family::Percussion]), ("Hi-Hat", &[Family::HiHat]), ("Cymbals", &[Family::Cymbal]), ("Toms", &[Family::Tom]), ("Kick & Snare", &[Family::Kick, Family::Snare])];

/// The Manual view's rows (a kit piece each).
pub const MANUAL_ROWS: [(&str, Family); 6] = [("Kick", Family::Kick), ("Snare", Family::Snare), ("Hi-Hat", Family::HiHat), ("Cymbal", Family::Cymbal), ("Toms", Family::Tom), ("Percussion", Family::Percussion)];

/// A Drummer region's custom pattern: sixteen steps per kit-piece row, and the rows muted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrumManual {
    pub steps: [[bool; 16]; 6],
    pub muted: [bool; 6],
}

impl DrumManual {
    pub fn active(&self) -> bool {
        self.steps.iter().any(|r| r.iter().any(|s| *s))
    }
}

#[derive(Default)]
pub struct State {
    pub manual: HashMap<(usize, usize), DrumManual>,
    /// Follow Rhythm Of ▸ Chords off (on by default)
    pub chords_off: HashMap<(usize, usize), bool>,
    /// The kit piece group selected in Main (its Details pop-ups show)
    pub selected: Option<usize>,
    pub patterns_open: bool,
}

fn piece_icon(i: usize) -> &'static str {
    match i {
        0 => glyphs::ph::BELL_SIMPLE,
        1 => glyphs::ph::DISC,
        2 => glyphs::ph::CIRCLE_HALF,
        3 => glyphs::ph::CYLINDER,
        _ => glyphs::br::DRUM,
    }
}

/// The custom pattern and the muted rows on a performance: a muted row's piece is taken out; a
/// row with steps replaces that piece's hits by the steps, bar after bar (velocity from the
/// piece's own hits).
pub fn apply_manual(notes: &mut Vec<lg_core::events::Note>, m: &DrumManual, kit: KitMap, start: u32, length: u32, bar_t: u32) {
    let fam = |n: &lg_core::events::Note| kit.piece(n.pitch).map(|p| p.family());
    for (row, (_, f)) in MANUAL_ROWS.iter().enumerate() {
        let is = |n: &lg_core::events::Note| fam(n) == Some(*f) || (*f == Family::Percussion && fam(n).is_none());
        if m.muted[row] {
            notes.retain(|n| !is(n));
            continue;
        }
        if !m.steps[row].iter().any(|s| *s) {
            continue;
        }
        let mine: Vec<&lg_core::events::Note> = notes.iter().filter(|n| is(n)).collect();
        let pitch = mine.first().map(|n| n.pitch).unwrap_or_else(|| kit.note(match f {
            Family::Kick => lg_players::kit::Piece::Kick,
            Family::Snare => lg_players::kit::Piece::Snare,
            Family::HiHat => lg_players::kit::Piece::HatClosed,
            Family::Cymbal => lg_players::kit::Piece::Crash,
            Family::Tom => lg_players::kit::Piece::TomMid,
            _ => lg_players::kit::Piece::Tambourine,
        }));
        let vel = if mine.is_empty() { 90 } else { (mine.iter().map(|n| n.velocity as u32).sum::<u32>() / mine.len() as u32) as u8 };
        let ch = mine.first().map_or(9, |n| n.channel);
        notes.retain(|n| !is(n));
        let step = bar_t / 16;
        let mut b = start;
        while b < start + length {
            for (k, on) in m.steps[row].iter().enumerate() {
                let pos = b + k as u32 * step;
                if *on && pos < start + length {
                    notes.push(lg_core::events::Note { channel: ch, pitch, velocity: if k % 4 == 0 { vel.saturating_add(8).min(127) } else { vel }, pos, len: step, muted: false });
                }
            }
            b += bar_t;
        }
    }
    notes.sort_by_key(|n| (n.pos, n.pitch));
}

/// The rhythm a drummer region follows: the chosen track's onsets, and the chord changes
/// (Follow Rhythm Of ▸ Chords, on by default; not for Percussion drummers).
pub fn follow_onsets(app: &App, t: usize, r: usize, percussion: bool) -> Vec<u32> {
    let mut v = crate::spx::follow_onsets(app, t, r);
    if !percussion && !app.spdrum.chords_off.get(&(t, r)).copied().unwrap_or(false) && !app.chord_text.trim().is_empty() {
        if let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)) {
            let (a, b) = (rg.start, rg.start + rg.length);
            let ch = crate::players::chords(app, b);
            let mut last = None;
            for c in ch {
                if Some(c.chord) != last && c.pos >= a && c.pos < b {
                    v.push(c.pos);
                }
                last = Some(c.chord);
            }
        }
    }
    v.sort();
    v.dedup();
    v
}

/// The first bar's hits of a group as 16 levels (0 none, 1 weaker, 2 stronger).
fn group_dots(notes: &[lg_core::events::Note], kit: KitMap, fams: &[Family], start: u32, bar_t: u32) -> lg_players::synth::Dots {
    let mut d = [0u8; 16];
    let step = (bar_t / 16).max(1);
    for n in notes.iter().filter(|n| n.pos >= start && n.pos < start + bar_t) {
        let f = kit.piece(n.pitch).map(|p| p.family()).unwrap_or(Family::Percussion);
        if fams.contains(&f) {
            let k = (((n.pos - start) + step / 2) / step).min(15) as usize;
            d[k] = d[k].max(if n.velocity >= 85 { 2 } else { 1 });
        }
    }
    d
}

/// The editor.
pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect, t: usize, r: usize) {
    let Some(region) = app.project.as_ref().and_then(|pr| pr.tracks.get(t)?.regions.get(r)).cloned() else { return };
    let mut params = app.drum_params.get(&(t, r)).cloned().unwrap_or_default();
    let before = params.clone();
    let ch = params.character();
    let kit = ch.kit_map();
    let mut regen = false;
    let Some(body) = spedit::top(app, ui, area, t, r, Some(kit), &mut regen) else { return };
    let mut manual = app.spdrum.manual.get(&(t, r)).cloned().unwrap_or_default();
    let manual_before = manual.clone();
    let bar_t = app.bar_ticks().max(1);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
    egui::ScrollArea::vertical().id_salt(("sd-scroll", t, r)).auto_shrink([false, false]).show(&mut child, |ui| {
        let w = body.width();
        let top = ui.cursor().min;
        let cx = top.x + w / 2.0;
        let cxs = Cx { p: ui.painter().clone(), base: ui.scope_id().with(("spdrum", t, r)) };
        let y0 = top.y + 22.0;
        // the Session Player button: Acoustic / Electronic Drummer over the drummer
        cxs.label(pos2(cx - 83.0, y0), if ch.electronic() { "Electronic Drummer" } else { "Acoustic Drummer" });
        let sr = Rect::from_center_size(pos2(cx - 83.0, y0 + 9.0), vec2(150.0, 38.0));
        let sresp = ui.interact(sr, cxs.id("who"), Sense::click()).on_hover_text("Session Player: choose the drummer");
        cxs.p.text(pos2(cx - 83.0, y0 + 18.0), Align2::CENTER_CENTER, ch.name, look::regular(13.0), if sresp.hovered() { Color32::WHITE } else { yellow() });
        egui::Popup::menu(&sresp).show(|ui| {
            ui.set_min_width(240.0);
            for g in GENRES {
                ui.menu_button(g.name(), |ui| {
                    for c in drummers_in(g) {
                        if ui.add(egui::Button::selectable(c.id == ch.id, c.name).shortcut_text(format!("{} · {}", c.personality, c.kit))).clicked() {
                            params = DrummerParams { seed: params.seed, ..DrummerParams::for_character(c) };
                            ui.close();
                        }
                    }
                });
            }
        });
        cxs.label(pos2(cx + 21.0, y0), "Complexity");
        cxs.label(pos2(cx + 105.0, y0), "Intensity");
        let st = y0 + 24.0;
        cxs.vslider(ui, Rect::from_min_size(pos2(cx + 19.0, st), vec2(4.0, 83.0)), "cx", &mut params.complexity);
        cxs.vslider(ui, Rect::from_min_size(pos2(cx + 103.0, st), vec2(4.0, 83.0)), "iy", &mut params.intensity);
        let rg = Rect::from_center_size(pos2(cx - 83.0, st + 83.0), vec2(83.0, 24.0));
        let rresp = ui.interact(rg, cxs.id("regen"), Sense::click());
        cxs.p.rect_filled(rg, 6.0, Color32::from_gray(if rresp.hovered() { 0x4c } else { 0x41 }));
        cxs.p.text(rg.center(), Align2::CENTER_CENTER, "Regenerate", look::regular(13.0), look::text());
        if rresp.clicked() {
            params = params.regenerated();
        }
        let tabs_c = st + 83.0 + 46.0;
        let mut tab = app.drum_tab.min(2);
        if cxs.seg(ui, Rect::from_center_size(pos2(cx, tabs_c), vec2(171.0, 24.0)), "tabs", &["Main", "Details", "Manual"], &mut tab) {
            app.drum_tab = tab;
        }
        let pw = PANEL_W.min(w - 16.0);
        let o = pos2(cx - pw / 2.0, tabs_c + 23.0);
        let bg = cxs.p.add(Shape::Noop);
        let h = match tab {
            0 => main_panel(app, ui, &cxs, o, pw, &mut params, &region, kit, bar_t),
            1 => details_panel(app, ui, &cxs, o, pw, &mut params),
            _ => manual_panel(ui, &cxs, o, pw, &mut manual),
        };
        cxs.p.set(bg, Shape::rect_filled(Rect::from_min_size(o, vec2(pw, h)), PANEL_R, spedit::panel_bg()));
        ui.allocate_exact_size(vec2(w, o.y + h + 24.0 - top.y), Sense::hover());
    });
    patterns_dialog(app, ui, t, r, &mut params);
    if manual != manual_before {
        app.checkpoint("Drummer Pattern");
        app.spdrum.manual.insert((t, r), manual);
        regen = true;
    }
    if params != before {
        app.checkpoint("Change Session Player");
        app.drum_params.insert((t, r), params);
        regen = true;
    }
    if regen {
        crate::drummer::regenerate(app, t, r);
        app.dirty = true;
    }
}

#[allow(clippy::too_many_arguments)]
fn main_panel(app: &mut App, ui: &mut egui::Ui, cx: &Cx, o: Pos2, w: f32, p: &mut DrummerParams, region: &lg_core::Region, kit: KitMap, bar_t: u32) -> f32 {
    let percussion_only = p.character().genre == Genre::Percussion;
    let mut y = o.y + 26.0;
    for (i, (name, fams)) in GROUPS.iter().enumerate() {
        let on = match i {
            0 => p.percussion.enabled,
            1 => p.cymbals.enabled,
            2 => p.crashes_on,
            3 => p.toms_on,
            _ => p.kick_snare.enabled,
        };
        // the kit-piece button: lit plays, dimmed mutes
        let ib = Rect::from_center_size(pos2(o.x + 34.0, y), vec2(32.0, 28.0));
        let iresp = ui.interact(ib, cx.id("piece").with(i), Sense::click()).on_hover_text(format!("{name}: click to play or mute"));
        glyphs::paint(&cx.p, ib, piece_icon(i), 20.0, if on { yellow() } else { Color32::from_gray(0x60) });
        if iresp.clicked() {
            match i {
                0 => p.percussion.enabled = !on,
                1 => p.cymbals.enabled = !on,
                2 => p.crashes_on = !on,
                3 => p.toms_on = !on,
                _ => p.kick_snare.enabled = !on,
            }
        }
        // the name selects the group (its Details pop-ups)
        let nr = Rect::from_min_size(pos2(o.x + 56.0, y - 11.0), vec2(96.0, 22.0));
        let sel = app.spdrum.selected == Some(i);
        let nresp = ui.interact(nr, cx.id("sel").with(i), Sense::click());
        if sel {
            cx.p.rect_filled(nr, 6.0, Color32::from_gray(0x2c));
        }
        cx.p.text(pos2(nr.min.x + 6.0, y), Align2::LEFT_CENTER, *name, look::regular(13.0), if on { spedit::label_col() } else { look::text_dim() });
        if nresp.clicked() {
            app.spdrum.selected = if sel { None } else { Some(i) };
        }
        // the pattern variation (a badge: A = Auto, the pad decides) and its dots (from the
        // region's first bar)
        let sec: Option<(&mut KitSection, u8)> = match i {
            0 => Some((&mut p.percussion, PERCUSSION_VARIATIONS)),
            1 => Some((&mut p.cymbals, CYMBAL_VARIATIONS)),
            4 => Some((&mut p.kick_snare, KICK_SNARE_VARIATIONS)),
            _ => None,
        };
        let badge = pos2(o.x + 172.0, y);
        if let Some((sec, max)) = sec {
            let resp = ui.interact(Rect::from_center_size(badge, vec2(20.0, 20.0)), cx.id("var").with(i), Sense::click()).on_hover_text("Pattern variation: click for the next, Option-click for Auto");
            cx.p.circle_filled(badge, 9.0, Color32::from_gray(if resp.hovered() { 0x4a } else { 0x3a }));
            let txt = if sec.variation == 0 { "A".to_string() } else { format!("{}", sec.variation) };
            cx.p.text(badge, Align2::CENTER_CENTER, txt, look::regular(11.5), spedit::label_col());
            if resp.clicked() {
                sec.variation = if ui.input(|x| x.modifiers.alt) { 0 } else { (sec.variation + 1) % (max + 1) };
            }
        }
        let d = group_dots(&region.notes, kit, fams, region.start, bar_t);
        cx.dots(pos2(o.x + 196.0, y), &d, 11.0);
        if i == 4 && !percussion_only {
            let pr = Rect::from_center_size(pos2(o.x + w - 26.0, y), vec2(22.0, 22.0));
            let resp = ui.interact(pr, cx.id("ks-patterns"), Sense::click()).on_hover_text("Kick and snare patterns: Follow Rhythm Of");
            glyphs::paint(&cx.p, pr, glyphs::ph::DOTS_THREE_CIRCLE, 18.0, if resp.hovered() { Color32::WHITE } else { spedit::label_col() });
            if resp.clicked() {
                app.spdrum.patterns_open = true;
            }
        }
        y += 34.0;
    }
    // Fill Amount, Fill Complexity (one lock keeps both), Swing with 8th / 16th
    y += 8.0;
    let pct = |v: f32| format!("{:.0}", v * 100.0);
    let cols = [o.x + w * 0.2, o.x + w * 0.42, o.x + w * 0.74];
    cx.knob(ui, pos2(cols[0], y), "fills", "Fill Amount", &mut p.fills, 0.0, 1.0, 0.35, &pct);
    cx.knob(ui, pos2(cols[1], y), "fillc", "Fill Complexity", &mut p.fill_complexity, 0.0, 1.0, 0.5, &pct);
    spedit::locks(app, ui, cx, pos2((cols[0] + cols[1]) / 2.0, y + 92.0));
    cx.knob(ui, pos2(cols[2], y), "swing", "Swing", &mut p.swing, 0.0, 1.0, 0.0, &pct);
    spedit::swing_type(ui, cx, pos2(cols[2], y + 92.0), &mut p.swing_type);
    y + 120.0 - o.y
}

fn details_panel(app: &mut App, ui: &mut egui::Ui, cx: &Cx, o: Pos2, w: f32, p: &mut DrummerParams) -> f32 {
    let pct = |v: f32| format!("{:.0}", v * 100.0);
    let at = |fx: f32, dy: f32| pos2(o.x + fx / PANEL_W * w, o.y + dy);
    let electronic = p.character().electronic();
    cx.knob(ui, at(229.5, 38.0), "ghost", "Ghost Notes", &mut p.ghost_notes, 0.0, 1.0, 0.22, &pct);
    let mut y = 183.0;
    if electronic {
        // each kit piece's complexity, and Phrase Variation
        y = 140.0;
        for (k, name) in ["Kick", "Snare / Clap", "Hi-Hat", "Percussion"].iter().enumerate() {
            let tr = Rect::from_min_size(at(40.0, y + 6.0 + k as f32 * 40.0), vec2(w * 0.8, 6.0));
            cx.hslider(ui, tr, &format!("pc{k}"), name, &mut p.piece_complexity[k]);
        }
        y += 160.0;
        let tr = Rect::from_min_size(at(40.0, y + 6.0), vec2(w * 0.8, 6.0));
        cx.hslider(ui, tr, "pvar", "Phrase Variation", &mut p.phrase_variation);
        y += 40.0;
    } else {
        // Snare, Percussion, Hi-Hat (as Logic's row); Cymbals and Toms when selected in Main
        let snares = [SnareMode::Automatic, SnareMode::Snare, SnareMode::SideStick, SnareMode::Rimshot];
        let mut si = snares.iter().position(|s| *s == p.snare).unwrap_or(0);
        if cx.popup(ui, at(76.0, 154.0), "snare", "Snare", &["Automatic", "Snare", "Side Stick", "Rimshot"], &mut si) {
            p.snare = snares[si];
        }
        let mut pi = if p.percussion.enabled { p.perc_choice as usize + 1 } else { 0 };
        let perc_opts: Vec<&str> = std::iter::once("Off").chain(PERC_CHOICES.iter().copied()).collect();
        if cx.popup(ui, at(230.0, 154.0), "perc", "Percussion", &perc_opts, &mut pi) {
            p.percussion.enabled = pi > 0;
            p.perc_choice = pi.saturating_sub(1) as u8;
        }
        let hats = [0.1f32, 0.35, 0.7];
        let mut hi = if p.hihat < 0.2 { 0 } else if p.hihat > 0.6 { 2 } else { 1 };
        if cx.popup(ui, at(383.0, 154.0), "hat", "Hi-Hat", &["Closed", "Half-Closed", "Open"], &mut hi) {
            p.hihat = hats[hi];
        }
        match app.spdrum.selected {
            Some(2) => {
                let mut c = p.cymbal_choice as usize;
                if cx.popup(ui, at(230.0, y - 6.0), "cym", "Cymbals", &CYMBAL_CHOICES, &mut c) {
                    p.cymbal_choice = c as u8;
                }
                y += 44.0;
            }
            Some(3) => {
                let mut c = p.toms as usize;
                if cx.popup(ui, at(230.0, y - 6.0), "toms", "Toms", &TOM_CHOICES, &mut c) {
                    p.toms = c as u8;
                }
                y += 44.0;
            }
            _ => {}
        }
    }
    // the strip: Feel, Dynamics, Tempo, Humanize
    let sy = o.y + y + 28.0;
    let strip = Rect::from_min_size(pos2(o.x, sy), vec2(w, 130.0));
    cx.p.rect_filled(strip, egui::CornerRadius { nw: 0, ne: 0, sw: PANEL_R as u8, se: PANEL_R as u8 }, spedit::strip_bg());
    let sat = |fx: f32| pos2(o.x + fx / PANEL_W * w, sy + 21.0);
    cx.knob(ui, sat(80.0), "feel", "Feel", &mut p.feel, -1.0, 1.0, 0.0, &|v| if v.abs() < 0.02 { "Center".into() } else if v > 0.0 { format!("Push {:.0}", v * 100.0) } else { format!("Pull {:.0}", -v * 100.0) });
    cx.p.text(sat(80.0) + vec2(-20.0, 87.0), Align2::CENTER_CENTER, "Pull", look::regular(12.0), look::text_dim());
    cx.p.text(sat(80.0) + vec2(20.0, 87.0), Align2::CENTER_CENTER, "Push", look::regular(12.0), look::text_dim());
    cx.knob(ui, sat(180.0), "dyn", "Dynamics", &mut p.dynamics, 0.0, 2.0, 1.0, &|v| format!("{:.0} %", v * 100.0));
    let mut ti = TempoFeel::ALL.iter().position(|x| *x == p.tempo).unwrap_or(1);
    if cx.popup(ui, sat(280.0), "tempo", "Tempo", &TempoFeel::ALL.map(|x| x.name()), &mut ti) {
        p.tempo = TempoFeel::ALL[ti];
    }
    cx.knob(ui, sat(380.0), "human", "Humanize", &mut p.humanize, 0.0, 1.0, 0.22, &pct);
    y + 28.0 + 130.0
}

/// The Manual view: a step row per kit piece; the kit-piece button at the row's start mutes it.
fn manual_panel(ui: &mut egui::Ui, cx: &Cx, o: Pos2, w: f32, m: &mut DrumManual) -> f32 {
    let x0 = o.x + 112.0;
    let sw = ((w - 132.0) / 16.0).floor();
    for (row, (name, _)) in MANUAL_ROWS.iter().enumerate() {
        let y = o.y + 22.0 + row as f32 * 30.0;
        let b = Rect::from_min_size(pos2(o.x + 14.0, y - 11.0), vec2(90.0, 22.0));
        let muted = m.muted[row];
        cx.p.rect_filled(b, 6.0, if muted { spedit::button_bg() } else { yellow() });
        cx.p.text(b.center(), Align2::CENTER_CENTER, *name, look::regular(12.0), if muted { look::text_dim() } else { Color32::BLACK });
        if ui.interact(b, cx.id("mrow").with(row), Sense::click()).on_hover_text(format!("{name}: mute or unmute")).clicked() {
            m.muted[row] = !muted;
        }
        for k in 0..16 {
            let sr = Rect::from_min_size(pos2(x0 + k as f32 * sw, y - 10.0), vec2(sw - 3.0, 20.0));
            let on = m.steps[row][k];
            cx.p.rect_filled(sr, 4.0, if on { yellow() } else if k % 4 == 0 { Color32::from_gray(0x33) } else { Color32::from_gray(0x26) });
            if ui.interact(sr, cx.id("mstep").with((row, k)), Sense::click()).clicked() {
                m.steps[row][k] = !on;
            }
        }
    }
    let cr = Rect::from_center_size(pos2(o.x + w / 2.0, o.y + 22.0 + 6.0 * 30.0 + 6.0), vec2(150.0, 24.0));
    cx.p.rect_filled(cr, 6.0, spedit::button_bg());
    cx.p.text(cr.center(), Align2::CENTER_CENTER, "Clear Pattern", look::regular(13.0), spedit::label_col());
    if ui.interact(cr, cx.id("mclear"), Sense::click()).clicked() {
        m.steps = [[false; 16]; 6];
    }
    6.0 * 30.0 + 60.0
}

/// The kick and snare Patterns dialog: Follow Rhythm Of (a track, the chords; not for
/// Percussion drummers) and, for brush drummers, Brush Sweep / Brush Taps.
fn patterns_dialog(app: &mut App, ui: &mut egui::Ui, t: usize, r: usize, p: &mut DrummerParams) {
    if !app.spdrum.patterns_open {
        return;
    }
    let percussion = p.character().genre == Genre::Percussion;
    let brush = p.character().style.groove == lg_players::character::Groove::Brush;
    let tracks: Vec<(usize, String)> = app.project.as_ref().map(|pr| pr.tracks.iter().enumerate().filter(|(i, tr)| *i != t && tr.regions.iter().any(|x| !x.notes.is_empty())).map(|(i, tr)| (i, format!("{} {}", i + 1, tr.name))).collect()).unwrap_or_default();
    let mut follow = app.spx.follow_track.get(&(t, r)).copied();
    let mut chords = !app.spdrum.chords_off.get(&(t, r)).copied().unwrap_or(false);
    let (f0, c0) = (follow, chords);
    let mut close = false;
    egui::Modal::new(egui::Id::unique(format!("sd-patterns-{t}-{r}"))).show(ui.ctx(), |ui| {
        ui.set_width(360.0);
        ui.label(egui::RichText::new("Kick & Snare").strong());
        if !percussion {
            ui.label("Follow Rhythm Of");
            egui::ComboBox::from_label("Track").selected_text(follow.and_then(|f| tracks.iter().find(|x| x.0 == f)).map_or("None".to_string(), |x| x.1.clone())).show_ui(ui, |ui| {
                ui.selectable_value(&mut follow, None, "None");
                for (i, n) in &tracks {
                    ui.selectable_value(&mut follow, Some(*i), n);
                }
            });
            ui.checkbox(&mut chords, "Chords");
            ui.add(egui::Slider::new(&mut p.follow, 0.0..=1.0).text("Follow strength"));
        }
        if brush {
            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut p.brush_taps, false, "Brush Sweep");
                ui.selectable_value(&mut p.brush_taps, true, "Brush Taps");
            });
        }
        if ui.button("Done").clicked() {
            close = true;
        }
    });
    if close {
        app.spdrum.patterns_open = false;
    }
    if follow != f0 || chords != c0 {
        match follow {
            Some(f) => app.spx.follow_track.insert((t, r), f),
            None => app.spx.follow_track.remove(&(t, r)),
        };
        app.spdrum.chords_off.insert((t, r), !chords);
        if (follow.is_some() || chords) && p.follow == 0.0 {
            p.follow = 0.6;
        }
        crate::drummer::regenerate(app, t, r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_rows() {
        let kit = KitMap::DrumKitDesigner;
        let note = |pos: u32, piece: lg_players::kit::Piece| lg_core::events::Note { channel: 9, pitch: kit.note(piece), velocity: 80, pos, len: 100, muted: false };
        let mut notes = vec![note(0, lg_players::kit::Piece::Kick), note(960, lg_players::kit::Piece::Snare), note(0, lg_players::kit::Piece::HatClosed)];
        let mut m = DrumManual::default();
        m.muted[2] = true;
        m.steps[0][0] = true;
        m.steps[0][8] = true;
        apply_manual(&mut notes, &m, kit, 0, 3840 * 2, 3840);
        assert!(!notes.iter().any(|n| kit.piece(n.pitch).map(|p| p.family()) == Some(Family::HiHat)), "the muted row");
        assert_eq!(notes.iter().filter(|n| n.pitch == kit.note(lg_players::kit::Piece::Kick)).count(), 4, "two steps a bar for two bars");
        assert_eq!(notes.iter().filter(|n| n.pitch == kit.note(lg_players::kit::Piece::Snare)).count(), 1, "the other rows untouched");
    }
}
