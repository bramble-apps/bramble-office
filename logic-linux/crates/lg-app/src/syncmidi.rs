//! Synchronization (lg_core::sync does the protocols): Studio sends MIDI Clock with Song
//! Position Pointer and MIDI Time Code from a timing thread, sends and listens to MIDI Machine
//! Control, and follows incoming MTC when the Sync Mode is MTC and external sync is on (the
//! control bar's Sync, Toggle Sync intern/extern). Project Settings ▸ Synchronization holds the
//! choices; Project Settings ▸ MIDI ▸ Chase ▸ Notes makes notes already under way sound from a
//! play start or locate.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lg_core::events::PPQ;
use lg_core::sync::{self, ClockOut, Mmc, MtcIn, MtcOut, Rate, Tc};

use crate::app::App;

pub const COMMANDS: &[&str] = &["Toggle Sync intern/extern", "Synchronization…"];

/// What the timing thread sends, refreshed each frame.
#[derive(Clone, Default)]
struct Cfg {
    clock_port: String,
    song_mode: bool,
    mtc_port: String,
    rate: Option<Rate>,
    offset: f64,
    tempo: f64,
    map: Vec<(u32, f64)>,
    delay_ms: f64,
    alive: bool,
}

#[derive(Default)]
pub struct State {
    /// External sync on (the Sync button): Studio follows incoming MTC.
    pub ext: bool,
    /// The control bar shows the Sync button (hidden by default, as Logic's).
    pub show_button: bool,
    mtc: MtcIn,
    /// The incoming MTC's frame rate (as Logic interprets it).
    pub mtc_rate: Option<Rate>,
    last_mtc: Option<Instant>,
    mtc_run: u32,
    cfg: Option<Arc<Mutex<Cfg>>>,
    last_playing: bool,
    last_recording: bool,
    last_pos: f64,
    last_click: f64,
    /// MMC locate stream while the playhead is dragged (time of the last one sent).
    last_locate: Option<Instant>,
}

/// Seconds from bar 1 to `tick` through tempo changes.
pub fn secs_at(map: &[(u32, f64)], tempo: f64, tick: f64) -> f64 {
    let (mut secs, mut at, mut bpm) = (0.0, 0.0, tempo);
    for &(t, b) in map {
        let t = t as f64;
        if t >= tick {
            break;
        }
        if t > at {
            secs += (t - at) / PPQ as f64 * 60.0 / bpm;
            at = t;
        }
        bpm = b;
    }
    secs + (tick - at).max(0.0) / PPQ as f64 * 60.0 / bpm
}

/// The tick at `secs` from bar 1 (the inverse of secs_at).
pub fn tick_at(map: &[(u32, f64)], tempo: f64, secs: f64) -> f64 {
    let (mut s, mut at, mut bpm) = (0.0, 0.0, tempo);
    for &(t, b) in map {
        let t = t as f64;
        if t > at {
            let seg = (t - at) / PPQ as f64 * 60.0 / bpm;
            if s + seg > secs {
                break;
            }
            s += seg;
            at = t;
        }
        bpm = b;
    }
    at + (secs - s).max(0.0) * bpm / 60.0 * PPQ as f64
}

fn rate_of(app: &App) -> Rate {
    let (_, fps, drop) = crate::projsettings::smpte(&app.proj);
    Rate { fps, drop }
}

/// The bar 1 SMPTE time in seconds.
fn offset(app: &App) -> f64 {
    crate::projsettings::smpte(&app.proj).0
}

fn port_of(v: &str) -> String {
    match v.trim() {
        "" | "Off" => String::new(),
        "All" => "*".into(),
        p => p.to_string(),
    }
}

fn mmc_port(app: &App) -> String {
    port_of(&app.proj.get("Synchronization / MIDI / MTC Destination:"))
}

fn mmc_id(app: &App) -> u8 {
    if app.logic_settings.get("MIDI / Sync / All") == "0" { app.logic_settings.get("MIDI / Sync / Output ID (Transport):").parse().unwrap_or(127) } else { 127 }
}

/// Song position in project seconds (bar 1 at its SMPTE time).
fn smpte_secs(app: &App, tick: f64) -> f64 {
    let (map, tempo) = app.project.as_ref().map_or((Vec::new(), 120.0), |p| (p.tempo_map.clone(), p.tempo));
    secs_at(&map, tempo, tick) + offset(app)
}

fn tick_of_smpte(app: &App, secs: f64) -> f64 {
    let (map, tempo) = app.project.as_ref().map_or((Vec::new(), 120.0), |p| (p.tempo_map.clone(), p.tempo));
    tick_at(&map, tempo, secs - offset(app))
}

/// Starts the timing thread (once): every millisecond it reads the transport and sends what
/// MIDI Clock and MTC need.
fn ensure_thread(app: &mut App) -> Arc<Mutex<Cfg>> {
    if let Some(c) = &app.hub.sync.cfg {
        return c.clone();
    }
    let cfg = Arc::new(Mutex::new(Cfg { alive: true, tempo: 120.0, ..Default::default() }));
    app.hub.sync.cfg = Some(cfg.clone());
    let engine = app.engine.clone();
    let outs = app.hub.outs.clone();
    let c2 = cfg.clone();
    std::thread::Builder::new().name("studio-sync".into()).spawn(move || {
        let mut clock = ClockOut::default();
        let mut mtc = MtcOut::default();
        let (mut last_pos, mut last_change) = (0.0f64, Instant::now());
        let mut was_playing = false;
        loop {
            std::thread::sleep(Duration::from_millis(1));
            let Ok(c) = c2.lock().map(|c| c.clone()) else { break };
            if !c.alive {
                break;
            }
            if c.clock_port.is_empty() && c.mtc_port.is_empty() {
                continue;
            }
            let (playing, pos, bpm) = match engine.lock() {
                Ok(e) => (e.playing, e.pos, e.tempo_map.iter().rev().find(|t| t.0 as f64 <= e.pos).map_or(e.tempo, |t| t.1)),
                Err(_) => break,
            };
            // (between audio blocks the position moves on with the tempo)
            if pos != last_pos || !playing || !was_playing {
                last_pos = pos;
                last_change = Instant::now();
            }
            was_playing = playing;
            let est = if playing { pos + last_change.elapsed().as_secs_f64() * bpm / 60.0 * PPQ as f64 } else { pos };
            // All MIDI Output Delay: positive values send later
            let est = est - c.delay_ms / 1000.0 * bpm / 60.0 * PPQ as f64;
            if !c.clock_port.is_empty() {
                for m in clock.poll(playing, est.max(0.0), false, c.song_mode) {
                    outs.send(&c.clock_port, &m);
                }
            }
            if let (false, Some(r)) = (c.mtc_port.is_empty(), c.rate) {
                let secs = secs_at(&c.map, c.tempo, est.max(0.0)) + c.offset;
                for m in mtc.poll(playing, secs, r) {
                    outs.send(&c.mtc_port, &m);
                }
            }
        }
    }).ok();
    cfg
}

/// Each frame: the thread's settings, MMC sent on transport changes, MTC drop-out, the MIDI
/// click object, Chase ▸ Notes.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    let p = &app.proj;
    let clock_port = if p.on("Synchronization / MIDI / Transmit MIDI Clock") { port_of(&p.get("Synchronization / MIDI / MIDI Clock Destination:")) } else { String::new() };
    let mtc_port = if p.on("Synchronization / MIDI / Transmit MTC") { port_of(&p.get("Synchronization / MIDI / MTC Destination:")) } else { String::new() };
    let song_mode = !p.get("Synchronization / MIDI / Clock Mode:").starts_with("Pattern");
    let chase = p.on("MIDI / Chase / Notes");
    if let Ok(mut e) = app.engine.lock() {
        e.chase_notes = chase;
    }
    if !clock_port.is_empty() || !mtc_port.is_empty() || app.hub.sync.cfg.is_some() {
        let rate = rate_of(app);
        let off = offset(app);
        let (tempo, map) = app.project.as_ref().map_or((120.0, Vec::new()), |p| (p.tempo, p.tempo_map.clone()));
        let delay_ms = app.logic_settings.get("MIDI / Sync / Delay:").parse().unwrap_or(0.0);
        let cfg = ensure_thread(app);
        if let Ok(mut c) = cfg.lock() {
            *c = Cfg { clock_port, song_mode, mtc_port, rate: Some(rate), offset: off, tempo, map, delay_ms, alive: true };
        }
    }
    let playing = app.playing();
    let recording = app.recording();
    let pos = app.position();
    // MMC out: transport changes and locates (Transmit MMC)
    if app.proj.on("Synchronization / MIDI / Transmit MIDI Machine Control (MMC)") {
        let port = mmc_port(app);
        let id = mmc_id(app);
        let r = rate_of(app);
        let send = |m: Mmc| app.hub.outs.send(if port.is_empty() { "*" } else { &port }, &sync::mmc(id, m, r));
        let s = &app.hub.sync;
        if playing != s.last_playing {
            if playing {
                send(Mmc::Locate(Tc::from_secs(smpte_secs(app, pos), r)));
                send(Mmc::Play);
            } else {
                send(Mmc::Stop);
            }
        }
        if recording != s.last_recording {
            send(if recording { Mmc::RecordStrobe } else { Mmc::RecordExit });
        }
        // (dragging the playhead: a stream of locates)
        if !playing && (pos - s.last_pos).abs() > 1.0 && s.last_locate.is_none_or(|t| t.elapsed() > Duration::from_millis(30)) {
            send(Mmc::Locate(Tc::from_secs(smpte_secs(app, pos), r)));
            app.hub.sync.last_locate = Some(Instant::now());
        }
    }
    // MTC stopped coming: following stops
    if app.hub.sync.ext && app.hub.sync.last_mtc.is_some_and(|t| t.elapsed() > Duration::from_millis(250)) {
        app.hub.sync.last_mtc = None;
        app.hub.sync.mtc_run = 0;
        if app.playing() {
            app.halt();
        }
    }
    // the MIDI click object: bar, beat and division clicks while playing with the metronome on
    let metronome = app.engine.lock().map(|e| e.metronome).unwrap_or(false);
    if playing && metronome {
        let div = PPQ as f64 / 4.0;
        let (a, b) = (app.hub.sync.last_click, pos);
        if b > a && b - a < PPQ as f64 * 4.0 {
            let next = (a / div).floor() * div + div;
            if next <= b {
                let n = (next / div).round() as u32;
                let bar_ticks = app.bar_ticks() as f64;
                let which = if (next / bar_ticks).fract().abs() < 1e-6 { 0 } else if n % 4 == 0 { 1 } else { 2 };
                crate::envwin::click(app, ctx, which);
            }
        }
    }
    // a cycle jump: arpeggiators start over
    if playing && app.cycle_on() && pos + 1.0 < app.hub.sync.last_click {
        app.hub.env.env.cycle_jump();
    }
    let s = &mut app.hub.sync;
    s.last_click = pos;
    s.last_playing = playing;
    s.last_recording = recording;
    s.last_pos = pos;
    if s.ext || playing {
        ctx.request_repaint_after(Duration::from_millis(10));
    }
}

/// MTC and MMC in. True when taken.
pub fn midi_in(app: &mut App, ctx: &egui::Context, _port: usize, bytes: &[u8]) -> bool {
    // Logic can't receive MIDI Clock; Start / Continue / Stop and Song Position are ignored too
    if matches!(bytes[0], 0xF2 | 0xFA | 0xFB | 0xFC) {
        return true;
    }
    if let Some(m) = sync::mmc_parse(bytes, Some(mmc_id(app))) {
        if app.proj.on("Synchronization / MIDI / Listen to MIDI Machine Control (MMC) input") {
            mmc_received(app, ctx, m);
        }
        return true;
    }
    if bytes[0] == 0xF1 || (bytes[0] == 0xF0 && bytes.get(1) == Some(&0x7F) && bytes.get(3) == Some(&0x01)) {
        if let Some((secs, r)) = app.hub.sync.mtc.feed(bytes) {
            // Frame Rate in the project settings overrides detection when Auto detect is off
            let auto = app.proj.on("Synchronization / General / Auto detect format of MTC");
            let rate = if auto { r } else { rate_of(app) };
            app.hub.sync.mtc_rate = Some(rate);
            mtc_received(app, secs, bytes[0] == 0xF1);
        }
        return true;
    }
    false
}

fn mtc_received(app: &mut App, secs: f64, running: bool) {
    let mode_mtc = app.proj.get("Synchronization / General / Sync Mode:") == "MTC";
    // Auto enable External Sync: incoming time code turns sync on
    if mode_mtc && app.proj.on("Synchronization / General / Auto enable External Sync") {
        app.hub.sync.ext = true;
    }
    if !(app.hub.sync.ext && mode_mtc) {
        return;
    }
    let tick = tick_of_smpte(app, secs);
    app.hub.sync.last_mtc = if running { Some(Instant::now()) } else { app.hub.sync.last_mtc };
    if running {
        app.hub.sync.mtc_run += 1;
    }
    let pos = app.position();
    let tps = app.tempo() / 60.0 * PPQ as f64;
    // (more than two frames off: locate; MTC Continuous keeps following)
    if (pos - tick).abs() > tps * 2.0 / rate_of(app).fps {
        app.locate(tick);
    }
    // a few quarter frames in a row: play along
    if running && app.hub.sync.mtc_run >= 2 && !app.playing() {
        if let Ok(mut e) = app.engine.lock() {
            e.playing = true;
        }
    }
}

fn mmc_received(app: &mut App, ctx: &egui::Context, m: Mmc) {
    match m {
        Mmc::Play | Mmc::DeferredPlay => {
            if !app.playing() {
                app.play_stop();
            }
        }
        Mmc::Stop | Mmc::Pause => app.halt(),
        Mmc::Locate(tc) => {
            let t = tick_of_smpte(app, tc.to_secs(rate_of(app)));
            app.locate(t);
        }
        Mmc::RecordStrobe => {
            if !app.recording() {
                let _ = app.key_command(ctx, "Record");
            }
        }
        Mmc::RecordExit => {
            if app.recording() {
                app.halt();
            }
        }
        Mmc::FastForward => {
            let b = app.bar_ticks() as f64;
            let p = app.position();
            app.locate(p + b);
        }
        Mmc::Rewind => {
            let b = app.bar_ticks() as f64;
            let p = app.position();
            app.locate((p - b).max(0.0));
        }
        Mmc::Arm(_) => {}
    }
}

/// An MMC record-enable for tape tracks (MMC Record Buttons object).
pub fn send_arm(app: &mut App, mask: u64) {
    let port = mmc_port(app);
    let r = rate_of(app);
    app.hub.outs.send(if port.is_empty() { "*" } else { &port }, &sync::mmc(mmc_id(app), Mmc::Arm(mask), r));
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    match name {
        "Toggle Sync intern/extern" => {
            let s = &mut app.hub.sync;
            s.ext = !s.ext;
            let mode = app.proj.get("Synchronization / General / Sync Mode:");
            let msg = if !app.hub.sync.ext {
                "Sync: Internal (Studio is the transmitter)".to_string()
            } else if mode == "MTC" {
                "Sync: following incoming MTC".to_string()
            } else {
                format!("Sync on; the Sync Mode is {mode} (only MTC is received by Studio)")
            };
            app.tell(msg);
        }
        // File ▸ Project Settings ▸ Synchronization…
        "Synchronization…" => {
            let _ = ctx;
            app.pf.settings = Some("Synchronization".into());
        }
        _ => return false,
    }
    true
}

/// The control bar's Sync button: lit while sync is received; a click turns external sync on
/// or off (the source stays chosen); Control-click: the sync options.
pub fn sync_button(app: &mut App, ui: &mut egui::Ui, r: egui::Rect) {
    let on = app.hub.sync.ext;
    let receiving = app.hub.sync.last_mtc.is_some_and(|t| t.elapsed() < Duration::from_millis(250));
    let resp = ui.interact(r, ui.scope_id().with("cb-sync"), egui::Sense::click()).on_hover_text("Sync");
    let p = ui.painter();
    let cell = r.shrink2(egui::vec2(1.0, 2.0));
    if on {
        p.rect_filled(cell, cell.height() / 2.0, if receiving { crate::look::accent() } else { crate::look::accent().gamma_multiply(0.55) });
    } else if resp.hovered() {
        p.rect_filled(cell, cell.height() / 2.0, crate::look::button_on().gamma_multiply(0.55));
    }
    crate::glyphs::paint(p, r, crate::glyphs::ph::ARROWS_CLOCKWISE, 15.0, if on { egui::Color32::WHITE } else { crate::look::text() });
    if resp.clicked() {
        let ctx = ui.ctx().clone();
        let _ = command(app, &ctx, "Toggle Sync intern/extern");
    }
    resp.context_menu(|ui| {
        let mode = app.proj.get("Synchronization / General / Sync Mode:");
        for (m, label, ok) in [("Internal", "Internal Sync", true), ("Ableton Link", "Ableton Link", false), ("MTC", "SMPTE Sync (MTC)", true), ("Manual", "Manual Sync (Tempo Interpreter)", false)] {
            if ui.add_enabled(ok, egui::Button::new(if mode == m { format!("✓ {label}") } else { label.to_string() })).clicked() {
                app.proj.0.insert("Synchronization / General / Sync Mode:".into(), m.into());
                app.hub.sync.ext = m != "Internal";
                ui.close();
            }
        }
        ui.separator();
        let mut auto = app.proj.on("Synchronization / General / Auto enable External Sync");
        if ui.checkbox(&mut auto, "Auto Sync In").changed() {
            app.proj.0.insert("Synchronization / General / Auto enable External Sync".into(), (auto as u8).to_string());
        }
        let mut mmc = app.proj.on("Synchronization / MIDI / Transmit MIDI Machine Control (MMC)");
        if ui.checkbox(&mut mmc, "MIDI Machine Control (MMC)").changed() {
            app.proj.0.insert("Synchronization / MIDI / Transmit MIDI Machine Control (MMC)".into(), (mmc as u8).to_string());
        }
        ui.separator();
        if ui.button("Synchronization Settings…").clicked() {
            app.pf.settings = Some("Synchronization".into());
            ui.close();
        }
        if ui.button("Open Tempo List…").clicked() {
            let ctx = ui.ctx().clone();
            let _ = app.key_command(&ctx, "Open Tempo List…");
            ui.close();
        }
    });
}

pub fn checked(app: &App, label: &str) -> Option<bool> {
    match label {
        "Toggle Sync intern/extern" => Some(app.hub.sync.ext),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_and_ticks() {
        let map = vec![(0, 120.0), (3840, 60.0)];
        // one bar at 120: 2 s; then 960 ticks at 60: 1 s
        assert!((secs_at(&map, 120.0, 4800.0) - 3.0).abs() < 1e-9);
        assert!((tick_at(&map, 120.0, 3.0) - 4800.0).abs() < 1e-6);
        assert!((tick_at(&[], 120.0, 1.0) - 1920.0).abs() < 1e-6);
    }
}
