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
use lg_players::drummer::{DrummerParams, KitSection, SnareMode, CYMBAL_CHOICES, CYMBAL_VARIATIONS, KICK_SNARE_VARIATIONS, PERCUSSION_VARIATIONS, PERC_CHOICES, TOM_CHOICES};
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
    let wide = spedit::is_wide(area.width());
    if wide {
        // "Preset:" and the drummer's preset pop-up after Chords
        preset_popup(ui, area, t, r, &mut params);
    }
    let mut manual = app.spdrum.manual.get(&(t, r)).cloned().unwrap_or_default();
    let manual_before = manual.clone();
    let bar_t = app.bar_ticks().max(1);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
    egui::ScrollArea::vertical().id_salt(("sd-scroll", t, r)).auto_shrink([false, false]).show(&mut child, |ui| {
        let w = body.width();
        let top = ui.cursor().min;
        let cx = top.x + w / 2.0;
        let cxs = Cx { p: ui.painter().clone(), base: ui.scope_id().with(("spdrum", t, r)) };
        if wide {
            let f = spedit::wide_frame(app, ui, &cxs, top, w, spedit::Picture::Drums, &mut params.complexity, &mut params.intensity);
            who_button(ui, &cxs, f.label, &mut params);
            if f.regen {
                params = params.regenerated();
            }
            let o = f.panel;
            let bg = cxs.p.add(Shape::Noop);
            let (h, knobs) = match f.tab {
                0 => (wide_main(app, ui, &cxs, o, &mut params, &region, kit, bar_t, t, r), true),
                1 => (details_panel(app, ui, &cxs, o, PANEL_W, &mut params, true), true),
                _ => (manual_panel(ui, &cxs, o, spedit::WIDE_PANEL_W, &mut manual).max(spedit::WIDE_PANEL_H), false),
            };
            spedit::wide_panel_bg(&cxs, bg, o, spedit::WIDE_PANEL_W, h, knobs);
            ui.allocate_exact_size(vec2(w, o.y + h + 24.0 - top.y), Sense::hover());
            return;
        }
        let y0 = top.y + 22.0;
        // the Session Player button: Acoustic / Electronic Drummer over the drummer
        who_button(ui, &cxs, pos2(cx - 83.0, y0), &mut params);
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
            1 => details_panel(app, ui, &cxs, o, pw, &mut params, false),
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

/// The Session Player button: Acoustic / Electronic Drummer over the drummer (yellow); a click
/// chooses the drummer by genre.
fn who_button(ui: &mut egui::Ui, cxs: &Cx, c: Pos2, params: &mut DrummerParams) {
    let ch = params.character();
    cxs.label(c, if ch.electronic() { "Electronic Drummer" } else { "Acoustic Drummer" });
    let sr = Rect::from_center_size(c + vec2(0.0, 9.0), vec2(150.0, 38.0));
    let sresp = ui.interact(sr, cxs.id("who"), Sense::click()).on_hover_text("Session Player: choose the drummer");
    cxs.p.text(c + vec2(0.0, 18.0), Align2::CENTER_CENTER, ch.name, look::regular(13.0), if sresp.hovered() { Color32::WHITE } else { yellow() });
    egui::Popup::menu(&sresp).show(|ui| {
        ui.set_min_width(240.0);
        for g in GENRES {
            ui.menu_button(g.name(), |ui| {
                for c in drummers_in(g) {
                    if ui.add(egui::Button::selectable(c.id == ch.id, c.name).shortcut_text(format!("{} · {}", c.personality, c.kit))).clicked() {
                        *params = DrummerParams { seed: params.seed, ..DrummerParams::for_character(c) };
                        ui.close();
                    }
                }
            });
        }
    });
}

/// The wide bar's "Preset:" pop-up for a drummer: the drummer's own settings ("Default") or
/// "Custom"; Recall Default.
fn preset_popup(ui: &mut egui::Ui, area: Rect, t: usize, r: usize, params: &mut DrummerParams) {
    let cy = area.min.y + spedit::BAR_CY;
    ui.painter().text(pos2(area.min.x + 101.0, cy), Align2::LEFT_CENTER, "Preset:", look::regular(13.0), look::text_dim());
    let pr = Rect::from_min_size(pos2(area.min.x + 150.0, cy - 11.0), vec2(252.0, 22.0));
    let default = DrummerParams { seed: params.seed, ..DrummerParams::for_character(params.character()) };
    let resp = ui.interact(pr, ui.scope_id().with(("sd-preset", t, r)), Sense::click()).on_hover_text("Drummer presets");
    crate::spx::preset_field(&ui.painter_at(pr.expand(2.0)), pr, if *params == default { "Default" } else { "Custom" }, resp.hovered());
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(240.0);
        if ui.add(egui::Button::selectable(*params == default, "Default")).clicked() || ui.button("Recall Default").clicked() {
            *params = default.clone();
            ui.close();
        }
    });
}

fn piece_on(p: &DrummerParams, i: usize) -> bool {
    match i {
        0 => p.percussion.enabled,
        1 => p.cymbals.enabled,
        2 => p.crashes_on,
        3 => p.toms_on,
        _ => p.kick_snare.enabled,
    }
}

fn set_piece(p: &mut DrummerParams, i: usize, on: bool) {
    match i {
        0 => p.percussion.enabled = on,
        1 => p.cymbals.enabled = on,
        2 => p.crashes_on = on,
        3 => p.toms_on = on,
        _ => p.kick_snare.enabled = on,
    }
}

/// A kit piece drawn as line art (the wide layout's, as Logic's figure): the hi-hat, a cymbal, a
/// shaker, the kick drum from the front, a drum from the side.
fn kit_art(p: &egui::Painter, b: Rect, i: usize, col: Color32) {
    let c = b.center();
    let line = egui::Stroke::new(1.3, col);
    let fill = col.gamma_multiply(0.35);
    match i {
        1 => {
            for dy in [-5.0, -2.0] {
                p.add(Shape::ellipse_filled(c + vec2(0.0, dy), vec2(11.0, 2.6), fill));
                p.add(Shape::ellipse_stroke(c + vec2(0.0, dy), vec2(11.0, 2.6), line));
            }
            p.line_segment([c + vec2(0.0, -10.0), c + vec2(0.0, 10.0)], line);
            p.line_segment([c + vec2(-6.0, 10.0), c + vec2(6.0, 10.0)], line);
        }
        2 => {
            let pts: Vec<Pos2> = (0..24).map(|k| {
                let a = k as f32 / 24.0 * std::f32::consts::TAU;
                let (x, y) = (a.cos() * 12.0, a.sin() * 3.2);
                c + vec2(x * 0.96 + y * 0.28, y - x * 0.28)
            }).collect();
            p.add(Shape::convex_polygon(pts, fill, line));
        }
        0 => {
            let r = Rect::from_center_size(c, vec2(10.0, 21.0));
            p.rect_filled(r, 3.0, fill);
            p.rect_stroke(r, 3.0, line, egui::StrokeKind::Middle);
            for dy in [-5.0, 0.0, 5.0] {
                p.hline(r.x_range(), c.y + dy, egui::Stroke::new(1.0, col));
            }
        }
        4 => {
            p.circle_filled(c + vec2(0.0, -1.0), 10.0, fill);
            p.circle_stroke(c + vec2(0.0, -1.0), 10.0, line);
            p.circle_stroke(c + vec2(0.0, -1.0), 4.0, egui::Stroke::new(1.0, col));
            p.line_segment([c + vec2(-6.0, 7.0), c + vec2(-9.0, 12.0)], line);
            p.line_segment([c + vec2(6.0, 7.0), c + vec2(9.0, 12.0)], line);
        }
        _ => {
            let r = Rect::from_center_size(c + vec2(0.0, 1.0), vec2(22.0, 13.0));
            p.rect_filled(r, 2.0, fill);
            p.rect_stroke(r, 2.0, line, egui::StrokeKind::Middle);
            for k in 1..4 {
                p.vline(r.min.x + r.width() * k as f32 / 4.0, r.y_range(), egui::Stroke::new(1.0, col));
            }
        }
    }
}

/// A kit-piece button (lit: plays, dimmed: muted); a click plays or mutes the group. `size` 0:
/// line art (wide), else the icon at that size.
fn piece_button(ui: &mut egui::Ui, cx: &Cx, b: Rect, p: &mut DrummerParams, i: usize, size: f32) {
    let on = piece_on(p, i);
    let resp = ui.interact(b, cx.id("piece").with(i), Sense::click()).on_hover_text(format!("{}: click to play or mute", GROUPS[i].0));
    let col = if on { yellow() } else { Color32::from_gray(0x60) };
    if size == 0.0 {
        kit_art(&cx.p, b, i, col);
    } else {
        glyphs::paint(&cx.p, b, piece_icon(i), size, col);
    }
    if resp.clicked() {
        set_piece(p, i, !on);
    }
}

/// A group's pattern variation badge (A = Auto, the pad decides); a click picks the next,
/// Option-click Auto.
fn variation_badge(ui: &mut egui::Ui, cx: &Cx, badge: Pos2, p: &mut DrummerParams, i: usize) {
    let sec: Option<(&mut KitSection, u8)> = match i {
        0 => Some((&mut p.percussion, PERCUSSION_VARIATIONS)),
        1 => Some((&mut p.cymbals, CYMBAL_VARIATIONS)),
        4 => Some((&mut p.kick_snare, KICK_SNARE_VARIATIONS)),
        _ => None,
    };
    if let Some((sec, max)) = sec {
        let resp = ui.interact(Rect::from_center_size(badge, vec2(20.0, 20.0)), cx.id("var").with(i), Sense::click()).on_hover_text("Pattern variation: click for the next, Option-click for Auto");
        cx.p.circle_filled(badge, 9.0, Color32::from_gray(if resp.hovered() { 0x4a } else { 0x3a }));
        let txt = if sec.variation == 0 { "A".to_string() } else { format!("{}", sec.variation) };
        cx.p.text(badge, Align2::CENTER_CENTER, txt, look::regular(11.5), spedit::label_col());
        if resp.clicked() {
            sec.variation = if ui.input(|x| x.modifiers.alt) { 0 } else { (sec.variation + 1) % (max + 1) };
        }
    }
}

/// The wide Main panel (Logic's figure): the cymbals and percussion row (Hi-Hat, Cymbals,
/// Percussion) and the kick and snare row (Kick & Snare, Toms), each with its variation badge
/// and dots (kick and snare: two lines), Follow under them; the knobs in the right column.
#[allow(clippy::too_many_arguments)]
fn wide_main(app: &mut App, ui: &mut egui::Ui, cx: &Cx, o: Pos2, p: &mut DrummerParams, region: &lg_core::Region, kit: KitMap, bar_t: u32, t: usize, r: usize) -> f32 {
    let percussion_only = p.character().genre == Genre::Percussion;
    for (k, i) in [1usize, 2, 0].into_iter().enumerate() {
        piece_button(ui, cx, Rect::from_center_size(pos2(o.x + 37.0 + k as f32 * 48.0, o.y + 51.0), vec2(30.0, 28.0)), p, i, 0.0);
    }
    for (k, i) in [4usize, 3].into_iter().enumerate() {
        piece_button(ui, cx, Rect::from_center_size(pos2(o.x + 62.0 + k as f32 * 47.0, o.y + 157.0), vec2(30.0, 28.0)), p, i, 0.0);
    }
    cx.p.hline(o.x..=(o.x + PANEL_W), o.y + 104.0, egui::Stroke::new(1.0, Color32::from_gray(0x1e)));
    variation_badge(ui, cx, pos2(o.x + 185.0, o.y + 51.0), p, 1);
    variation_badge(ui, cx, pos2(o.x + 185.0, o.y + 157.0), p, 4);
    let top_dots = group_dots(&region.notes, kit, &[Family::HiHat, Family::Cymbal], region.start, bar_t);
    let snare = group_dots(&region.notes, kit, &[Family::Snare], region.start, bar_t);
    let kick = group_dots(&region.notes, kit, &[Family::Kick], region.start, bar_t);
    let sp = 12.1;
    cx.dots(pos2(o.x + 213.0, o.y + 51.0), &top_dots, sp);
    cx.dots(pos2(o.x + 213.0, o.y + 149.0), &snare, sp);
    cx.dots(pos2(o.x + 213.0, o.y + 162.0), &kick, sp);
    if !percussion_only {
        let follow = match app.spx.follow_track.get(&(t, r)).and_then(|f| app.project.as_ref()?.tracks.get(*f)) {
            Some(tr) => tr.name.clone(),
            None if !app.spdrum.chords_off.get(&(t, r)).copied().unwrap_or(false) => "Chord Rhythm".into(),
            None => "Off".into(),
        };
        let g = cx.p.layout_no_wrap("Follow: ".into(), look::regular(11.5), spedit::label_col());
        let fy = o.y + 180.0;
        let fr = Rect::from_min_size(pos2(o.x + 209.0, fy - 8.0), vec2(200.0, 16.0));
        let gw = g.size().x;
        cx.p.galley(pos2(fr.min.x, fy - g.size().y / 2.0), g, spedit::label_col());
        cx.p.text(pos2(fr.min.x + gw, fy), Align2::LEFT_CENTER, follow, look::regular(11.5), yellow());
        if ui.interact(fr, cx.id("follow"), Sense::click()).on_hover_text("Kick and snare patterns: Follow Rhythm Of").clicked() {
            app.spdrum.patterns_open = true;
        }
    }
    spedit::knob_column(app, ui, cx, pos2(o.x + PANEL_W, o.y), &mut p.fills, 0.35, &mut p.fill_complexity, &mut p.swing, &mut p.swing_type);
    spedit::WIDE_PANEL_H
}

#[allow(clippy::too_many_arguments)]
fn main_panel(app: &mut App, ui: &mut egui::Ui, cx: &Cx, o: Pos2, w: f32, p: &mut DrummerParams, region: &lg_core::Region, kit: KitMap, bar_t: u32) -> f32 {
    let percussion_only = p.character().genre == Genre::Percussion;
    let mut y = o.y + 26.0;
    for (i, (name, fams)) in GROUPS.iter().enumerate() {
        let on = piece_on(p, i);
        // the kit-piece button: lit plays, dimmed mutes
        piece_button(ui, cx, Rect::from_center_size(pos2(o.x + 34.0, y), vec2(32.0, 28.0)), p, i, 20.0);
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
        variation_badge(ui, cx, pos2(o.x + 172.0, y), p, i);
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

fn details_panel(app: &mut App, ui: &mut egui::Ui, cx: &Cx, o: Pos2, w: f32, p: &mut DrummerParams, wide: bool) -> f32 {
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
    // Feel, Dynamics, Tempo, Humanize: the strip (wide: the right column)
    if wide {
        let h = (y + 28.0).max(spedit::WIDE_PANEL_H);
        spedit::feel_controls(ui, cx, Rect::from_min_size(pos2(o.x + PANEL_W, o.y), vec2(spedit::KNOBS_W, h)), true, &mut p.feel, &mut p.dynamics, &mut p.tempo, &mut p.humanize, 0.22);
        return h;
    }
    spedit::feel_controls(ui, cx, Rect::from_min_size(pos2(o.x, o.y + y + 28.0), vec2(w, 130.0)), false, &mut p.feel, &mut p.dynamics, &mut p.tempo, &mut p.humanize, 0.22);
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
