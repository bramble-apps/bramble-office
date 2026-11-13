//! What the Settings window's rows do (settings.rs draws them and keeps their values): each row
//! Studio has a counterpart for is read where that behaviour lives, through the helpers here, or
//! pushed into the app's state when it changes (`changed`). Rows whose state lives in the app as
//! well (the Record menu's overlap choices) read and write it there (`live_get` / `live_set`).
//!
//! Wired: Project Handling (Startup Action, Default Template, Auto Backup, Recent Items with
//! File ▸ Open Recent), Editing (Undo Steps, Double-Clicking a MIDI Region Opens, Select regions
//! on track selection, Select tracks on region selection), Catch, Audio ▸ Devices (Enabled, Output
//! and Input Device, I/O Buffer Size with the resulting latency, Apply), Recording (Bit Depth,
//! the overlapping-recording choices, Replace, the Project Settings button), MIDI (Reset All MIDI
//! Drivers, Reset Messages on Stop twice and Send Reset Controllers, Inputs on/off and the virtual
//! input), View (help tags, wide playhead, animations, middle C), Mixer (Open plug-in window on
//! insertion), Editors (Bright background) and the buttons that open Project Settings panes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use crate::app::App;
use crate::recording::{Overlap, ReplaceMode};
use crate::settings::Values;

pub const COMMANDS: &[&str] = &["Send Reset Controllers", "Clear Menu", "Reset All Settings Except Key Command Assignments…"];

// ---- keys (Tab / Sub / row) ------------------------------------------------------------------
pub const STARTUP: &str = "General / Project Handling / Startup Action:";
pub const DEFAULT_TEMPLATE: &str = "General / Project Handling / Default Template:";
pub const AUTO_BACKUP: &str = "General / Project Handling / Auto Backup:";
pub const RECENT_ITEMS: &str = "General / Project Handling / Recent Items:";
pub const DOUBLE_CLICK_MIDI: &str = "General / Editing / Double-Clicking a MIDI Region Opens:";
pub const SELECT_REGIONS_ON_TRACK: &str = "General / Editing / Select regions on track selection";
pub const SELECT_TRACKS_ON_REGION: &str = "General / Editing / Select tracks on region/marquee selection";
pub const CATCH_START: &str = "General / Catch / Catch when starting playback";
pub const CATCH_MOVE: &str = "General / Catch / Catch when moving playhead";
pub const AUDIO_ENABLED: &str = "Audio / Devices / Enabled";
pub const OUTPUT_DEVICE: &str = "Audio / Devices / Output Device:";
pub const INPUT_DEVICE: &str = "Audio / Devices / Input Device:";
pub const BUFFER: &str = "Audio / Devices / I/O Buffer Size:";
pub const REC_FILE_TYPE: &str = "Recording /  / File Type:";
pub const REC_BIT_DEPTH: &str = "Recording /  / Bit Depth:";
pub const REC_MIDI_OFF: &str = "Recording /  / Cycle Off:";
pub const REC_AUDIO_OFF: &str = "Recording /  / Cycle Off: Audio";
pub const REC_MIDI_ON: &str = "Recording /  / Cycle On:";
pub const REC_AUDIO_ON: &str = "Recording /  / Cycle On: Audio";
pub const REC_REPLACE: &str = "Recording /  / Replace:";
pub const VIRTUAL_IN: &str = "MIDI / Inputs / Logic Pro Virtual In";
pub const WIDE_PLAYHEAD: &str = "View / General / Wide playhead";
pub const ANIMATIONS: &str = "View / General / Show animations";
pub const OPEN_ON_INSERT: &str = "View / Mixer / Open plug-in window on insertion";
pub const BRIGHT_EDITORS: &str = "View / Editors / Bright background";
/// Linked to the control bar's Low Latency button; lasts until Studio quits (not saved).
pub const LOW_LATENCY: &str = "Audio / General / Low Latency Monitoring Mode";
/// Plug-in Latency Compensation: All (every strip, the engine's compensation on) or Audio and
/// Software Instrument Tracks / Off (no compensation across the busses).
pub const COMPENSATION: &str = "Audio / General / Compensation:";

/// The key of a MIDI input port's On checkbox in Settings ▸ MIDI ▸ Inputs.
pub fn input_key(port: &str) -> String {
    format!("MIDI / Inputs / port: {port}")
}

// ---- what drawing code reads without the app ---------------------------------------------------
static WIDE: AtomicBool = AtomicBool::new(false);
static BRIGHT: AtomicBool = AtomicBool::new(false);
static INPUTS_OFF: RwLock<Option<HashSet<String>>> = RwLock::new(None);

/// The playhead's line width (View ▸ Wide playhead).
pub fn playhead_width() -> f32 {
    if WIDE.load(Ordering::Relaxed) { 3.0 } else { 1.2 }
}

/// View ▸ Editors ▸ Bright background (the Piano Roll's).
pub fn bright_editors() -> bool {
    BRIGHT.load(Ordering::Relaxed)
}

/// An input port switched off in Settings ▸ MIDI ▸ Inputs (not opened, so not recorded from).
pub fn input_off(port: &str) -> bool {
    INPUTS_OFF.read().ok().is_some_and(|s| s.as_ref().is_some_and(|s| s.contains(port)))
}

/// The virtual input other apps can play into (Logic Pro Virtual In: Bramble Studio's).
pub fn virtual_in(v: &Values) -> bool {
    v.on(VIRTUAL_IN)
}

fn sync_statics(v: &Values) {
    WIDE.store(v.on(WIDE_PLAYHEAD), Ordering::Relaxed);
    BRIGHT.store(v.on(BRIGHT_EDITORS), Ordering::Relaxed);
    let off: HashSet<String> = v.0.iter().filter(|(k, x)| k.starts_with("MIDI / Inputs / port: ") && *x == "0").map(|(k, _)| k["MIDI / Inputs / port: ".len()..].to_string()).collect();
    if let Ok(mut s) = INPUTS_OFF.write() {
        *s = Some(off);
    }
    // Audio ▸ Sampler: where a dropped sample's root key comes from
    let src = ["File/Analysis", "Filename/Analysis", "Filename Only", "File Only", "Analysis Only"].iter().position(|x| *x == v.get("Audio / Sampler / Read Root Key From:")).unwrap_or(0);
    lg_synth::sampler_doc::ROOT_SOURCE.store(src as u8, Ordering::Relaxed);
    lg_synth::sampler_doc::ROOT_NAME_POS.store(v.get("Audio / Sampler / Root Key at File Name Position:").parse::<u8>().unwrap_or(0), Ordering::Relaxed);
}

// ---- Project Handling ---------------------------------------------------------------------------

/// Auto Backup: how many backups a save keeps (None: no backups).
pub fn backups(v: &Values) -> Option<usize> {
    let s = v.get(AUTO_BACKUP);
    match s.as_str() {
        "None" => None,
        "Last Alternative Version" => Some(1),
        _ => Some(s.split_whitespace().find_map(|w| w.parse().ok()).unwrap_or(10)),
    }
}

/// Recent Items: how many projects File ▸ Open Recent lists (System Default: 10).
pub fn recent_items(v: &Values) -> usize {
    v.get(RECENT_ITEMS).parse().unwrap_or(10)
}

fn recent_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("recent-projects.txt"))
}

/// The projects opened, newest first (those still on disk).
pub fn recent() -> Vec<PathBuf> {
    recent_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| t.lines().map(PathBuf::from).filter(|p| p.exists()).collect()).unwrap_or_default()
}

/// A project opened or saved: first in Open Recent.
pub fn note_opened(path: &Path) {
    if std::env::var_os("LG_SNAP").is_some() {
        return;
    }
    let Some(f) = recent_path() else { return };
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut list = recent();
    list.retain(|p| *p != path);
    list.insert(0, path);
    list.truncate(50);
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, list.iter().map(|p| format!("{}\n", p.display())).collect::<String>());
}

/// File ▸ Open Recent ▸ (the projects, Clear Menu).
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    if it.label != "Open Recent" {
        return false;
    }
    let list: Vec<PathBuf> = recent().into_iter().take(recent_items(&app.logic_settings)).collect();
    ui.menu_button("Open Recent", |ui| {
        ui.set_min_width(240.0);
        for p in &list {
            let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if ui.button(name).on_hover_text(p.display().to_string()).clicked() {
                app.open_path(p);
                ui.close();
            }
        }
        if !list.is_empty() {
            ui.separator();
        }
        if ui.add_enabled(!list.is_empty(), egui::Button::new("Clear Menu")).clicked() {
            if let Some(f) = recent_path() {
                let _ = std::fs::remove_file(f);
            }
            ui.close();
        }
    });
    true
}

/// At launch with no project named: Settings ▸ General ▸ Project Handling ▸ Startup Action.
/// (Snapshots and tests keep the demo song.)
pub fn startup(app: &mut App) {
    sync_statics(&app.logic_settings);
    if app.automated {
        return;
    }
    match app.logic_settings.get(STARTUP).as_str() {
        "Open Most Recent Project" => {
            if let Some(p) = recent().into_iter().next() {
                app.open_path(&p);
            }
        }
        "Open Existing Project" => app.open_dialog(),
        "Create New Empty Project" => {
            app.set_project(crate::demo::empty(), true);
            app.file = None;
            app.new_track_open = true;
        }
        "Create New Project Using Default Template" => {
            let k = default_template(&app.logic_settings);
            app.set_project(crate::projfile::layout_project(k), true);
            app.file = None;
        }
        "Select a Template" | "Ask" => app.pf.chooser = true,
        // Do Nothing: an empty project, no dialog
        "Do Nothing" => {
            app.set_project(crate::demo::empty(), true);
            app.file = None;
        }
        _ => {}
    }
}

/// The Default Template (one of Studio's templates; the first: Empty Project).
pub fn default_template(v: &Values) -> usize {
    let n = v.get(DEFAULT_TEMPLATE);
    crate::projfile::LAYOUTS.iter().position(|l| l.0 == n).unwrap_or(0)
}

// ---- Editing ----------------------------------------------------------------------------------

/// A track header clicked (select_regions: Option held): with Select regions on track
/// selection, the track's regions are selected too (only those in the cycle area when Cycle is
/// on), as when Option-clicking.
pub fn track_clicked(app: &mut App, t: usize, option: bool) {
    if !option && !app.logic_settings.on(SELECT_REGIONS_ON_TRACK) {
        return;
    }
    let cycle = app.cycle_on().then_some(app.cycle);
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return };
    let regs: Vec<(usize, usize)> = tr.regions.iter().enumerate().filter(|(_, r)| cycle.is_none_or(|(a, b)| r.start < b && r.start + r.length > a)).map(|(k, _)| (t, k)).collect();
    app.selected_regions = regs;
    app.selected_region = app.selected_regions.first().copied();
}

/// Select tracks on region/marquee selection: whether selecting a region selects its track.
pub fn region_selects_track(app: &App) -> bool {
    app.logic_settings.on(SELECT_TRACKS_ON_REGION)
}

/// A MIDI region double-clicked: the editor Double-Clicking a MIDI Region Opens names.
pub fn open_midi_region(app: &mut App, ctx: &egui::Context) {
    match app.logic_settings.get(DOUBLE_CLICK_MIDI).as_str() {
        "Score Editor" => {
            app.edit_command(ctx, "Open Score Editor…");
        }
        "Event List" => {
            app.right_pane = Some(crate::app::RightPane::Lists);
            app.list_tab = 0;
        }
        _ => {
            app.edit_command(ctx, "Open Piano Roll…");
        }
    }
}

// ---- Catch -------------------------------------------------------------------------------------

/// Catch when starting playback: playback starts with Catch on (the view follows again).
pub fn catch_on_start(app: &App) -> bool {
    app.logic_settings.on(CATCH_START)
}

/// Catch when moving playhead: locating turns Catch back on.
pub fn catch_on_move(app: &App) -> bool {
    app.logic_settings.on(CATCH_MOVE)
}

// ---- Audio devices ------------------------------------------------------------------------------

/// The system's audio devices (output or input), "System Default" first.
pub fn devices(output: bool) -> Vec<String> {
    let mut v = vec!["System Default".to_string()];
    if std::env::var_os("LG_SNAP").is_some() {
        return v;
    }
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    let host = rodio::cpal::default_host();
    let list = if output { host.output_devices().map(|d| d.collect::<Vec<_>>()) } else { host.input_devices().map(|d| d.collect::<Vec<_>>()) };
    for d in list.unwrap_or_default() {
        if let Ok(n) = d.description().map(|x| x.name().to_string()) {
            if !v.contains(&n) {
                v.push(n);
            }
        }
    }
    v
}

/// The device settings the running output was opened with.
static OPENED: RwLock<Option<(String, String, bool)>> = RwLock::new(None);

fn device_settings(v: &Values) -> (String, String, bool) {
    (v.get(OUTPUT_DEVICE), v.get(BUFFER), v.on(AUDIO_ENABLED))
}

/// Apply is on when the device, buffer size or Enabled changed since the output opened.
pub fn apply_pending(v: &Values) -> bool {
    OPENED.read().ok().and_then(|o| o.clone()).is_some_and(|o| o != device_settings(v))
}

/// Opens the output as the Devices pane says (at launch, and by Apply): off (Enabled unticked)
/// leaves Studio silent, MIDI still runs.
pub fn open_output(app: &mut App) {
    let (dev, buf, on) = device_settings(&app.logic_settings);
    if let Ok(mut o) = OPENED.write() {
        *o = Some((dev.clone(), buf.clone(), on));
    }
    let engine = app.output.engine.clone();
    // (the old stream closes first: one device at a time)
    app.output = crate::audio::Output::silent(engine.clone());
    if !on {
        return;
    }
    let dev = (dev != "System Default").then_some(dev);
    let buf: Option<u32> = buf.parse().ok().filter(|_| dev.is_some() || app.logic_settings.0.contains_key(BUFFER));
    app.output = crate::audio::Output::open_on(engine, dev.as_deref(), buf);
    if let Some(e) = app.output.error.clone() {
        app.tell(format!("Audio output: {e}"));
    }
}

/// The latency text under the buffer size: output and roundtrip at Studio's rate.
pub fn latency_text(v: &Values) -> String {
    let n: f64 = v.get(BUFFER).parse().unwrap_or(128.0);
    let ms = n / crate::audio::RATE as f64 * 1000.0;
    format!("{:.1} ms Roundtrip ({:.1} ms Output)", ms * 2.0 + 64.0 / crate::audio::RATE as f64 * 2000.0, ms + 64.0 / crate::audio::RATE as f64 * 1000.0)
}

// ---- View ▸ General: the time and position displays ---------------------------------------------

pub const DISPLAY_TIME: &str = "View / General / Display Time As:";
pub const ZEROS: &str = "View / General / Zeros as spaces";
pub const CLOCK: &str = "View / General / Clock Format:";

/// A time as the control bar shows it: Display Time As, with the project's frame rate and bar 1
/// SMPTE time (Project Settings ▸ Synchronization) and Zeros as spaces.
pub fn time_text(v: &Values, proj: &crate::projsettings::Values, secs: f64) -> String {
    let (off, fps, drop) = crate::projsettings::smpte(proj);
    let t = (secs + off).max(0.0);
    let nf = fps.round().max(1.0);
    let frames_total = t * fps;
    let (h, m, s) = ((t / 3600.0).floor(), ((t % 3600.0) / 60.0).floor(), (t % 60.0).floor());
    let ff = (frames_total % nf).floor().min(nf - 1.0);
    let sub = ((frames_total.fract()) * 80.0).floor();
    let sep = if drop { ';' } else { ':' };
    let out = match v.get(DISPLAY_TIME).as_str() {
        "SMPTE/EBU without Subframes" => format!("{h:02.0}:{m:02.0}:{s:02.0}{sep}{ff:02.0}"),
        "SMPTE/EBU with Bits" => format!("{h:02.0}:{m:02.0}:{s:02.0}{sep}{ff:02.0}.{:02.0}", (sub / 80.0 * 64.0).floor()),
        "SMPTE/EBU Feet" | "Feet/Frames" => {
            // 35 mm film: 16 frames a foot
            let fr = frames_total.floor();
            format!("{:.0}:{:02.0}", (fr / 16.0).floor(), fr % 16.0)
        }
        "Seconds and Milliseconds" => format!("{h:02.0}:{m:02.0}:{:06.3}", t % 60.0),
        _ => format!("{h:02.0}:{m:02.0}:{s:02.0}{sep}{ff:02.0}.{sub:02.0}"),
    };
    if v.on(ZEROS) { zeros_as_spaces(&out) } else { out }
}

/// Leading zeros of each field shown as spaces.
fn zeros_as_spaces(s: &str) -> String {
    let mut out = String::new();
    let mut lead = true;
    for (i, ch) in s.chars().enumerate() {
        let next_digit = s.chars().nth(i + 1).is_some_and(|c| c.is_ascii_digit());
        if ch.is_ascii_digit() {
            if lead && ch == '0' && next_digit {
                out.push(' ');
                continue;
            }
            lead = false;
            out.push(ch);
        } else {
            lead = true;
            out.push(ch);
        }
    }
    out
}

/// A bars-and-beats position as Clock Format writes it.
pub fn beats_text(v: &Values, bar: i64, beat: i64, div: i64, tick: i64) -> String {
    match v.get(CLOCK).as_str() {
        "1.1.1.1" => format!("{bar}.{beat}.{div}.{tick}"),
        "1 1 1 1" => format!("{bar} {beat} {div} {tick}"),
        _ => format!("{bar}  {beat}  {div}  {tick:>3}"),
    }
}

// ---- Recording ----------------------------------------------------------------------------------

/// A text row whose words come from the settings (Resulting Latency).
pub fn dynamic_text(v: &Values, key: &str) -> Option<String> {
    (key == "Audio / Devices / Resulting Latency:").then(|| latency_text(v))
}

/// Recording ▸ Bit Depth in bits (32: float).
pub fn bit_depth(v: &Values) -> u16 {
    match v.get(REC_BIT_DEPTH).as_str() {
        "16-bit" => 16,
        "32-bit Float" => 32,
        _ => 24,
    }
}

fn overlap_name(o: Overlap, audio: bool, cycle: bool) -> &'static str {
    match (o, audio, cycle) {
        (Overlap::TakeFolder, ..) => "Create Take Folder",
        (Overlap::Merge, ..) => "Merge",
        (Overlap::Overlap, ..) => "Overlap",
        (Overlap::OverlapMergeSelected, ..) => "Overlap/Merge Selected Regions",
        (Overlap::NewTrack, _, false) => "Create Track",
        (Overlap::NewTrack, _, true) => "Create Tracks",
        (Overlap::MergeCurrentOnly, ..) => "Merge Current Recording Only",
        (Overlap::NewTracksAndMute, ..) => "Create Tracks and Mute",
    }
}

fn overlap_of(s: &str) -> Option<Overlap> {
    Some(match s {
        "Create Take Folder" => Overlap::TakeFolder,
        "Merge" => Overlap::Merge,
        "Overlap" => Overlap::Overlap,
        "Overlap/Merge Selected Regions" => Overlap::OverlapMergeSelected,
        "Create Track" | "Create Tracks" => Overlap::NewTrack,
        "Merge Current Recording Only" => Overlap::MergeCurrentOnly,
        "Create Tracks and Mute" => Overlap::NewTracksAndMute,
        _ => return None,
    })
}

const REPLACE: [(ReplaceMode, &str); 4] = [(ReplaceMode::RegionErase, "Region Erase"), (ReplaceMode::RegionPunch, "Region Punch"), (ReplaceMode::ContentErase, "Content Erase"), (ReplaceMode::ContentPunch, "Content Punch")];

/// Limit Dragging to One Direction In: Piano Roll Editor and Score Editor: a note drag moves
/// in time or in pitch, whichever way it first went.
pub fn roll_drag_axis(app: &mut App, ui: &egui::Ui) -> Option<bool> {
    if !app.logic_settings.on("General / Editing / Piano Roll Editor and Score Editor") {
        return None;
    }
    if app.rp.axis.is_none() {
        let d = ui.input(|i| i.pointer.press_origin().zip(i.pointer.latest_pos()).map(|(a, b)| b - a));
        if let Some(d) = d.filter(|d| d.length() > 4.0) {
            app.rp.axis = Some(d.x.abs() >= d.y.abs());
        }
    }
    app.rp.axis
}

/// Rows Logic shows greyed until another setting allows them: the Low Latency Limit while
/// Low Latency Monitoring Mode is on.
pub fn enabled_now(app: &App, key: &str) -> bool {
    key.starts_with("Audio / General / Limit:") && app.low_latency
}

/// A row whose value lives in the app (the Record menu shares it).
pub fn live_get(app: &App, key: &str) -> Option<String> {
    let r = &app.rec;
    Some(match key {
        REC_MIDI_OFF => overlap_name(r.midi_cycle_off, false, false).into(),
        REC_MIDI_ON => overlap_name(r.midi_cycle_on, false, true).into(),
        REC_AUDIO_OFF => overlap_name(r.audio_cycle_off, true, false).into(),
        REC_AUDIO_ON => overlap_name(r.audio_cycle_on, true, true).into(),
        REC_REPLACE => REPLACE.iter().find(|x| x.0 == r.midi_replace)?.1.into(),
        REC_FILE_TYPE => "WAVE".into(),
        LOW_LATENCY => if app.low_latency { "1" } else { "0" }.into(),
        COMPENSATION if !app.mix.compensate && app.logic_settings.get(COMPENSATION).starts_with("All") => "Audio and Software Instrument Tracks".into(),
        COMPENSATION if app.mix.compensate => "All (recommended)".into(),
        _ => return None,
    })
}

fn live_set(app: &mut App, key: &str, val: &str) {
    let r = &mut app.rec;
    match key {
        REC_MIDI_OFF => r.midi_cycle_off = overlap_of(val).unwrap_or(r.midi_cycle_off),
        REC_MIDI_ON => r.midi_cycle_on = overlap_of(val).unwrap_or(r.midi_cycle_on),
        REC_AUDIO_OFF => r.audio_cycle_off = overlap_of(val).unwrap_or(r.audio_cycle_off),
        REC_AUDIO_ON => r.audio_cycle_on = overlap_of(val).unwrap_or(r.audio_cycle_on),
        REC_REPLACE => r.midi_replace = REPLACE.iter().find(|x| x.1 == val).map_or(r.midi_replace, |x| x.0),
        LOW_LATENCY => {
            app.low_latency = val == "1";
            crate::mixengine::sync(app);
        }
        COMPENSATION => {
            let all = val.starts_with("All");
            if app.mix.compensate != all {
                app.mix.compensate = all;
                app.load_engine();
            }
        }
        _ => {}
    }
}

/// The choices a pop-up offers when they depend on the system or differ from the layout's.
pub fn choices(app: &App, key: &str) -> Option<Vec<String>> {
    let v = |a: &[&str]| Some(a.iter().map(|s| s.to_string()).collect());
    match key {
        OUTPUT_DEVICE => Some(devices(true)),
        INPUT_DEVICE => Some(devices(false)),
        DEFAULT_TEMPLATE => Some(crate::projfile::LAYOUTS.iter().map(|l| l.0.to_string()).collect()),
        "Audio / Devices / Processing Threads:" => {
            let n = std::thread::available_parallelism().map_or(4, |n| n.get());
            Some(std::iter::once("Automatic (Recommended)".to_string()).chain((1..=n).map(|k| format!("{k} (of {n} cores)"))).collect())
        }
        // (Studio records WAVE; AIFF and CAF aren't written)
        REC_FILE_TYPE => v(&["WAVE"]),
        REC_MIDI_OFF => v(&["Create Take Folder", "Merge", "Overlap", "Overlap/Merge Selected Regions", "Create Track"]),
        REC_MIDI_ON => v(&["Create Take Folder", "Merge", "Merge Current Recording Only", "Create Tracks", "Create Tracks and Mute"]),
        REC_AUDIO_OFF => v(&["Create Take Folder", "Create Track"]),
        REC_AUDIO_ON => v(&["Create Take Folder", "Create Tracks and Mute"]),
        _ => {
            let _ = app;
            None
        }
    }
}

// ---- MIDI ---------------------------------------------------------------------------------------

/// Reset Messages: what's sent to every software instrument on Stop pressed twice and Send
/// Reset Controllers (sustain off, foot, breath, modulation to zero, pitch bend centred, as
/// ticked). Aftertouch: Studio's instruments have none to reset.
pub fn send_reset(app: &mut App) {
    let v = &app.logic_settings;
    let base = "MIDI / Reset Messages / ";
    let mut msgs: Vec<(u8, u16)> = Vec::new();
    for (row, cc, val) in [("Control 64 (Sustain) off", 64u8, 0u16), ("Control 4 (Foot Control) to zero", 4, 0), ("Control 2 (Breath) to zero", 2, 0), ("Control 1 (Modulation) to zero", 1, 0), ("Pitch Bend to center position", 128, 8192)] {
        if v.on(&format!("{base}{row}")) {
            msgs.push((cc, val));
        }
    }
    let after = v.on(&format!("{base}Aftertouch to zero"));
    // (an instrument with No Reset in its Track inspector gets no reset messages)
    let skip: Vec<bool> = app.project.as_ref().map_or(Vec::new(), |p| p.tracks.iter().map(|t| t.extra.set.midi.no_reset).collect());
    let n = skip.len();
    if let Ok(mut e) = app.engine.lock() {
        for t in (0..n).filter(|&t| !skip[t]) {
            for &(cc, val) in &msgs {
                e.live_control(t, cc, val);
            }
            if after {
                e.live_message(t, 0xD0, 0, 0);
            }
        }
        // (Stop clicked again while stopped: stuck notes off, as Logic's double-click on Stop)
        e.all_off();
    }
    // External MIDI: the checked messages on all 16 channels of every MIDI output
    let bytes = external_reset(v);
    if !bytes.is_empty() {
        let outs = app.hub.outs.clone();
        for port in outs.ports() {
            outs.send(&port, &bytes);
        }
    }
}

/// Audio ▸ General ▸ Playback pre-roll: how many ticks before the play position playback starts
/// (the plug-in latency compensation's delay, `samples`, at `bpm`), so compensated tracks' first
/// transients are heard; 0 when the setting is off or nothing is delayed.
pub fn playback_preroll(v: &Values, samples: usize, bpm: f64) -> f64 {
    if samples == 0 || !v.on("Audio / General / Playback pre-roll") || v.get("Audio / General / Compensation:") == "Off" {
        return 0.0;
    }
    samples as f64 / crate::audio::RATE as f64 * bpm / 60.0 * lg_core::events::PPQ as f64
}

/// Settings ▸ MIDI ▸ Reset Messages' External MIDI rows as MIDI bytes, for channels 1-16.
pub fn external_reset(v: &Values) -> Vec<u8> {
    let base = "MIDI / Reset Messages / ";
    // (the External MIDI rows repeat Software Instruments' names: their keys end in " #2")
    let on = |row: &str, dup: bool| v.on(&format!("{base}{row}{}", if dup { " #2" } else { "" }));
    let mut out = Vec::new();
    for ch in 0..16u8 {
        for (row, dup, cc) in [("Control 123 (All Notes Off)", false, 123u8), ("Control 121 (Reset Controls)", false, 121), ("Control 64 (Sustain) off", true, 64), ("Control 4 (Foot Control) to zero", true, 4), ("Control 2 (Breath Control) to zero", false, 2), ("Control 1 (Modulation) to zero", true, 1)] {
            if on(row, dup) {
                out.extend([0xB0 | ch, cc, 0]);
            }
        }
        if on("Aftertouch to zero", true) {
            out.extend([0xD0 | ch, 0]);
        }
        if on("Pitch Bend to center position", true) {
            out.extend([0xE0 | ch, 0x00, 0x40]);
        }
    }
    out
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Send Reset Controllers" => send_reset(app),
        // (Settings back to Logic's defaults; the key command set stays)
        "Reset All Settings Except Key Command Assignments…" => {
            app.logic_settings = Values::default();
            if !app.automated {
                app.logic_settings.save();
            }
            apply_all(app);
            apply_animations(app);
            app.tell("All settings reset to their defaults (key commands kept)");
        }
        _ => return false,
    }
    true
}

// ---- buttons, changes ---------------------------------------------------------------------------

/// A Settings button pressed (by its key); false: Studio has nothing behind it.
pub fn button(app: &mut App, key: &str) -> bool {
    let pane = |app: &mut App, p: &str| app.pf.settings = Some(p.to_string());
    match key {
        "Recording /  / Recording Project Settings…" => pane(app, "Recording"),
        "MIDI / Sync / MIDI Sync Project Settings…" => pane(app, "Synchronization"),
        "Score /  / Score Project Settings…" => {
            let ctx = app.ctx.clone();
            app.key_command(&ctx, "Open Score Project Settings…");
        }
        "Movie /  / Movie Project Settings…" => pane(app, "Movie"),
        "MIDI / General / Reset All MIDI Drivers" => {
            let ctx = app.ctx.clone();
            app.live.connect_midi(&ctx);
            app.tell(format!("MIDI drivers reset: {} input{}", app.live.input_names.len(), if app.live.input_names.len() == 1 { "" } else { "s" }));
        }
        "Audio / Devices / Apply" => open_output(app),
        k if crate::autoext::button(app, k) => {}
        k if k.ends_with("Controller Assignments…") || k.ends_with("/ Setup…") => {
            let ctx = app.ctx.clone();
            app.key_command(&ctx, if k.ends_with("Setup…") { "Control Surfaces Setup…" } else { "Open Controller Assignments" });
        }
        _ => return false,
    }
    true
}

/// After a row changed (its new value saved).
pub fn changed(app: &mut App, key: &str) {
    let val = app.logic_settings.get(key);
    live_set(app, key, &val);
    if key == LOW_LATENCY {
        // (Logic: for all open projects until it quits; not kept)
        app.logic_settings.0.remove(LOW_LATENCY);
        app.logic_settings.save();
    }
    sync_statics(&app.logic_settings);
    crate::hotplug::device_changed(app, key);
    match key {
        ANIMATIONS => apply_animations(app),
        INPUT_DEVICE => {
            // (the next recording opens the new input)
            app.rec.reset_device();
        }
        VIRTUAL_IN => {
            let ctx = app.ctx.clone();
            app.live.connect_midi(&ctx);
        }
        k if k.starts_with("MIDI / Inputs / port: ") => {
            let ctx = app.ctx.clone();
            app.live.connect_midi(&ctx);
        }
        _ => {}
    }
}

fn apply_animations(app: &App) {
    let on = app.logic_settings.on(ANIMATIONS);
    app.ctx.all_styles_mut(|s| s.animation_time = if on { 0.083 } else { 0.0 });
}

/// At launch, after the app is made: the saved settings into the app.
pub fn apply_all(app: &mut App) {
    sync_statics(&app.logic_settings);
    let keys: Vec<String> = app.logic_settings.0.keys().cloned().collect();
    for k in keys {
        let v = app.logic_settings.get(&k);
        live_set(app, &k, &v);
    }
    if !app.logic_settings.on(AUDIO_ENABLED) || app.logic_settings.0.contains_key(OUTPUT_DEVICE) || app.logic_settings.0.contains_key(BUFFER) {
        open_output(app);
    } else if let Ok(mut o) = OPENED.write() {
        *o = Some(device_settings(&app.logic_settings));
    }
    if !app.logic_settings.on(ANIMATIONS) {
        apply_animations(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(kv: &[(&str, &str)]) -> Values {
        Values(kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
    }

    #[test]
    fn playback_preroll_ticks() {
        // 4800 samples = 0.1 s = 0.2 beats at 120 bpm
        assert!((playback_preroll(&vals(&[]), 4800, 120.0) - 192.0).abs() < 1e-6);
        assert_eq!(playback_preroll(&vals(&[("Audio / General / Playback pre-roll", "0")]), 4800, 120.0), 0.0);
        assert_eq!(playback_preroll(&vals(&[]), 0, 120.0), 0.0);
    }

    #[test]
    fn external_reset_messages() {
        // defaults: sustain, foot, breath, modulation, aftertouch, bend on 16 channels
        let b = external_reset(&vals(&[]));
        assert_eq!(b.len(), 16 * (4 * 3 + 2 + 3));
        assert_eq!(&b[..3], &[0xB0, 64, 0]);
        let b = external_reset(&vals(&[("MIDI / Reset Messages / Control 123 (All Notes Off)", "1"), ("MIDI / Reset Messages / Control 64 (Sustain) off #2", "0")]));
        assert_eq!(&b[..3], &[0xB0, 123, 0]);
        assert!(!b.windows(2).any(|w| w == [0xB0, 64]));
    }

    #[test]
    fn keys_are_rows_of_the_settings() {
        let all: Vec<String> = crate::settings::sections().iter().flat_map(|s| s.rows.iter().map(|r| r.key.clone())).collect();
        for k in [STARTUP, DEFAULT_TEMPLATE, AUTO_BACKUP, RECENT_ITEMS, DOUBLE_CLICK_MIDI, SELECT_REGIONS_ON_TRACK, SELECT_TRACKS_ON_REGION, CATCH_START, CATCH_MOVE, AUDIO_ENABLED, OUTPUT_DEVICE, INPUT_DEVICE, BUFFER, REC_FILE_TYPE, REC_BIT_DEPTH, REC_MIDI_OFF, REC_AUDIO_OFF, REC_MIDI_ON, REC_AUDIO_ON, REC_REPLACE, VIRTUAL_IN, WIDE_PLAYHEAD, ANIMATIONS, OPEN_ON_INSERT, BRIGHT_EDITORS] {
            assert!(all.iter().any(|x| x == k), "{k}");
        }
        // the defaults are Logic's
        let v = Values::default();
        assert_eq!(v.get(STARTUP), "Open Most Recent Project");
        assert_eq!(backups(&v), Some(10));
        assert_eq!(bit_depth(&v), 24);
        assert!(v.on(SELECT_TRACKS_ON_REGION));
    }

    #[test]
    fn backups_and_recent_items() {
        assert_eq!(backups(&vals(&[(AUTO_BACKUP, "None")])), None);
        assert_eq!(backups(&vals(&[(AUTO_BACKUP, "Last Alternative Version")])), Some(1));
        assert_eq!(backups(&vals(&[(AUTO_BACKUP, "Last 25 Alternative Versions")])), Some(25));
        assert_eq!(recent_items(&vals(&[(RECENT_ITEMS, "System Default")])), 10);
        assert_eq!(recent_items(&vals(&[(RECENT_ITEMS, "5")])), 5);
        assert_eq!(bit_depth(&vals(&[(REC_BIT_DEPTH, "16-bit")])), 16);
        assert_eq!(default_template(&vals(&[(DEFAULT_TEMPLATE, "Songwriter")])), 1);
    }

    #[test]
    fn time_display_follows_the_settings() {
        let p = crate::projsettings::Values::default();
        // Logic's default: SMPTE with subframes, bar 1 at 01:00:00:00, 25 fps
        assert_eq!(time_text(&Values::default(), &p, 0.0), "01:00:00:00.00");
        assert_eq!(time_text(&Values::default(), &p, 1.5), "01:00:01:12.40");
        assert_eq!(time_text(&vals(&[(DISPLAY_TIME, "SMPTE/EBU without Subframes")]), &p, 61.0), "01:01:01:00");
        assert_eq!(time_text(&vals(&[(DISPLAY_TIME, "SMPTE/EBU without Subframes"), (ZEROS, "1")]), &p, 61.0), " 1: 1: 1: 0");
        assert_eq!(beats_text(&vals(&[(CLOCK, "1.1.1.1")]), 3, 2, 1, 1), "3.2.1.1");
    }

    #[test]
    fn overlap_names_round_trip() {
        for o in [Overlap::TakeFolder, Overlap::Merge, Overlap::Overlap, Overlap::OverlapMergeSelected, Overlap::NewTrack, Overlap::MergeCurrentOnly, Overlap::NewTracksAndMute] {
            for cyc in [false, true] {
                assert_eq!(overlap_of(overlap_name(o, false, cyc)), Some(o));
            }
        }
    }

    #[test]
    fn inputs_switched_off() {
        sync_statics(&vals(&[("MIDI / Inputs / port: Keystation 49", "0"), ("MIDI / Inputs / port: Other", "1")]));
        assert!(input_off("Keystation 49"));
        assert!(!input_off("Other"));
        sync_statics(&vals(&[(WIDE_PLAYHEAD, "1")]));
        assert_eq!(playhead_width(), 3.0);
        sync_statics(&Values::default());
    }
}
