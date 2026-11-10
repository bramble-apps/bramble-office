//! Logic's Bass Player and Keyboard Player (Session Players) on instrument tracks: each region's
//! settings (style, the XY pad, the knobs), its performance made by lg-players over the song's
//! chords (the Chord global track; Logic's default C – Am – F – G until chords are set), and the
//! editor for the bottom pane (Main / Details), laid out as the Drummer's.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::TrackKind;
use lg_players::bass::{BassParams, BassStyle};
use lg_players::chords::{parse_bars, ChordAt};
use lg_players::keys::{KeysParams, KeysStyle};
use lg_players::RegionSpec;

use crate::app::App;
use crate::look;
use crate::widgets;

/// A Session Player track's player.
#[derive(Clone, Debug, PartialEq)]
pub enum Player {
    Bass(BassParams),
    Keys(KeysParams),
}

impl Player {
    pub fn style_name(&self) -> String {
        match self {
            Player::Bass(p) => p.style.id().to_string(),
            Player::Keys(p) => p.style.id().to_string(),
        }
    }
    fn xy(&mut self) -> (&mut f32, &mut f32) {
        match self {
            Player::Bass(p) => (&mut p.complexity, &mut p.intensity),
            Player::Keys(p) => (&mut p.complexity, &mut p.intensity),
        }
    }
}

/// The chords the players follow: the chord track's (one per bar from bar 1), repeated to cover
/// `until`.
pub fn chords(app: &App, until: u32) -> Vec<ChordAt> {
    let bar = app.bar_ticks();
    let text = if app.chord_text.trim().is_empty() { "C Am F G" } else { app.chord_text.as_str() };
    let one = parse_bars(text, 0, bar);
    if one.is_empty() {
        return Vec::new();
    }
    let span = one.len() as u32 * bar;
    let mut out = Vec::new();
    let mut off = 0;
    while off < until.max(span) {
        out.extend(one.iter().map(|c| ChordAt { pos: c.pos + off, chord: c.chord }));
        off += span;
    }
    out
}

/// Makes a player region's performance.
pub fn regenerate(app: &mut App, t: usize, r: usize) {
    let Some(player) = app.players.get(&(t, r)).cloned() else { return };
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let end = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map_or(0, |x| x.start + x.length);
    let ch = chords(app, end + 4 * app.bar_ticks());
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        let spec = RegionSpec::new(rg.start, rg.length, sig);
        rg.notes = match &player {
            Player::Bass(p) => lg_players::bass::generate(p, &spec, &ch),
            Player::Keys(p) => lg_players::keys::generate(p, &spec, &ch),
        };
        for n in &mut rg.notes {
            n.channel = 0;
        }
    }
    app.load_engine();
}

/// Every player region again (after the chords change).
pub fn regenerate_all(app: &mut App) {
    let keys: Vec<(usize, usize)> = app.players.keys().copied().collect();
    for (t, r) in keys {
        regenerate(app, t, r);
    }
}

/// File ▸ New Track ▸ Session Player ▸ Bass Player / Keyboard Player: a track with an 8-bar
/// region at the playhead's bar.
pub fn new_track(app: &mut App, keys: bool) {
    app.checkpoint(if keys { "New Keyboard Player Track" } else { "New Bass Player Track" });
    let bar = app.bar_ticks();
    let at = (app.position() as u32 / bar) * bar;
    let player = if keys { Player::Keys(KeysParams::default()) } else { Player::Bass(BassParams::default()) };
    let (name, patch) = if keys { ("Keyboard Player", "Steinway Grand Piano") } else { ("Bass Player", "Fingerstyle Bass") };
    let Some(p) = app.project.as_mut() else { return };
    let row = p.tracks.len() as u32 + 1;
    let region = lg_core::Region { name: player.style_name(), start: at, length: 8 * bar, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default() };
    p.tracks.push(lg_core::Track { name: name.into(), kind: TrackKind::Instrument, channel: 0, row, regions: vec![region], volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new() });
    let t = p.tracks.len() - 1;
    app.programs.push(if keys { 0 } else { 33 });
    app.instrument_choice.insert(t, patch.into());
    app.players.insert((t, 0), player);
    app.selected_track = Some(t);
    app.selected_region = Some((t, 0));
    app.bottom = crate::app::Bottom::PianoRoll;
    app.drum_tab = 0;
    regenerate(app, t, 0);
}

/// The editor for a Bass / Keyboard Player region (Main: style, XY pad, Regenerate, the knobs;
/// Details: feel and the player's own controls; Manual: the Piano Roll).
pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect, t: usize, r: usize) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let Some(mut player) = app.players.get(&(t, r)).cloned() else { return };
    let before = player.clone();
    let green = Color32::from_rgb(0x52, 0xc0, 0x3c);
    let head = Rect::from_min_size(area.min, vec2(area.width(), 34.0));
    p.hline(head.x_range(), head.max.y, Stroke::new(1.0, look::line()));
    let who = Rect::from_min_size(head.min + vec2(12.0, 5.0), vec2(300.0, 24.0));
    let resp = ui.interact(who, ui.scope_id().with(("pl-who", t, r)), Sense::click());
    if resp.hovered() {
        p.rect_filled(who, 5.0, look::group_bg());
    }
    p.text(pos2(who.min.x + 6.0, who.center().y), Align2::LEFT_CENTER, player.style_name(), look::semibold(13.0), green);
    p.text(pos2(who.max.x - 8.0, who.center().y), Align2::RIGHT_CENTER, "⌄", look::regular(12.0), look::text_dim());
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(260.0);
        match &mut player {
            Player::Bass(b) => {
                for s in BassStyle::ALL {
                    if ui.add(egui::Button::selectable(b.style == s, s.id())).clicked() {
                        b.style = s;
                        ui.close();
                    }
                }
            }
            Player::Keys(k) => {
                for s in KeysStyle::ALL {
                    if ui.add(egui::Button::selectable(k.style == s, s.id())).clicked() {
                        k.style = s;
                        ui.close();
                    }
                }
            }
        }
    });
    let chords_txt = if app.chord_text.trim().is_empty() { "C Am F G".to_string() } else { app.chord_text.clone() };
    p.text(pos2(who.max.x + 14.0, who.center().y), Align2::LEFT_CENTER, format!("Chords: {chords_txt}"), look::regular(12.0), look::text_dim());
    let tw = 82.0;
    for (i, n) in ["Main", "Details", "Manual"].iter().enumerate() {
        let tr = Rect::from_center_size(pos2(head.max.x - 14.0 - tw * (2.5 - i as f32), head.center().y), vec2(tw - 4.0, 24.0));
        let on = app.drum_tab == i;
        p.rect_filled(tr, 6.0, if on { green } else { look::group_bg() });
        p.text(tr.center(), Align2::CENTER_CENTER, *n, look::regular(13.0), if on { Color32::BLACK } else { look::text() });
        if ui.interact(tr, ui.scope_id().with(("pltab", i)), Sense::click()).clicked() {
            app.drum_tab = i;
        }
    }
    let body = Rect::from_min_max(pos2(area.min.x + 12.0, head.max.y + 10.0), area.max - vec2(12.0, 8.0));
    if app.drum_tab == 0 {
        // the XY pad and Regenerate
        let side = body.height().clamp(100.0, 220.0) - 30.0;
        let pad = Rect::from_min_size(body.min, vec2(side * 1.35, side));
        p.rect_filled(pad, 8.0, look::lcd_bg());
        p.text(pad.left_bottom() + vec2(6.0, -6.0), Align2::LEFT_BOTTOM, "Simple · Soft", look::regular(10.5), look::lcd_dim());
        p.text(pad.right_bottom() + vec2(-6.0, -6.0), Align2::RIGHT_BOTTOM, "Complex", look::regular(10.5), look::lcd_dim());
        p.text(pad.left_top() + vec2(6.0, 6.0), Align2::LEFT_TOP, "Loud", look::regular(10.5), look::lcd_dim());
        let (cx, iy) = player.xy();
        let resp = ui.interact(pad, ui.scope_id().with(("pl-xy", t, r)), Sense::click_and_drag());
        if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
            *cx = ((pp.x - pad.min.x) / pad.width()).clamp(0.0, 1.0);
            *iy = ((pad.max.y - pp.y) / pad.height()).clamp(0.0, 1.0);
        }
        let puck = pos2(pad.min.x + *cx * pad.width(), pad.max.y - *iy * pad.height());
        p.circle_filled(puck, 9.0, green);
        p.circle_stroke(puck, 9.0, Stroke::new(2.0, Color32::WHITE));
        if ui.put(Rect::from_min_size(pos2(pad.min.x, pad.max.y + 6.0), vec2(pad.width(), 24.0)), egui::Button::new("Regenerate").corner_radius(6.0)).clicked() {
            player = match &player {
                Player::Bass(b) => Player::Bass(b.regenerated()),
                Player::Keys(k) => Player::Keys(k.regenerated()),
            };
        }
        // the knobs
        let kx = pad.max.x + 24.0;
        let kr = |k: f32| Rect::from_min_size(pos2(kx + k * 90.0, pad.min.y), vec2(80.0, 96.0));
        let base = ui.scope_id();
        let id = move |s: &str| base.with((s, t, r));
        let pct = |v: f32| format!("{:.0}", v * 100.0);
        match &mut player {
            Player::Bass(b) => {
                widgets::knob(ui, kr(0.0), id("fills"), &mut b.fills, 0.0, 1.0, 0.3, "Fills", &pct);
                widgets::knob(ui, kr(1.0), id("swing"), &mut b.swing, 0.0, 1.0, 0.0, "Swing", &pct);
                widgets::knob(ui, kr(2.0), id("dead"), &mut b.dead_notes, 0.0, 1.0, 0.2, "Dead Notes", &pct);
                widgets::knob(ui, kr(3.0), id("slides"), &mut b.slides, 0.0, 1.0, 0.2, "Slides", &pct);
            }
            Player::Keys(k) => {
                widgets::knob(ui, kr(0.0), id("fills"), &mut k.fills, 0.0, 1.0, 0.3, "Fills", &pct);
                widgets::knob(ui, kr(1.0), id("swing"), &mut k.swing, 0.0, 1.0, 0.0, "Swing", &pct);
                widgets::knob(ui, kr(2.0), id("grace"), &mut k.grace_notes, 0.0, 1.0, 0.2, "Grace Notes", &pct);
                let mut voicing = k.upper_voicing as f32;
                if widgets::knob(ui, kr(3.0), id("voicing"), &mut voicing, 1.0, 8.0, 5.0, "Voicing", &|v| ["Triad", "Triad + top", "Sevenths", "Suspended", "Add 9", "Open", "Four + 9", "Shells"].get((v.round() as usize).saturating_sub(1)).copied().unwrap_or("").to_string()) {
                    k.upper_voicing = voicing.round() as u8;
                }
            }
        }
    } else if app.drum_tab == 1 {
        let kr = |k: f32| Rect::from_min_size(pos2(body.min.x + k * 92.0, body.min.y + 6.0), vec2(82.0, 100.0));
        let base = ui.scope_id();
        let id = move |s: &str| base.with((s, t, r));
        let feel = |v: f32| if v.abs() < 0.02 { "Center".to_string() } else if v > 0.0 { format!("Push {:.0}", v * 100.0) } else { format!("Pull {:.0}", -v * 100.0) };
        let pct = |v: f32| format!("{:.0}", v * 100.0);
        match &mut player {
            Player::Bass(b) => {
                widgets::knob(ui, kr(0.0), id("feel"), &mut b.feel, -1.0, 1.0, 0.0, "Feel", &feel);
                widgets::knob(ui, kr(1.0), id("humanize"), &mut b.humanize, 0.0, 1.0, 0.2, "Humanize", &pct);
                widgets::knob(ui, kr(2.0), id("dyn"), &mut b.dynamics, 0.0, 2.0, 1.0, "Dynamics", &|v| format!("{:.0} %", v * 100.0));
                widgets::knob(ui, kr(3.0), id("double"), &mut b.double_stops, 0.0, 1.0, 0.0, "Double Stops", &pct);
                widgets::knob(ui, kr(4.0), id("mute-off"), &mut b.mute_offset, -1.0, 1.0, 0.0, "Mute Offset", &|v| format!("{:+.0}", v * 100.0));
                let mut low = b.lowest_note as f32;
                if widgets::knob(ui, kr(5.0), id("lowest"), &mut low, 28.0, 52.0, 40.0, "Lowest Note", &|v| crate::app::note_name(v.round() as u8)) {
                    b.lowest_note = low.round() as u8;
                }
            }
            Player::Keys(k) => {
                widgets::knob(ui, kr(0.0), id("feel"), &mut k.feel, -1.0, 1.0, 0.0, "Feel", &feel);
                widgets::knob(ui, kr(1.0), id("humanize"), &mut k.humanize, 0.0, 1.0, 0.2, "Humanize", &pct);
                widgets::knob(ui, kr(2.0), id("dyn"), &mut k.dynamics, 0.0, 2.0, 1.0, "Dynamics", &|v| format!("{:.0} %", v * 100.0));
                let mut up = k.upper_position as f32;
                if widgets::knob(ui, kr(3.0), id("upper"), &mut up, 57.0, 82.0, 69.0, "Right Hand", &|v| crate::app::note_name(v.round() as u8)) {
                    k.upper_position = up.round() as u8;
                }
                let mut lo = k.lower_position as f32;
                if widgets::knob(ui, kr(4.0), id("lower"), &mut lo, 41.0, 78.0, 55.0, "Left Hand", &|v| crate::app::note_name(v.round() as u8)) {
                    k.lower_position = lo.round() as u8;
                }
            }
        }
    } else {
        p.text(body.center(), Align2::CENTER_CENTER, "Manual: the region's notes are in the Piano Roll (double-click the region)", look::regular(12.0), look::text_dim());
    }
    if player != before {
        app.checkpoint("Change Session Player");
        app.players.insert((t, r), player);
        regenerate(app, t, r);
        app.dirty = true;
    }
}
