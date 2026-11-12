//! Synchronization (lg_core::sync does the protocols): Studio sends MIDI Clock with Song
//! Position Pointer and MIDI Time Code from a timing thread, sends and listens to MIDI Machine
//! Control, and follows incoming MTC when the Sync Mode is MTC and external sync is on (the
//! control bar's Sync, Toggle Sync intern/extern). Project Settings ▸ Synchronization holds the
//! choices; Project Settings ▸ MIDI ▸ Chase ▸ Notes makes notes already under way sound from a
//! play start or locate.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lg_core::events::PPQ;
use lg_core::sync::{self, ClockMode, ClockOut, Mmc, MtcIn, MtcOut, Rate, Tc};

use crate::app::App;

pub const COMMANDS: &[&str] = &["Toggle Sync intern/extern", "Synchronization…"];

/// What the timing thread sends, refreshed each frame.
#[derive(Clone, Default)]
struct Cfg {
    /// The destinations that get MIDI Clock or MTC (Synchronization ▸ MIDI's rows).
    dests: Vec<Dest>,
    mode: ClockMode,
    /// Clock Start (ticks) and the pattern length (ticks) for Pattern mode.
    start_at: f64,
    pattern: f64,
    /// The mix's plug-in delay (seconds), for rows with PDC ticked.
    pdc: f64,
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

/// One row of Synchronization ▸ MIDI: a destination and what it gets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dest {
    /// The port ("*" for All).
    pub port: String,
    pub clock: bool,
    /// Clock delay (ms; negative: earlier).
    pub delay_ms: f64,
    pub pdc: bool,
    pub mtc: bool,
    pub mmc: bool,
}

/// Logic's ten rows.
pub const ROWS: usize = 10;
const K: &str = "Synchronization / MIDI / ";

fn row_key(n: usize, col: &str) -> String {
    format!("{K}{col} {}", n + 1)
}

/// A row's value (row 1 goes to All with MMC ticked unless changed; older projects' single
/// Transmit MIDI Clock / MTC boxes become row 1's).
fn row_get(p: &crate::projsettings::Values, n: usize, col: &str) -> String {
    if let Some(v) = p.0.get(&row_key(n, col)) {
        return v.clone();
    }
    match (n, col) {
        (0, "Destination") => {
            let legacy = if p.on(&format!("{K}Transmit MIDI Clock")) { p.get(&format!("{K}MIDI Clock Destination:")) } else { p.get(&format!("{K}MTC Destination:")) };
            if legacy.is_empty() { "All".into() } else { legacy }
        }
        (0, "Clock") => p.get(&format!("{K}Transmit MIDI Clock")),
        (0, "MTC") => p.get(&format!("{K}Transmit MTC")),
        (0, "MMC") => "1".into(),
        (_, "Destination") => "Off".into(),
        (_, "Delay") => "0".into(),
        _ => "0".into(),
    }
}

/// The rows in use (destination not Off).
pub fn dests(p: &crate::projsettings::Values) -> Vec<Dest> {
    (0..ROWS)
        .filter_map(|n| {
            let port = port_of(&row_get(p, n, "Destination"));
            if port.is_empty() {
                return None;
            }
            let on = |c: &str| row_get(p, n, c) == "1";
            Some(Dest { port, clock: on("Clock"), delay_ms: row_get(p, n, "Delay").parse().unwrap_or(0.0), pdc: on("PDC"), mtc: on("MTC"), mmc: on("MMC") })
        })
        .collect()
}

/// Clock Start and the pattern length, in ticks (from "bar beat div tick" and bars).
fn clock_start(app: &App) -> (f64, f64) {
    let bar = app.bar_ticks() as f64;
    let pos = app.proj.0.get(&format!("{K}Clock Start:")).cloned().unwrap_or_else(|| "1 1 1 1".into());
    let f: Vec<f64> = pos.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    let beat = PPQ as f64 * 4.0 / app.project.as_ref().map_or(4.0, |p| p.signature.1 as f64);
    let start = (f.first().copied().unwrap_or(1.0) - 1.0).max(0.0) * bar + (f.get(1).copied().unwrap_or(1.0) - 1.0).max(0.0) * beat + (f.get(2).copied().unwrap_or(1.0) - 1.0).max(0.0) * beat / 4.0 + (f.get(3).copied().unwrap_or(1.0) - 1.0).max(0.0);
    let bars: f64 = app.proj.0.get(&format!("{K}Pattern Length:")).and_then(|v| v.parse().ok()).unwrap_or(1.0);
    (start, bars.max(1.0) * bar)
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
    let (_, fps, drop) = crate::projsettings::smpte_real(&app.proj);
    Rate { fps, drop }
}

/// The bar 1 SMPTE time in seconds.
fn offset(app: &App) -> f64 {
    crate::projsettings::smpte_real(&app.proj).0
}

fn port_of(v: &str) -> String {
    match v.trim() {
        "" | "Off" => String::new(),
        "All" => "*".into(),
        p => p.to_string(),
    }
}

/// Where MMC goes: the rows with MMC ticked.
fn mmc_ports(app: &App) -> Vec<String> {
    dests(&app.proj).into_iter().filter(|d| d.mmc).map(|d| d.port).collect()
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
        let mut clocks: Vec<(String, ClockOut, MtcOut)> = Vec::new();
        let (mut last_pos, mut last_change) = (0.0f64, Instant::now());
        let mut was_playing = false;
        loop {
            std::thread::sleep(Duration::from_millis(1));
            let Ok(c) = c2.lock().map(|c| c.clone()) else { break };
            if !c.alive {
                break;
            }
            if c.dests.is_empty() {
                clocks.clear();
                continue;
            }
            // (each destination keeps its own clock and time code state)
            if clocks.len() != c.dests.len() || clocks.iter().zip(&c.dests).any(|(a, d)| a.0 != d.port) {
                clocks = c.dests.iter().map(|d| (d.port.clone(), ClockOut::default(), MtcOut::default())).collect();
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
            let tps = bpm / 60.0 * PPQ as f64;
            for (d, (_, clock, mtc)) in c.dests.iter().zip(clocks.iter_mut()) {
                // the MIDI settings' Delay, the row's Delay (positive: later) and PDC
                let late = c.delay_ms / 1000.0 + d.delay_ms / 1000.0 + if d.pdc { c.pdc } else { 0.0 };
                let est = (est - late * tps).max(0.0);
                if d.clock {
                    for m in clock.poll_mode(playing, est, c.mode, c.start_at, c.pattern) {
                        outs.send(&d.port, &m);
                    }
                }
                if let (true, Some(r)) = (d.mtc, c.rate) {
                    let secs = secs_at(&c.map, c.tempo, est) + c.offset;
                    for m in mtc.poll(playing, secs, r) {
                        outs.send(&d.port, &m);
                    }
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
    let dests: Vec<Dest> = dests(p).into_iter().filter(|d| d.clock || d.mtc).collect();
    let mode = ClockMode::parse(&p.get("Synchronization / MIDI / Clock Mode:"));
    let chase = p.on("MIDI / Chase / Notes");
    let cfg = crate::chase::Cfg::of(p);
    let no_transpose: Vec<bool> = (0..app.project.as_ref().map_or(0, |p| p.tracks.len())).map(|t| app.track_props.get(&t).is_some_and(|tp| tp.no_transpose)).collect();
    if let Ok(mut e) = app.engine.lock() {
        e.chase_notes = chase;
        e.chase.cfg = cfg;
        e.chase.no_transpose = no_transpose;
    }
    if !dests.is_empty() || app.hub.sync.cfg.is_some() {
        let rate = rate_of(app);
        let off = offset(app);
        let (tempo, map) = app.project.as_ref().map_or((120.0, Vec::new()), |p| (p.tempo, p.tempo_map.clone()));
        let delay_ms = app.logic_settings.get("MIDI / Sync / Delay:").parse().unwrap_or(0.0);
        let (start_at, pattern) = clock_start(app);
        let pdc = app.engine.lock().map(|e| e.latency() as f64 / crate::audio::RATE as f64).unwrap_or(0.0);
        let thread = ensure_thread(app);
        if let Ok(mut c) = thread.lock() {
            *c = Cfg { dests, mode, start_at, pattern, pdc, rate: Some(rate), offset: off, tempo, map, delay_ms, alive: true };
        }
    }
    let playing = app.playing();
    let recording = app.recording();
    let pos = app.position();
    // Chase ▸ Send full MIDI reset before chasing: the Reset Messages when playback stops
    if !playing && app.hub.sync.last_playing && cfg.reset_before {
        crate::prefs::send_reset(app);
    }
    // MMC out: transport changes and locates (Transmit MMC)
    if app.proj.on("Synchronization / MIDI / Transmit MIDI Machine Control (MMC)") {
        let ports = mmc_ports(app);
        let id = mmc_id(app);
        let r = rate_of(app);
        let send = |m: Mmc| {
            for port in &ports {
                app.hub.outs.send(port, &sync::mmc(id, m, r));
            }
        };
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
                // (compound meters: beats grouped in threes of eighths)
                let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
                let in_bar = next - (next / bar_ticks).floor() * bar_ticks;
                let group = sig.1 >= 8 && sig.0 > 3 && sig.0 % 3 == 0 && (in_bar / (3.0 * PPQ as f64 * 4.0 / sig.1 as f64)).fract().abs() < 1e-6;
                crate::metroset::midi_click(app, which, group);
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
        // (ignored while external MTC is coming in)
        let mtc_live = app.hub.sync.last_mtc.is_some_and(|t| t.elapsed() < Duration::from_millis(250));
        if app.proj.on("Synchronization / MIDI / Listen to MIDI Machine Control (MMC) input") && !mtc_live {
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
            let full = bytes[0] == 0xF0;
            // a Full Frame Message moves the playhead (without playing) when listening to MMC
            let following = app.hub.sync.ext && app.proj.get("Synchronization / General / Sync Mode:") == "MTC";
            if full && !following && app.proj.on("Synchronization / MIDI / Listen to MIDI Machine Control (MMC) input") {
                let t = tick_of_smpte(app, secs);
                app.locate(t);
                return true;
            }
            mtc_received(app, secs, !full);
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
    let r = rate_of(app);
    for port in mmc_ports(app) {
        app.hub.outs.send(&port, &sync::mmc(mmc_id(app), Mmc::Arm(mask), r));
    }
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

/// Project Settings ▸ Synchronization ▸ MIDI, as Logic's: the ten destination rows (Clock,
/// Delay, PDC, MTC, MMC), Clock Mode, Clock Start with the pattern length, the MMC boxes and the
/// MIDI Sync Settings button.
pub fn pane(app: &mut App, ui: &mut egui::Ui) {
    let mut choices = vec!["Off".to_string(), "All".to_string()];
    choices.extend(app.hub.outs.ports());
    let set = |v: &mut crate::projsettings::Values, k: String, val: String| {
        v.0.insert(k, val);
    };
    egui::Grid::new("sync-midi-rows").num_columns(6).spacing([10.0, 3.0]).striped(true).show(ui, |ui| {
        for h in ["Destination", "Clock", "Delay [ms]", "PDC", "MTC", "MMC"] {
            ui.label(egui::RichText::new(h).small().strong());
        }
        ui.end_row();
        for n in 0..ROWS {
            let cur = row_get(&app.proj, n, "Destination");
            egui::ComboBox::from_id_salt(("sync-dest", n)).selected_text(&cur).width(210.0).show_ui(ui, |ui| {
                for c in &choices {
                    if ui.selectable_label(cur == *c, c).clicked() {
                        set(&mut app.proj, row_key(n, "Destination"), c.clone());
                    }
                }
            });
            let used = cur != "Off";
            for col in ["Clock", "Delay", "PDC", "MTC", "MMC"] {
                if col == "Delay" {
                    let mut d: f64 = row_get(&app.proj, n, "Delay").parse().unwrap_or(0.0);
                    if ui.add_enabled(used, egui::DragValue::new(&mut d).range(-500.0..=500.0).speed(0.2).fixed_decimals(1)).changed() {
                        set(&mut app.proj, row_key(n, "Delay"), format!("{d:.1}"));
                    }
                    continue;
                }
                let mut on = row_get(&app.proj, n, col) == "1";
                // (MMC goes out only with Transmit MMC on)
                let enabled = used && (col != "MMC" || app.proj.on(&format!("{K}Transmit MIDI Machine Control (MMC)")));
                if ui.add_enabled(enabled, egui::Checkbox::without_text(&mut on)).changed() {
                    set(&mut app.proj, row_key(n, col), (on as u8).to_string());
                }
            }
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    egui::Grid::new("sync-midi-clock").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label("Clock Mode:");
        let cur = ClockMode::parse(&app.proj.get(&format!("{K}Clock Mode:")));
        let names = ClockMode::NAMES;
        egui::ComboBox::from_id_salt("sync-clock-mode").selected_text(names[cur as usize]).width(420.0).show_ui(ui, |ui| {
            for (k, nm) in names.iter().enumerate() {
                if ui.selectable_label(cur as usize == k, *nm).clicked() {
                    crate::projsettings::set(&mut app.proj, &format!("{K}Clock Mode:"), nm);
                }
            }
        });
        ui.end_row();
        ui.label("Clock Start:");
        ui.horizontal(|ui| {
            let mut pos = app.proj.0.get(&format!("{K}Clock Start:")).cloned().unwrap_or_else(|| "1 1 1 1".into());
            ui.label("at");
            if ui.add(egui::TextEdit::singleline(&mut pos).desired_width(80.0)).changed() {
                app.proj.0.insert(format!("{K}Clock Start:"), pos);
            }
            ui.label("with Pattern Length of");
            let mut bars: u32 = app.proj.0.get(&format!("{K}Pattern Length:")).and_then(|v| v.parse().ok()).unwrap_or(1);
            if ui.add(egui::DragValue::new(&mut bars).range(1..=999)).changed() {
                app.proj.0.insert(format!("{K}Pattern Length:"), bars.to_string());
            }
            ui.label("bars");
        });
        ui.end_row();
    });
    ui.add_space(4.0);
    for l in ["Transmit MIDI Machine Control (MMC)", "Listen to MIDI Machine Control (MMC) input"] {
        let k = format!("{K}{l}");
        let mut on = app.proj.on(&k);
        if ui.checkbox(&mut on, l).changed() {
            crate::projsettings::set(&mut app.proj, &k, if on { "1" } else { "0" });
        }
    }
    ui.add_space(4.0);
    if ui.button("MIDI Sync Settings…").clicked() {
        app.settings_tab = "MIDI".into();
        app.settings_sub = "Sync".into();
        app.show_settings = true;
    }
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Bramble Studio Virtual Out reaches other apps. Clock and MTC go out while playing; MMC on play, stop, record and locate; incoming MMC (Play, Deferred Play, Stop, Locate) is ignored while external MTC comes in.").small().color(crate::look::text_dim()));
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
