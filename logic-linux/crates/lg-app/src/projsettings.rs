//! Project Settings (File ▸ Project Settings ▸, ⌥P), Logic's panes with their sub-panes: General,
//! Synchronization (General / Audio / MIDI), Metronome, Recording, Tuning, Audio (General /
//! Channel Strips), MIDI (General / Input Filter / Chase / Clip Length), Score, Movie, Smart
//! Tempo and Assets; Import Project Settings… takes chosen panes from another project.
//!
//! What they do in Studio: General ▸ Use musical grid (the Record menu's), Synchronization ▸ the
//! frame rate and the bar 1 SMPTE time (the control bar's time display), Metronome (what clicks:
//! Simple mode, while playing / recording / only during count-in, bar / beat / division, Tone,
//! Volume), Recording (count-in, erase duplicates on MIDI recording, the audio recording folder),
//! Tuning (Tune in cents, Equal Tempered / Fixed historic scales / User per semitone, root key,
//! stretch; all software instruments that tune by MIDI note), MIDI ▸ General (CC 7 / 10 set the
//! track's volume and pan), MIDI ▸ Input Filter (what live MIDI is let in), Smart Tempo ▸ Project
//! Tempo Mode (Keep stops loops from changing the tempo), Assets ▸ Copy audio files.
//!
//! The values are kept per project in ~/.config/bramble-studio/project-settings/ (the Logic file
//! format's own project settings aren't decoded: Studio can't write them into the .logicx).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::app::App;

pub const COMMANDS: &[&str] = &["Import Project Settings…", "Import Settings from Project…"];

/// The panes, as Logic's File ▸ Project Settings ▸ submenu lists them.
pub const PANES: [&str; 11] = ["General", "Synchronization", "Metronome", "Recording", "Tuning", "Audio", "MIDI", "Score", "Movie", "Smart Tempo", "Assets"];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Check,
    Popup(&'static [&'static str]),
    /// A number with its range and unit.
    Num(f32, f32, &'static str),
    Text,
    Heading,
}

/// A row: (pane, sub-pane, label, kind, default). Labels double as keys ("Pane / Sub / label").
struct Row(&'static str, &'static str, &'static str, Kind, &'static str);

const FRAME_RATES: &[&str] = &["23.976", "24", "25", "29.97d", "29.97", "30d", "30", "50", "59.94d", "59.94", "60d", "60"];
const KEYS: &[&str] = &["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
const FIXED: &[&str] = &["Pythagorean", "Meantone (1/4 comma)", "Werckmeister III", "Kirnberger III", "Vallotti", "Just Intonation (major)"];
const COUNT_IN: &[&str] = &["None", "1 Bar", "2 Bars", "3 Bars", "4 Bars", "5 Bars", "6 Bars", "1/4", "2/4", "3/4", "4/4", "5/4", "6/4"];

const ROWS: &[Row] = &[
    Row("General", "", "Use musical grid", Kind::Check, "1"),
    Row("General", "", "Only load plug-ins needed for project playback", Kind::Check, "0"),
    Row("General", "", "Region automation takes priority over track automation", Kind::Check, "1"),
    Row("Synchronization", "General", "Sync Mode:", Kind::Popup(&["Internal", "Ableton Link", "MTC", "Manual"]), "Internal"),
    Row("Synchronization", "General", "Frame Rate:", Kind::Popup(FRAME_RATES), "25"),
    Row("Synchronization", "General", "Auto detect format of MTC", Kind::Check, "1"),
    Row("Synchronization", "General", "Bar Position 1 1 1 1 plays at SMPTE:", Kind::Text, "01:00:00:00.00"),
    Row("Synchronization", "General", "Enable separate SMPTE view offset", Kind::Check, "0"),
    Row("Synchronization", "Audio", "Core Audio:", Kind::Popup(&["MTC Continuous", "MTC Trigger + Auto Speed Detection", "MTC Trigger", "External or Free"]), "MTC Continuous"),
    Row("Synchronization", "General", "Auto enable External Sync", Kind::Check, "0"),
    Row("Synchronization", "MIDI", "Transmit MIDI Clock", Kind::Check, "0"),
    Row("Synchronization", "MIDI", "MIDI Clock Destination:", Kind::Text, "All"),
    Row("Synchronization", "MIDI", "Clock Mode:", Kind::Popup(&["Pattern", "Song - SPP at Play Start and Cycle Jump", "Song - SPP at Play Start, Cycle Jump and while Playing (scrubbing)"]), "Song - SPP at Play Start and Cycle Jump"),
    Row("Synchronization", "MIDI", "Transmit MTC", Kind::Check, "0"),
    Row("Synchronization", "MIDI", "MTC Destination:", Kind::Text, "All"),
    Row("Synchronization", "MIDI", "Transmit MIDI Machine Control (MMC)", Kind::Check, "0"),
    Row("Synchronization", "MIDI", "Listen to MIDI Machine Control (MMC) input", Kind::Check, "0"),
    Row("Metronome", "", "Simple mode", Kind::Check, "1"),
    Row("Metronome", "", "Click while recording", Kind::Check, "1"),
    Row("Metronome", "", "Only during count-in", Kind::Check, "0"),
    Row("Metronome", "", "Polyphonic clicks", Kind::Check, "0"),
    Row("Metronome", "", "Source", Kind::Heading, ""),
    Row("Metronome", "", "Audio Click (Klopfgeist)", Kind::Check, "1"),
    Row("Metronome", "", "Bar", Kind::Check, "1"),
    Row("Metronome", "", "Beat", Kind::Check, "1"),
    Row("Metronome", "", "Division", Kind::Check, "0"),
    Row("Metronome", "", "Tone:", Kind::Num(0.0, 100.0, "%"), "50"),
    Row("Metronome", "", "Volume:", Kind::Num(-60.0, 6.0, "dB"), "0"),
    Row("Recording", "General", "Count-in:", Kind::Popup(COUNT_IN), "1 Bar"),
    Row("Recording", "General", "Allow tempo change recording", Kind::Check, "0"),
    Row("Recording", "MIDI", "Automatically colorize takes", Kind::Check, "1"),
    Row("Recording", "MIDI", "Automatically erase duplicates", Kind::Check, "0"),
    Row("Recording", "MIDI", "MIDI data reduction", Kind::Check, "0"),
    Row("Recording", "Audio Recording Path", "Folder:", Kind::Text, "Project"),
    Row("Tuning", "", "Tune:", Kind::Num(-100.0, 100.0, "cents"), "0"),
    Row("Tuning", "", "Software Instrument Tuning Table", Kind::Popup(&["Equal Tempered", "Fixed", "User", "Hermode Tuning (HMT)"]), "Equal Tempered"),
    Row("Tuning", "", "Type:", Kind::Popup(FIXED), "Werckmeister III"),
    Row("Tuning", "", "Root Key:", Kind::Popup(KEYS), "C"),
    Row("Tuning", "", "Stretch Lower:", Kind::Num(0.0, 50.0, "cents"), "0"),
    Row("Tuning", "", "Stretch Upper:", Kind::Num(0.0, 50.0, "cents"), "0"),
    Row("Tuning", "", "HMT Type:", Kind::Popup(&["Classic (3/5-all)", "Pop/Jazz (3/5/7-all)", "Baroque (3/5-adaptive)", "Off"]), "Classic (3/5-all)"),
    Row("Tuning", "", "Depth:", Kind::Num(0.0, 100.0, "%"), "100"),
    Row("Audio", "General", "Pan Law:", Kind::Popup(&["0 dB", "-3 dB", "-3 dB Compensated", "-4.5 dB", "-4.5 dB Compensated", "-6 dB", "-6 dB Compensated"]), "-3 dB"),
    Row("Audio", "General", "Apply Pan Law compensation to stereo balancers", Kind::Check, "0"),
    Row("Audio", "Channel Strips", "Automatic management of channel strip objects", Kind::Check, "1"),
    Row("Audio", "Channel Strips", "Automatic naming of channel strip objects", Kind::Check, "1"),
    Row("MIDI", "General", "Control change message 7/10 sets Volume fader/Pan knob", Kind::Check, "1"),
    Row("MIDI", "General", "SysEx with MIDI Thru function", Kind::Check, "0"),
    Row("MIDI", "Input Filter", "Notes", Kind::Check, "0"),
    Row("MIDI", "Input Filter", "Program changes", Kind::Check, "0"),
    Row("MIDI", "Input Filter", "Pitch Bend", Kind::Check, "0"),
    Row("MIDI", "Input Filter", "Control Changes", Kind::Check, "0"),
    Row("MIDI", "Input Filter", "Aftertouch", Kind::Check, "0"),
    Row("MIDI", "Input Filter", "Polyphonic Aftertouch", Kind::Check, "0"),
    Row("MIDI", "Input Filter", "System Exclusive", Kind::Check, "0"),
    Row("MIDI", "Chase", "Notes", Kind::Check, "1"),
    Row("MIDI", "Chase", "Program Change", Kind::Check, "1"),
    Row("MIDI", "Chase", "Pitch Bend", Kind::Check, "1"),
    Row("MIDI", "Chase", "Control Changes", Kind::Check, "1"),
    Row("MIDI", "Clip Length", "Control 1 (Modulation) to zero", Kind::Check, "1"),
    Row("MIDI", "Clip Length", "Pitch Bend to center position", Kind::Check, "1"),
    Row("Smart Tempo", "", "Project Tempo Mode:", Kind::Popup(&["Keep Project Tempo", "Adapt Project Tempo", "Automatic Mode"]), "Automatic Mode"),
    Row("Smart Tempo", "", "Free Tempo Recording Default Tempo Behavior:", Kind::Popup(&["Ask", "Apply region tempo to project", "Apply average region tempo to project", "Apply project tempo to region", "Don't analyze region tempo or change project tempo"]), "Ask"),
    Row("Assets", "", "Copy audio files into project", Kind::Check, "1"),
    Row("Assets", "", "Convert audio file sample rate when importing", Kind::Check, "1"),
    Row("Assets", "", "Copy Sampler audio files into project", Kind::Check, "1"),
    Row("Assets", "", "Copy Space Designer impulse responses into project", Kind::Check, "1"),
];

fn key(r: &Row) -> String {
    format!("{} / {} / {}", r.0, r.1, r.2)
}

/// A project's settings (only the values changed from Logic's defaults are kept).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Values(pub BTreeMap<String, String>);

impl Values {
    pub fn get(&self, k: &str) -> String {
        self.0.get(k).cloned().or_else(|| ROWS.iter().find(|r| key(r) == k).map(|r| r.4.to_string())).unwrap_or_default()
    }

    pub fn on(&self, k: &str) -> bool {
        self.get(k) == "1"
    }

    pub fn num(&self, k: &str) -> f32 {
        self.get(k).parse().unwrap_or(0.0)
    }

    pub fn to_text(&self) -> String {
        self.0.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
    }

    pub fn from_text(t: &str) -> Values {
        Values(t.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect())
    }

    /// Import Project Settings: the chosen panes' values from another project.
    pub fn import(&mut self, other: &Values, panes: &[&str]) {
        for r in ROWS.iter().filter(|r| panes.contains(&r.0)) {
            let k = key(r);
            match other.0.get(&k) {
                Some(v) => self.0.insert(k, v.clone()),
                None => self.0.remove(&k),
            };
        }
    }
}

// ---- what the values do ------------------------------------------------------------------------

/// The engine's click (Metronome pane).
pub fn click_cfg(v: &Values) -> crate::audio::ClickCfg {
    let m = |l: &str| v.on(&format!("Metronome /  / {l}"));
    crate::audio::ClickCfg {
        simple: m("Simple mode"),
        while_recording: m("Click while recording"),
        only_count_in: m("Only during count-in"),
        bar: m("Audio Click (Klopfgeist)") && m("Bar"),
        beat: m("Audio Click (Klopfgeist)") && m("Beat"),
        division: m("Audio Click (Klopfgeist)") && m("Division"),
        tone: v.num("Metronome /  / Tone:") / 100.0,
        volume_db: v.num("Metronome /  / Volume:"),
    }
}

/// Cents per pitch class of the historic scales (C = 0; from their published ratios).
fn fixed_scale(name: &str) -> [f32; 12] {
    match name {
        "Pythagorean" => [0.0, 13.7, 3.9, -5.9, 7.8, -2.0, 11.7, 2.0, 15.6, 5.9, -3.9, 9.8],
        "Meantone (1/4 comma)" => [0.0, -24.0, -6.8, 10.3, -13.7, 3.4, -20.5, -3.4, -27.4, -10.3, 6.8, -17.1],
        "Werckmeister III" => [0.0, -9.8, -7.8, -5.9, -9.8, -2.0, -11.7, -3.9, -7.8, -11.7, -3.9, -7.8],
        "Kirnberger III" => [0.0, -9.8, -6.8, -5.9, -13.7, -2.0, -9.8, -3.4, -7.8, -10.3, -3.9, -11.7],
        "Vallotti" => [0.0, -5.9, -3.9, -2.0, -7.8, 2.0, -7.8, -2.0, -3.9, -5.9, 0.0, -9.8],
        "Just Intonation (major)" => [0.0, 11.7, 3.9, 15.6, -13.7, -2.0, -9.8, 2.0, 13.7, -15.6, 17.6, -11.7],
        _ => [0.0; 12],
    }
}

/// The tuning table the instruments play by (cents per MIDI note).
pub fn tuning_table(v: &Values) -> [f32; 128] {
    let tune = v.num("Tuning /  / Tune:");
    let root = KEYS.iter().position(|k| *k == v.get("Tuning /  / Root Key:")).unwrap_or(0);
    let scale: [f32; 12] = match v.get("Tuning /  / Software Instrument Tuning Table").as_str() {
        "Fixed" => fixed_scale(&v.get("Tuning /  / Type:")),
        "User" => std::array::from_fn(|k| v.get(&format!("Tuning /  / User {}", KEYS[k])).parse().unwrap_or(0.0)),
        _ => [0.0; 12],
    };
    let (lo, hi) = (v.num("Tuning /  / Stretch Lower:"), v.num("Tuning /  / Stretch Upper:"));
    std::array::from_fn(|p| {
        // (the scale's degrees count from the root key)
        let deg = (p + 12 - root) % 12;
        let base = scale[deg] - scale[0];
        // stretch: notes below C3 (60) go flat, above go sharp, by up to the value at the ends
        let stretch = if p < 60 { -lo * (60 - p) as f32 / 60.0 } else { hi * (p - 60) as f32 / 67.0 };
        tune + base + stretch
    })
}

/// The bar 1 SMPTE time in seconds and the frame rate (fps, drop-frame).
pub fn smpte(v: &Values) -> (f64, f64, bool) {
    let rate = v.get("Synchronization / General / Frame Rate:");
    let drop = rate.ends_with('d');
    let fps: f64 = rate.trim_end_matches('d').parse().unwrap_or(25.0);
    let off = if v.on("Synchronization / General / Enable separate SMPTE view offset") {
        0.0
    } else {
        let t = v.get("Synchronization / General / Bar Position 1 1 1 1 plays at SMPTE:");
        let f: Vec<f64> = t.split([':', '.', ';']).filter_map(|x| x.trim().parse().ok()).collect();
        f.first().copied().unwrap_or(0.0) * 3600.0 + f.get(1).copied().unwrap_or(0.0) * 60.0 + f.get(2).copied().unwrap_or(0.0) + f.get(3).copied().unwrap_or(0.0) / fps.round()
    };
    (off, fps, drop)
}

/// A MIDI message let through by MIDI ▸ Input Filter.
pub fn input_passes(v: &Values, status: u8) -> bool {
    let f = |l: &str| !v.on(&format!("MIDI / Input Filter / {l}"));
    match status & 0xF0 {
        0x80 | 0x90 => f("Notes"),
        0xA0 => f("Polyphonic Aftertouch"),
        0xB0 => f("Control Changes"),
        0xC0 => f("Program changes"),
        0xD0 => f("Aftertouch"),
        0xE0 => f("Pitch Bend"),
        0xF0 => f("System Exclusive"),
        _ => true,
    }
}

/// MIDI ▸ General: CC 7 and 10 move the track's fader and pan.
pub fn cc_moves_strip(v: &Values) -> bool {
    v.on("MIDI / General / Control change message 7/10 sets Volume fader/Pan knob")
}

/// Smart Tempo ▸ Keep Project Tempo: loops and imports don't change the tempo.
pub fn keep_tempo(v: &Values) -> bool {
    v.get("Smart Tempo /  / Project Tempo Mode:") == "Keep Project Tempo"
}

/// Recording ▸ Automatically erase duplicates.
pub fn erase_duplicates(v: &Values) -> bool {
    v.on("Recording / MIDI / Automatically erase duplicates")
}

/// Recording ▸ Audio Recording Path: a folder of its own (None: the project's).
pub fn recording_folder(v: &Values) -> Option<PathBuf> {
    let f = v.get("Recording / Audio Recording Path / Folder:");
    (f != "Project" && !f.is_empty()).then(|| PathBuf::from(f))
}

/// Pushes the values into the app (after a change, an open, an import).
pub fn apply(app: &mut App) {
    let v = app.proj.clone();
    app.rec.musical_grid = v.on("General /  / Use musical grid");
    app.pf.copy_assets = v.on("Assets /  / Copy audio files into project");
    let ci = v.get("Recording / General / Count-in:");
    if ci == "None" {
        app.count_in = false;
    } else if let Some(c) = parse_count_in(&ci) {
        app.count_in = true;
        app.rec.count_in = c;
    }
    lg_dsp::set_tuning(&tuning_table(&v));
    let cfg = click_cfg(&v);
    if let Ok(mut e) = app.engine.lock() {
        e.click_cfg = cfg;
    }
}

fn parse_count_in(s: &str) -> Option<crate::recording::CountIn> {
    if let Some(n) = s.strip_suffix(" Bars").or_else(|| s.strip_suffix(" Bar")) {
        return n.trim().parse().ok().map(crate::recording::CountIn::Bars);
    }
    s.strip_suffix("/4").and_then(|n| n.parse().ok()).map(crate::recording::CountIn::Beats)
}

/// The app's own state back into the values (the Record menu changes musical grid and count-in).
fn read_back(app: &mut App) {
    let grid = if app.rec.musical_grid { "1" } else { "0" };
    set(&mut app.proj, "General /  / Use musical grid", grid);
    let ci = if !app.count_in {
        "None".to_string()
    } else {
        match app.rec.count_in {
            crate::recording::CountIn::Bars(1) => "1 Bar".into(),
            crate::recording::CountIn::Bars(n) => format!("{n} Bars"),
            crate::recording::CountIn::Beats(n) => format!("{n}/4"),
        }
    };
    set(&mut app.proj, "Recording / General / Count-in:", &ci);
    let ca = if app.pf.copy_assets { "1" } else { "0" };
    set(&mut app.proj, "Assets /  / Copy audio files into project", ca);
}

fn set(v: &mut Values, k: &str, val: &str) {
    let def = ROWS.iter().find(|r| key(r) == k).map(|r| r.4);
    if def == Some(val) {
        v.0.remove(k);
    } else {
        v.0.insert(k.to_string(), val.to_string());
    }
}

// ---- kept per project ---------------------------------------------------------------------------

fn file_for(project: &Path) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let id: String = project.canonicalize().unwrap_or_else(|_| project.to_path_buf()).display().to_string().chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    Some(base.join("bramble-studio").join("project-settings").join(format!("{id}.txt")))
}

fn project_path(app: &App) -> Option<PathBuf> {
    app.project.as_ref().map(|p| p.path.clone()).filter(|p| !app.demo && p.exists())
}

/// A project opened: its settings (Logic's defaults when it has none).
pub fn after_open(app: &mut App) {
    app.proj = project_path(app).and_then(|p| file_for(&p)).and_then(|f| std::fs::read_to_string(f).ok()).map(|t| Values::from_text(&t)).unwrap_or_default();
    apply(app);
    crate::surround::after_open(app);
}

/// The project saved: its settings beside it.
pub fn save(app: &mut App) {
    read_back(app);
    crate::surround::store(app);
    if app.automated {
        return;
    }
    let Some(f) = project_path(app).and_then(|p| file_for(&p)) else { return };
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, app.proj.to_text());
}

/// Import Project Settings…: the settings another project keeps (see `file_for`).
pub fn settings_of(project: &Path) -> Option<Values> {
    file_for(project).and_then(|f| std::fs::read_to_string(f).ok()).map(|t| Values::from_text(&t))
}

// ---- the window ---------------------------------------------------------------------------------

#[derive(Default)]
pub struct Ui {
    pub sub: String,
    /// Import Project Settings: the project chosen, its settings, the panes ticked.
    pub import: Option<(PathBuf, Values, Vec<String>)>,
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Import Project Settings…" | "Import Settings from Project…" => {
            if app.automated {
                return true;
            }
            if let Some(p) = kn_ui::files::FileDialog::new().set_title("Import Project Settings from a project (.logicx)").pick_folder() {
                let p = lg_core::project::package_of(&p);
                match settings_of(&p) {
                    Some(v) => app.ps_ui.import = Some((p, v, PANES.iter().filter(|p| !matches!(**p, "Score" | "Movie")).map(|s| s.to_string()).collect())),
                    None => app.tell(format!("“{}” has no project settings Studio made (only projects saved in Studio keep them)", p.file_stem().unwrap_or_default().to_string_lossy())),
                }
            }
        }
        _ => return false,
    }
    true
}

fn subs(pane: &str) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for r in ROWS.iter().filter(|r| r.0 == pane) {
        if !v.contains(&r.1) {
            v.push(r.1);
        }
    }
    v
}

/// The window (projfile.rs calls this with the pane shown).
pub fn window(app: &mut App, ctx: &egui::Context, pane: &mut String, open: &mut bool) {
    read_back(app);
    let before = app.proj.clone();
    egui::Window::new("Project Settings").id(egui::Id::unique("project-settings")).open(open).collapsible(false).resizable(false).default_width(620.0).show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            for t in PANES {
                if ui.add(egui::Button::selectable(pane == t, t)).clicked() {
                    *pane = t.to_string();
                    app.ps_ui.sub.clear();
                }
            }
        });
        ui.separator();
        let ss = subs(pane);
        if ss.len() > 1 {
            if !ss.contains(&app.ps_ui.sub.as_str()) {
                app.ps_ui.sub = ss[0].to_string();
            }
            ui.horizontal(|ui| {
                for s in &ss {
                    if ui.add(egui::Button::selectable(app.ps_ui.sub == *s, *s)).clicked() {
                        app.ps_ui.sub = s.to_string();
                    }
                }
            });
            ui.add_space(4.0);
        }
        let sub = if ss.len() > 1 { app.ps_ui.sub.clone() } else { ss.first().map(|s| s.to_string()).unwrap_or_default() };
        match pane.as_str() {
            "General" => crate::projfile::general_info(app, ui),
            "Score" => {
                ui.label("The Score's project settings (staff size, bar numbers, chords, MIDI Meaning, colours) are in its own window.");
                if ui.button("Score Project Settings…").clicked() {
                    crate::score::set_editor_tab(ctx, crate::score::EditorTab::Score);
                    app.bottom = crate::app::Bottom::PianoRoll;
                    crate::score::with_state(ctx, |s| s.settings = Some(0));
                }
            }
            "Movie" => {
                ui.label(egui::RichText::new("Studio has no movie track: Movie project settings have nothing to set.").color(crate::look::text_dim()));
            }
            "Audio" if sub == "General" => {
                if let Some(p) = &app.project {
                    ui.label(format!("Sample Rate: {:.1} kHz (the project's; Studio plays at {} kHz)", p.sample_rate as f64 / 1000.0, crate::audio::RATE / 1000));
                }
                crate::surround_ui::project_settings(app, ui);
            }
            "Recording" if sub == "Audio Recording Path" => {
                let cur = app.proj.get("Recording / Audio Recording Path / Folder:");
                ui.label(format!("Folder: {}", if cur == "Project" { crate::recording::audio_dir(app).display().to_string() } else { cur.clone() }));
                ui.horizontal(|ui| {
                    if ui.button("Set…").clicked() && !app.automated {
                        if let Some(d) = kn_ui::files::FileDialog::new().set_title("Audio Recording Path").pick_folder() {
                            set(&mut app.proj, "Recording / Audio Recording Path / Folder:", &d.display().to_string());
                        }
                    }
                    if ui.button("Project").clicked() {
                        set(&mut app.proj, "Recording / Audio Recording Path / Folder:", "Project");
                    }
                });
            }
            _ => {}
        }
        egui::Grid::new(("ps-rows", pane.clone(), sub.clone())).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            for r in ROWS.iter().filter(|r| r.0 == pane && r.1 == sub) {
                let k = key(r);
                if matches!(r.3, Kind::Text) && k.ends_with("Folder:") {
                    continue;
                }
                let cur = app.proj.get(&k);
                match r.3 {
                    Kind::Heading => {
                        ui.label(egui::RichText::new(r.2).strong());
                        ui.end_row();
                        continue;
                    }
                    Kind::Check => {
                        ui.label("");
                        let mut on = cur == "1";
                        if ui.checkbox(&mut on, r.2).changed() {
                            set(&mut app.proj, &k, if on { "1" } else { "0" });
                        }
                    }
                    Kind::Popup(ch) => {
                        ui.label(r.2);
                        egui::ComboBox::from_id_salt(&k).selected_text(&cur).width(260.0).show_ui(ui, |ui| {
                            for c in ch {
                                if ui.selectable_label(cur == *c, *c).clicked() {
                                    set(&mut app.proj, &k, c);
                                }
                            }
                        });
                    }
                    Kind::Num(lo, hi, unit) => {
                        ui.label(r.2);
                        let mut x: f32 = cur.parse().unwrap_or(0.0);
                        if ui.add(egui::Slider::new(&mut x, lo..=hi).suffix(format!(" {unit}"))).changed() {
                            set(&mut app.proj, &k, &format!("{x:.1}"));
                        }
                    }
                    Kind::Text => {
                        ui.label(r.2);
                        let mut t = cur.clone();
                        if ui.text_edit_singleline(&mut t).changed() {
                            set(&mut app.proj, &k, &t);
                        }
                    }
                }
                ui.end_row();
            }
        });
        // Tuning ▸ User: the twelve semitones from the root (the root itself fixed)
        if pane == "Tuning" && app.proj.get("Tuning /  / Software Instrument Tuning Table") == "User" {
            ui.horizontal_wrapped(|ui| {
                for (k, n) in KEYS.iter().enumerate() {
                    let key = format!("Tuning /  / User {n}");
                    let mut c: f32 = app.proj.0.get(&key).and_then(|x| x.parse().ok()).unwrap_or(0.0);
                    ui.label(*n);
                    let root = KEYS.iter().position(|x| *x == app.proj.get("Tuning /  / Root Key:")).unwrap_or(0);
                    if ui.add_enabled(k != root, egui::DragValue::new(&mut c).range(-100.0..=100.0).speed(0.1).fixed_decimals(1)).changed() {
                        app.proj.0.insert(key, format!("{c:.1}"));
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Copy to User").on_hover_text("The Fixed scale's values into the User semitones").clicked() {
                    let s = fixed_scale(&app.proj.get("Tuning /  / Type:"));
                    for (k, n) in KEYS.iter().enumerate() {
                        app.proj.0.insert(format!("Tuning /  / User {n}"), format!("{:.1}", s[k] - s[0]));
                    }
                }
                if ui.button("Reset").clicked() {
                    app.proj.0.retain(|k, _| !k.starts_with("Tuning /  / User "));
                }
            });
        }
        let note = match (pane.as_str(), sub.as_str()) {
            ("Synchronization", "General") => "Sync Mode MTC: with external sync on (Toggle Sync intern/extern) Studio follows incoming MIDI Time Code. Ableton Link and Manual (Tempo Interpreter) aren't available. The frame rate and bar 1's SMPTE time set MTC, MMC and the time display.",
            ("Synchronization", "MIDI") => "Destinations: a MIDI output's name, All or Off (Bramble Studio Virtual Out reaches other apps). MIDI Clock and MTC go out while playing; MMC is sent on play, stop, record and locate.",
            ("Synchronization", _) => "Studio has no word clock: Core Audio's choice is kept with the project only.",
            ("Audio", "Channel Strips") => "Automatic management keeps one channel strip object per track on the Environment's Mixer layer (Window ▸ Open MIDI Environment).",
            ("Metronome", _) => "Klopfgeist: Studio's own click (bar, beat and division pitches, Tone, Volume).",
            ("Tuning", _) => "Applies to every software instrument that tunes by MIDI note. Hermode Tuning isn't built (plays equal tempered).",
            ("MIDI", "Chase") => "Notes: notes already under way at a play start or locate sound from there. Studio's regions carry no program, pitch bend or controller events to chase.",
            ("MIDI", "Clip Length") => "Kept with the project; Studio's instruments don't reset these yet.",
            ("Audio", "General") => "Studio's mixer pans with its own law: the Pan Law choice is kept with the project only.",
            _ => "",
        };
        if !note.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(note).small().color(crate::look::text_dim()));
        }
    });
    if app.proj != before {
        apply(app);
        app.dirty = true;
    }
    import_sheet(app, ctx);
}

fn import_sheet(app: &mut App, ctx: &egui::Context) {
    let Some((path, vals, mut ticked)) = app.ps_ui.import.take() else { return };
    let mut done = None;
    egui::Window::new("Import Project Settings").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -40.0)).show(ctx, |ui| {
        ui.label(format!("From “{}”:", path.file_stem().unwrap_or_default().to_string_lossy()));
        for p in PANES.iter().filter(|p| !matches!(**p, "Score" | "Movie")) {
            let mut on = ticked.iter().any(|t| t == p);
            if ui.checkbox(&mut on, *p).changed() {
                if on {
                    ticked.push(p.to_string());
                } else {
                    ticked.retain(|t| t != p);
                }
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                done = Some(false);
            }
            if ui.button("Import").clicked() {
                done = Some(true);
            }
        });
    });
    match done {
        Some(true) => {
            let panes: Vec<&str> = ticked.iter().map(|s| s.as_str()).collect();
            app.proj.import(&vals, &panes);
            apply(app);
            app.dirty = true;
        }
        Some(false) => {}
        None => app.ps_ui.import = Some((path, vals, ticked)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_round_trip() {
        let v = Values::default();
        assert_eq!(v.get("Synchronization / General / Frame Rate:"), "25");
        assert!(v.on("Metronome /  / Simple mode"));
        let mut w = v.clone();
        set(&mut w, "Tuning /  / Tune:", "12.0");
        set(&mut w, "Metronome /  / Simple mode", "1");
        assert_eq!(w.0.len(), 1, "defaults aren't kept");
        assert_eq!(Values::from_text(&w.to_text()), w);
    }

    #[test]
    fn tuning_tables() {
        let mut v = Values::default();
        assert!(tuning_table(&v).iter().all(|c| *c == 0.0));
        set(&mut v, "Tuning /  / Tune:", "10");
        assert!(tuning_table(&v).iter().all(|c| (*c - 10.0).abs() < 1e-4));
        set(&mut v, "Tuning /  / Tune:", "0");
        set(&mut v, "Tuning /  / Software Instrument Tuning Table", "Fixed");
        set(&mut v, "Tuning /  / Type:", "Meantone (1/4 comma)");
        let t = tuning_table(&v);
        // C stays, E (a pure third) is 13.7 cents low, in every octave
        assert_eq!(t[60], 0.0);
        assert!((t[64] + 13.7).abs() < 0.01 && (t[76] + 13.7).abs() < 0.01);
        // the root key moves the scale: in G, B is the low third
        set(&mut v, "Tuning /  / Root Key:", "G");
        let t = tuning_table(&v);
        assert!((t[71] + 13.7).abs() < 0.01 && t[67] == 0.0);
        // stretch: low notes flat, high sharp
        let mut s = Values::default();
        set(&mut s, "Tuning /  / Stretch Lower:", "20");
        set(&mut s, "Tuning /  / Stretch Upper:", "20");
        let t = tuning_table(&s);
        assert!(t[0] < -19.0 && t[127] > 19.0 && t[60] == 0.0);
    }

    #[test]
    fn filter_click_smpte_import() {
        let mut v = Values::default();
        assert!(input_passes(&v, 0x90));
        set(&mut v, "MIDI / Input Filter / Notes", "1");
        assert!(!input_passes(&v, 0x93) && input_passes(&v, 0xB0));
        let c = click_cfg(&v);
        assert!(c.simple && c.bar && c.beat && !c.division);
        assert_eq!(smpte(&v), (3600.0, 25.0, false));
        set(&mut v, "Synchronization / General / Frame Rate:", "29.97d");
        assert!(smpte(&v).2);
        let mut w = Values::default();
        w.import(&v, &["MIDI"]);
        assert!(w.on("MIDI / Input Filter / Notes"));
        assert_eq!(w.get("Synchronization / General / Frame Rate:"), "25");
        assert_eq!(parse_count_in("2 Bars"), Some(crate::recording::CountIn::Bars(2)));
        assert_eq!(parse_count_in("3/4"), Some(crate::recording::CountIn::Beats(3)));
    }
}
