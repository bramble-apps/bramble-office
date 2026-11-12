//! Logic's Bass Player and Keyboard Player (Session Players) on instrument tracks: each region's
//! settings (style, the XY pad, the knobs), its performance made by lg-players over the song's
//! chords (the Chord global track; Logic's default C – Am – F – G until chords are set), and the
//! editor for the bottom pane (Main / Details), laid out as the Drummer's.

use lg_core::TrackKind;
use lg_players::bass::BassParams;
use lg_players::chords::{parse_bars, ChordAt};
use lg_players::keys::KeysParams;
use lg_players::RegionSpec;

use crate::app::App;

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

    /// The name Logic gives a new region: the player, then the style ("Bass Player - Pop
    /// Songwriter").
    pub fn region_name(&self) -> String {
        match self {
            Player::Bass(p) => format!("{} - {}", if p.style.synth().is_some() { "Synth Bass Player" } else { "Bass Player" }, p.style.name()),
            Player::Keys(p) => format!("{} - {}", if p.style.synth().is_some() { "Synth Keyboard Player" } else { "Keyboard Player" }, p.style.name()),
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

/// Makes a player region's performance (its notes, and its pitch-bend and controller events:
/// slides, the Synth Player LFO, envelope and ducking, the sustain pedal).
pub fn regenerate(app: &mut App, t: usize, r: usize) {
    let Some(player) = app.players.get(&(t, r)).cloned() else { return };
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let ch = crate::spx::region_chords(app, t, r);
    let bpm = app.project.as_ref().map_or(120.0, |p| p.tempo as f32);
    let follow = crate::spx::follow_onsets(app, t, r);
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        let mut spec = RegionSpec::new(rg.start, rg.length, sig);
        spec.bpm = bpm;
        spec.follow = follow;
        let perf = match &player {
            Player::Bass(p) => lg_players::bass::generate_perf(p, &spec, &ch),
            Player::Keys(p) => lg_players::keys::generate_perf(p, &spec, &ch),
        };
        let mpe = matches!(&player, Player::Bass(b) if b.style.synth().is_some() && b.synth.slides.mono_mode > 0);
        rg.notes = perf.notes;
        if !mpe {
            for n in &mut rg.notes {
                n.channel = 0;
            }
        }
        let start = rg.start;
        rg.ext.events = perf.controls.iter().map(|c| lg_core::events::Control { pos: c.pos.saturating_sub(start), channel: if mpe { c.channel } else { 0 }, ..*c }).collect();
    }
    crate::spx::after_generate(app, t, r);
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
    let region = lg_core::Region { name: player.region_name(), start: at, length: 8 * bar, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
    p.tracks.push(lg_core::Track { name: name.into(), kind: TrackKind::Instrument, channel: 0, row, regions: vec![region], volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new(), automation: Vec::new(), extra: Default::default() });
    let t = p.tracks.len() - 1;
    app.programs.push(if keys { 0 } else { 33 });
    app.instrument_choice.insert(t, patch.into());
    app.players.insert((t, 0), player);
    app.selected_track = Some(t);
    app.selected_region = Some((t, 0));
    app.bottom = crate::app::Bottom::PianoRoll;
    app.drum_tab = 0;
    crate::spx::init_region_chords(app, t, 0);
    regenerate(app, t, 0);
}

/// The editor for a Bass / Keyboard Player region (spedit.rs: Logic 13's Session Player
/// Editor).
pub fn show(app: &mut App, ui: &mut egui::Ui, area: egui::Rect, t: usize, r: usize) {
    crate::spedit::show(app, ui, area, t, r);
}
