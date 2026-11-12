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

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::look;

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
    /// Radio buttons in a row (the choices).
    Radio(&'static [&'static str]),
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
    Row("General", "", "Reset record enable state of Software Instrument tracks", Kind::Check, "0"),
    Row("General", "", "Region automation takes priority over track automation", Kind::Check, "1"),
    Row("General", "", "Use preset parameter value for regions without region automation", Kind::Check, "0"),
    Row("Synchronization", "General", "Sync Mode:", Kind::Popup(&["Internal", "Ableton Link", "MTC", "Manual"]), "Internal"),
    Row("Synchronization", "General", "Frame Rate:", Kind::Popup(FRAME_RATES), "25"),
    Row("Synchronization", "General", "Auto detect format of MTC", Kind::Check, "1"),
    Row("Synchronization", "General", "Bar Position 1 1 1 1 plays at SMPTE:", Kind::Text, "01:00:00:00.00"),
    Row("Synchronization", "General", "Enable separate SMPTE view offset", Kind::Check, "0"),
    Row("Synchronization", "General", "Bar Position 1 1 1 1 displayed as SMPTE:", Kind::Text, "00:00:00:00.00"),
    Row("Synchronization", "Audio", "Core Audio:", Kind::Popup(&["MTC Continuous", "MTC Trigger + Auto Speed Detection", "MTC Trigger", "External or Free"]), "MTC Continuous"),
    Row("Synchronization", "General", "Auto enable External Sync", Kind::Check, "0"),
    Row("Synchronization", "MIDI", "Transmit MIDI Clock", Kind::Check, "0"),
    Row("Synchronization", "MIDI", "MIDI Clock Destination:", Kind::Text, "All"),
    Row("Synchronization", "MIDI", "Clock Mode:", Kind::Popup(&lg_core::sync::ClockMode::NAMES), "Song - SPP at Play Start and Stop/SPP/Continue at Cycle Jump"),
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
    Row("Metronome", "", "Group", Kind::Check, "0"),
    Row("Metronome", "", "Bar Note", Kind::Num(0.0, 127.0, ""), "93"),
    Row("Metronome", "", "Bar Velocity", Kind::Num(1.0, 127.0, ""), "112"),
    Row("Metronome", "", "Group Note", Kind::Num(0.0, 127.0, ""), "88"),
    Row("Metronome", "", "Group Velocity", Kind::Num(1.0, 127.0, ""), "100"),
    Row("Metronome", "", "Beat Note", Kind::Num(0.0, 127.0, ""), "88"),
    Row("Metronome", "", "Beat Velocity", Kind::Num(1.0, 127.0, ""), "100"),
    Row("Metronome", "", "Division Note", Kind::Num(0.0, 127.0, ""), "83"),
    Row("Metronome", "", "Division Velocity", Kind::Num(1.0, 127.0, ""), "88"),
    Row("Metronome", "", "MIDI Click", Kind::Check, "0"),
    Row("Metronome", "", "MIDI Bar", Kind::Check, "1"),
    Row("Metronome", "", "MIDI Bar Channel", Kind::Num(1.0, 16.0, ""), "10"),
    Row("Metronome", "", "MIDI Bar Note", Kind::Num(0.0, 127.0, ""), "37"),
    Row("Metronome", "", "MIDI Bar Velocity", Kind::Num(1.0, 127.0, ""), "112"),
    Row("Metronome", "", "MIDI Group", Kind::Check, "0"),
    Row("Metronome", "", "MIDI Group Channel", Kind::Num(1.0, 16.0, ""), "10"),
    Row("Metronome", "", "MIDI Group Note", Kind::Num(0.0, 127.0, ""), "37"),
    Row("Metronome", "", "MIDI Group Velocity", Kind::Num(1.0, 127.0, ""), "100"),
    Row("Metronome", "", "MIDI Beat", Kind::Check, "1"),
    Row("Metronome", "", "MIDI Beat Channel", Kind::Num(1.0, 16.0, ""), "10"),
    Row("Metronome", "", "MIDI Beat Note", Kind::Num(0.0, 127.0, ""), "37"),
    Row("Metronome", "", "MIDI Beat Velocity", Kind::Num(1.0, 127.0, ""), "88"),
    Row("Metronome", "", "MIDI Division", Kind::Check, "0"),
    Row("Metronome", "", "MIDI Division Channel", Kind::Num(1.0, 16.0, ""), "10"),
    Row("Metronome", "", "MIDI Division Note", Kind::Num(0.0, 127.0, ""), "37"),
    Row("Metronome", "", "MIDI Division Velocity", Kind::Num(1.0, 127.0, ""), "72"),
    Row("Metronome", "", "MIDI Port:", Kind::Text, "All"),
    Row("Metronome", "", "Tone:", Kind::Num(0.0, 100.0, "%"), "50"),
    Row("Metronome", "", "Volume:", Kind::Num(-60.0, 6.0, "dB"), "0"),
    Row("Recording", "General", "Lead-in:", Kind::Radio(&crate::recsettings::LEADS), "Count-in"),
    Row("Recording", "General", "Count-in:", Kind::Popup(COUNT_IN), "1 Bar"),
    Row("Recording", "General", "Pre-roll:", Kind::Num(0.0, 60.0, "s"), "1.0"),
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
    Row("MIDI", "Chase", "Sustained", Kind::Check, "1"),
    Row("MIDI", "Chase", "In No Transpose instrument channel strips", Kind::Check, "1"),
    Row("MIDI", "Chase", "Program Change", Kind::Check, "1"),
    Row("MIDI", "Chase", "Pitch Bend", Kind::Check, "1"),
    Row("MIDI", "Chase", "Control Changes", Kind::Check, "1"),
    Row("MIDI", "Chase", "0-15", Kind::Check, "1"),
    Row("MIDI", "Chase", "64-71", Kind::Check, "1"),
    Row("MIDI", "Chase", "All Other", Kind::Check, "1"),
    Row("MIDI", "Chase", "Aftertouch", Kind::Check, "1"),
    Row("MIDI", "Chase", "Polyphonic Aftertouch", Kind::Check, "1"),
    Row("MIDI", "Chase", "System Exclusive", Kind::Check, "1"),
    Row("MIDI", "Chase", "Text Meta Events", Kind::Check, "1"),
    Row("MIDI", "Chase", "Chase separate channels in All Channels instruments", Kind::Check, "1"),
    Row("MIDI", "Chase", "Chase on cycle jump", Kind::Check, "0"),
    Row("MIDI", "Chase", "Notes (on cycle jump)", Kind::Check, "1"),
    Row("MIDI", "Chase", "Send full MIDI reset before chasing", Kind::Check, "0"),
    Row("MIDI", "Clip Length", "Aftertouch to zero", Kind::Check, "1"),
    Row("MIDI", "Clip Length", "Pitch Bend to center position", Kind::Check, "1"),
    Row("MIDI", "Clip Length", "Control 1 (Modulation) to zero", Kind::Check, "1"),
    Row("MIDI", "Clip Length", "Control 2 (Breath Control) to zero", Kind::Check, "0"),
    Row("MIDI", "Clip Length", "Control 4 (Foot Control) to zero", Kind::Check, "0"),
    Row("MIDI", "Clip Length", "Control 11 (Expression) to max", Kind::Check, "0"),
    Row("Score", "Global", "Top Margin:", Kind::Num(0.0, 10.0, "cm"), "1.75"),
    Row("Score", "Global", "Bottom Margin:", Kind::Num(0.0, 10.0, "cm"), "1.75"),
    Row("Score", "Global", "Left Margin:", Kind::Num(0.0, 10.0, "cm"), "1.58"),
    Row("Score", "Global", "Right Margin:", Kind::Num(0.0, 10.0, "cm"), "1.58"),
    Row("Score", "Global", "Add bracket space", Kind::Check, "0"),
    Row("Score", "Global", "Alternating margins", Kind::Check, "0"),
    Row("Score", "Global", "Header Space:", Kind::Num(0.0, 15.0, "cm"), "2.45"),
    Row("Score", "Global", "Line Distance:", Kind::Num(0.0, 10.0, "cm"), "0.70"),
    Row("Score", "Global", "Maximum Bars/Line:", Kind::Num(1.0, 99.0, ""), "99"),
    Row("Score", "Global", "Justify last staff", Kind::Check, "0"),
    Row("Score", "Global", "Open single staffs", Kind::Check, "0"),
    Row("Score", "Global", "Hide muted notes", Kind::Check, "0"),
    Row("Score", "Global", "Hide muted regions", Kind::Check, "0"),
    Row("Score", "Global", "Hide muted tracks", Kind::Check, "0"),
    Row("Score", "Numbers & Names", "Page Numbers", Kind::Check, "1"),
    Row("Score", "Numbers & Names", "Page Horizontal Position:", Kind::Popup(&crate::scoreset::PAGE_H), "Alternating"),
    Row("Score", "Numbers & Names", "Page Vertical Position:", Kind::Popup(&["Top", "Bottom"]), "Bottom"),
    Row("Score", "Numbers & Names", "Page Offset:", Kind::Num(-99.0, 999.0, ""), "0"),
    Row("Score", "Numbers & Names", "Page Horizontal Distance:", Kind::Num(0.0, 10.0, "cm"), "0.70"),
    Row("Score", "Numbers & Names", "Page Vertical Distance:", Kind::Num(0.0, 10.0, "cm"), "0.70"),
    Row("Score", "Numbers & Names", "Hide first page number", Kind::Check, "0"),
    Row("Score", "Numbers & Names", "Prefix:", Kind::Text, ""),
    Row("Score", "Numbers & Names", "Bar Numbers", Kind::Check, "1"),
    Row("Score", "Numbers & Names", "Bar Horizontal Position:", Kind::Popup(&["At Bar Line", "At Bar Center"]), "At Bar Line"),
    Row("Score", "Numbers & Names", "Bar Vertical Position:", Kind::Num(-20.0, 20.0, ""), "2.0"),
    Row("Score", "Numbers & Names", "Step:", Kind::Num(0.0, 99.0, ""), "0"),
    Row("Score", "Numbers & Names", "Bar Offset:", Kind::Num(-999.0, 999.0, ""), "0"),
    Row("Score", "Numbers & Names", "Start With:", Kind::Num(1.0, 999.0, ""), "2"),
    Row("Score", "Numbers & Names", "Hide bar numbers in linear view", Kind::Check, "1"),
    Row("Score", "Numbers & Names", "Follow staff size", Kind::Check, "1"),
    Row("Score", "Numbers & Names", "Top/bottom staff only", Kind::Check, "1"),
    Row("Score", "Numbers & Names", "Instrument Names", Kind::Check, "0"),
    Row("Score", "Numbers & Names", "Name Position:", Kind::Popup(&["Above Staffs", "Beside Staffs"]), "Beside Staffs"),
    Row("Score", "Numbers & Names", "Name Align:", Kind::Popup(&["Left", "Right"]), "Right"),
    Row("Score", "Numbers & Names", "First Staff:", Kind::Popup(&crate::scoreset::NAMES), "Full Names"),
    Row("Score", "Numbers & Names", "Other Staffs:", Kind::Popup(&crate::scoreset::NAMES), "Short Names"),
    Row("Score", "Clefs & Signatures", "Clefs:", Kind::Popup(&crate::scoreset::SHOW), "Every Staff"),
    Row("Score", "Clefs & Signatures", "Key Signatures:", Kind::Popup(&crate::scoreset::SHOW), "Every Staff"),
    Row("Score", "Clefs & Signatures", "Time Signatures:", Kind::Popup(&crate::scoreset::SHOW), "First Staff on Page 1"),
    Row("Score", "Clefs & Signatures", "Hide bar lines", Kind::Check, "0"),
    Row("Smart Tempo", "", "Project Tempo Mode:", Kind::Popup(&["Keep Project Tempo", "Adapt Project Tempo", "Automatic Mode"]), "Automatic Mode"),
    Row("Smart Tempo", "", "Set New Recordings To:", Kind::Popup(&crate::stdefaults::CHOICES), "Flex Off"),
    Row("Smart Tempo", "", "Trim start of new regions", Kind::Check, "0"),
    Row("Smart Tempo", "", "Set Imported Files To:", Kind::Popup(&crate::stdefaults::CHOICES), "Flex Off"),
    Row("Smart Tempo", "", "Trim start of new regions #2", Kind::Check, "0"),
    Row("Smart Tempo", "", "Export Tempo Resolution:", Kind::Popup(&crate::stdefaults::EXPORT), "Smoothed"),
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
        group: m("Audio Click (Klopfgeist)") && m("Group"),
        poly: m("Polyphonic clicks"),
        notes: ["Bar", "Group", "Beat", "Division"].map(|l| v.num(&format!("Metronome /  / {l} Note")).clamp(0.0, 127.0) as u8),
        vels: ["Bar", "Group", "Beat", "Division"].map(|l| v.num(&format!("Metronome /  / {l} Velocity")).clamp(1.0, 127.0) as u8),
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

/// The bar 1 SMPTE time as the displays show it (the separate view offset when that is on) in
/// seconds, and the frame rate (fps, drop-frame).
pub fn smpte(v: &Values) -> (f64, f64, bool) {
    let (real, fps, drop) = smpte_real(v);
    if v.on("Synchronization / General / Enable separate SMPTE view offset") {
        (smpte_secs(&v.get("Synchronization / General / Bar Position 1 1 1 1 displayed as SMPTE:"), fps), fps, drop)
    } else {
        (real, fps, drop)
    }
}

/// The bar 1 SMPTE time that is played and synchronised to (MTC, the movie, the Tempo List).
pub fn smpte_real(v: &Values) -> (f64, f64, bool) {
    let rate = v.get("Synchronization / General / Frame Rate:");
    let drop = rate.ends_with('d');
    let fps: f64 = rate.trim_end_matches('d').parse().unwrap_or(25.0);
    (smpte_secs(&v.get("Synchronization / General / Bar Position 1 1 1 1 plays at SMPTE:"), fps), fps, drop)
}

fn smpte_secs(t: &str, fps: f64) -> f64 {
    let f: Vec<f64> = t.split([':', '.', ';']).filter_map(|x| x.trim().parse().ok()).collect();
    f.first().copied().unwrap_or(0.0) * 3600.0 + f.get(1).copied().unwrap_or(0.0) * 60.0 + f.get(2).copied().unwrap_or(0.0) + f.get(3).copied().unwrap_or(0.0) / fps.round()
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

/// MIDI ▸ Clip Length: aftertouch, pitch bend, CC 1, 2, 4 and 11 reset at a region's end.
pub fn clip_length(v: &Values) -> [bool; 6] {
    ["Aftertouch to zero", "Pitch Bend to center position", "Control 1 (Modulation) to zero", "Control 2 (Breath Control) to zero", "Control 4 (Foot Control) to zero", "Control 11 (Expression) to max"].map(|l| v.on(&format!("MIDI / Clip Length / {l}")))
}

/// Smart Tempo ▸ Keep Project Tempo: loops and imports don't change the tempo.
pub fn keep_tempo(v: &Values) -> bool {
    v.get("Smart Tempo /  / Project Tempo Mode:") == "Keep Project Tempo"
}

/// Recording ▸ Automatically erase duplicates.
pub fn erase_duplicates(v: &Values) -> bool {
    v.on("Recording / MIDI / Automatically erase duplicates")
}

/// Automatically erase duplicates: notes of one pitch and channel starting within 100 ticks of a
/// later one in the list (the newer take) are dropped, the newer kept (unquantized playing
/// counts as the same position within 100 ticks, as Logic).
pub fn erase_near_duplicates(notes: &mut Vec<lg_core::events::Note>) {
    let mut keep = vec![true; notes.len()];
    for i in 0..notes.len() {
        for j in i + 1..notes.len() {
            let (a, b) = (&notes[i], &notes[j]);
            if keep[j] && a.pitch == b.pitch && a.channel == b.channel && a.pos.abs_diff(b.pos) <= 100 {
                keep[i] = false;
                break;
            }
        }
    }
    let mut k = 0;
    notes.retain(|_| {
        k += 1;
        keep[k - 1]
    });
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
    crate::hmt::configure(&v);
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

pub(crate) fn set(v: &mut Values, k: &str, val: &str) {
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
    crate::metroset::after_open(app);
    restore_armed(app);
    crate::projend::after_open(app);
    apply(app);
    crate::surround::after_open(app);
}

/// Where the record enable state is kept (not a pane's row).
const ARMED: &str = "State / Tracks / Record enabled";

/// A project opened: its record-enabled tracks again (General ▸ Reset record enable state of
/// Software Instrument tracks: instrument tracks come back off).
fn restore_armed(app: &mut App) {
    let reset = app.proj.on("General /  / Reset record enable state of Software Instrument tracks");
    let kinds: Vec<lg_core::TrackKind> = app.project.as_ref().map(|p| p.tracks.iter().map(|t| t.kind).collect()).unwrap_or_default();
    let list = app.proj.0.get(ARMED).cloned().unwrap_or_default();
    app.armed = list.split(',').filter_map(|x| x.parse().ok()).filter(|&t: &usize| t < kinds.len() && !(reset && kinds[t] == lg_core::TrackKind::Instrument)).collect();
}

/// The project saved: its settings beside it.
pub fn save(app: &mut App) {
    read_back(app);
    // (the tracks' record enable buttons are kept with the project, as Logic keeps them)
    let mut armed: Vec<usize> = app.armed.iter().copied().collect();
    armed.sort_unstable();
    let list = armed.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(",");
    if list.is_empty() {
        app.proj.0.remove(ARMED);
    } else {
        app.proj.0.insert(ARMED.into(), list);
    }
    crate::surround::store(app);
    crate::projend::store(app);
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
                // (Score Sets, Staff Styles and Text Styles come from its Score data: scorefile.rs)
                let has_score = crate::scorefile::score_of(&p).is_some();
                match settings_of(&p).or_else(|| has_score.then(Values::default)) {
                    Some(v) => app.ps_ui.import = Some((p, v, PANES.iter().filter(|p| !matches!(**p, "Movie")).map(|s| s.to_string()).chain(crate::scorefile::IMPORT_PANES.iter().filter(|_| has_score).map(|s| s.to_string())).collect())),
                    None => app.tell(format!("“{}” has no project settings Studio made (only projects saved in Studio keep them)", p.file_stem().unwrap_or_default().to_string_lossy())),
                }
            }
        }
        _ => return false,
    }
    true
}

/// Sub-panes drawn by their own code (the rows still keep the values and defaults).
fn custom(pane: &str, sub: &str) -> bool {
    matches!((pane, sub), ("MIDI", "Chase") | ("Synchronization", "MIDI") | ("Metronome", "")) || pane == "Score"
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

/// The window (projfile.rs calls this with the pane shown), drawn as Logic's settings windows
/// (settings.rs): the title band with the panes as icon tabs, the sub-panes as a segmented
/// control on the group box's top border, labels right-aligned before their controls.
pub fn window(app: &mut App, ctx: &egui::Context, pane: &mut String, open: &mut bool) {
    read_back(app);
    let before = app.proj.clone();
    let screen = ctx.content_rect();
    egui::Window::new("Project Settings").id(egui::Id::unique("project-settings")).title_bar(false).resizable(false).collapsible(false).frame(egui::Frame::NONE).default_pos(pos2(screen.center().x - PS_W / 2.0, screen.min.y + 70.0)).show(ctx, |ui| {
        ui.set_width(PS_W);
        let p = ui.painter().clone();
        let bg = p.add(egui::Shape::Noop);
        let top = ui.cursor().min;
        // the title band: the close light, the title, the panes
        let (band, _) = ui.allocate_exact_size(vec2(PS_W, PS_BAND), Sense::hover());
        p.rect_filled(band, egui::CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 }, gray(0x2e2d2c));
        p.hline(band.x_range(), band.max.y, Stroke::new(0.5, gray(0x353433)));
        p.text(pos2(band.center().x, band.min.y + 15.5), Align2::CENTER_CENTER, "Project Settings", look::bold(13.0), gray(0xaaa9a8));
        let close = Rect::from_center_size(band.min + vec2(16.0, 16.0), vec2(14.0, 14.0));
        p.circle_filled(close.center(), 7.0, gray(0xe1675b));
        for k in 1..3 {
            p.circle_filled(close.center() + vec2(20.0 * k as f32, 0.0), 7.0, gray(0x525150));
        }
        if ui.interact(close, ui.scope_id().with("ps-close"), Sense::click()).clicked() {
            *open = false;
        }
        let widths: Vec<f32> = PANES.iter().map(|t| p.layout_no_wrap(t.to_string(), look::regular(11.0), Color32::WHITE).size().x + 14.0).map(|w| w.max(48.0)).collect();
        let gap = 4.0;
        let total = widths.iter().sum::<f32>() + gap * (PANES.len() - 1) as f32;
        let mut x = band.center().x - total / 2.0;
        for (i, t) in PANES.iter().enumerate() {
            let tr = Rect::from_min_size(pos2(x, band.min.y + 32.0), vec2(widths[i], 48.0));
            x += tr.width() + gap;
            let sel = pane == t;
            if sel {
                p.rect_filled(tr, 7.5, gray(0x4b4a49));
                p.rect_stroke(tr, 7.5, Stroke::new(0.5, gray(0x100f0f)), egui::StrokeKind::Outside);
                p.hline(tr.min.x + 7.0..=tr.max.x - 7.0, tr.min.y + 0.5, Stroke::new(0.5, gray(0x7b7977)));
            }
            let icon = PANE_ICONS[i];
            crate::glyphs::paint(&p, Rect::from_center_size(pos2(tr.center().x, band.min.y + 48.0), vec2(26.0, 26.0)), icon, 24.0, if sel { gray(0x3478f6) } else { gray(0xa1a1a0) });
            p.text(pos2(tr.center().x, band.min.y + 68.0), Align2::CENTER_CENTER, *t, look::regular(11.0), if sel { gray(0x3d85f7) } else { gray(0x979696) });
            if ui.interact(tr, ui.scope_id().with(("ps-tab", i)), Sense::click()).clicked() {
                *pane = t.to_string();
                app.ps_ui.sub.clear();
            }
        }
        let ss = subs(pane);
        if ss.len() > 1 && !ss.contains(&app.ps_ui.sub.as_str()) {
            app.ps_ui.sub = ss[0].to_string();
        }
        let sub = if ss.len() > 1 { app.ps_ui.sub.clone() } else { ss.first().map(|s| s.to_string()).unwrap_or_default() };
        // the group box (drawn behind its contents once their height is known)
        let boxed = ss.len() > 1;
        ui.add_space(if boxed { 30.0 } else { 20.0 });
        let box_top = ui.cursor().min.y;
        let box_bg = p.add(egui::Shape::Noop);
        ui.add_space(if boxed { 24.0 } else { 0.0 });
        let inner = Rect::from_min_max(pos2(top.x + 20.0 + 23.0, ui.cursor().min.y), pos2(top.x + PS_W - 20.0 - 23.0, ui.cursor().min.y + 2000.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
        child.style_mut().spacing.item_spacing.y = 7.0;
        body(app, ctx, &mut child, pane, &sub);
        let used = child.min_rect();
        ui.allocate_rect(Rect::from_min_max(pos2(top.x, inner.min.y), pos2(top.x + PS_W, used.max.y.max(inner.min.y) + 18.0)), Sense::hover());
        let bottom = ui.cursor().min.y;
        if boxed {
            let bx = Rect::from_min_max(pos2(top.x + 20.0, box_top), pos2(top.x + PS_W - 20.0, bottom - 2.0));
            p.set(box_bg, egui::Shape::rect_filled(bx, 4.0, gray(0x2d2c2b)));
            p.rect_stroke(bx, 4.0, Stroke::new(1.0, gray(0x464545)), egui::StrokeKind::Inside);
            // the segmented control on its top border
            let f = look::regular(13.0);
            let segw: Vec<f32> = ss.iter().map(|s| p.layout_no_wrap(s.to_string(), f.clone(), Color32::WHITE).size().x + 30.0).collect();
            let seg = Rect::from_center_size(pos2(bx.center().x, bx.min.y), vec2(segw.iter().sum::<f32>() + 4.0, 24.0));
            p.rect_filled(seg, 5.0, gray(0x363534));
            let mut sx = seg.min.x + 2.0;
            for (k, sname) in ss.iter().enumerate() {
                let sr = Rect::from_min_size(pos2(sx, seg.min.y + 2.0), vec2(segw[k], 20.0));
                sx += segw[k];
                if *sname == sub {
                    p.rect_filled(sr, 4.0, gray(0x545251));
                    p.rect_stroke(sr, 4.0, Stroke::new(0.5, gray(0x7c7978)), egui::StrokeKind::Inside);
                }
                p.text(sr.center(), Align2::CENTER_CENTER, *sname, f.clone(), gray(0xe0e0e0));
                if ui.interact(sr, ui.scope_id().with(("ps-sub", k)), Sense::click()).clicked() {
                    app.ps_ui.sub = sname.to_string();
                }
            }
        }
        ui.add_space(if boxed { 19.0 } else { 8.0 });
        let whole = Rect::from_min_max(top, pos2(top.x + PS_W, ui.cursor().min.y));
        p.set(bg, egui::Shape::rect_filled(whole, 12.0, gray(0x262524)));
        p.rect_stroke(whole, 12.0, Stroke::new(0.5, Color32::BLACK), egui::StrokeKind::Outside);
    });
    if app.proj != before {
        apply(app);
        app.dirty = true;
    }
    import_sheet(app, ctx);
}

const PS_W: f32 = 860.0;
const PS_BAND: f32 = 88.0;
const PANE_ICONS: [&str; 11] = [
    crate::glyphs::ph::GEAR_SIX,
    crate::glyphs::ph::ARROWS_CLOCKWISE,
    crate::glyphs::ph::METRONOME,
    crate::glyphs::ph::RECORD,
    crate::glyphs::br::TUNING_FORK,
    crate::glyphs::ph::WAVEFORM,
    crate::glyphs::br::MIDI,
    crate::glyphs::ph::MUSIC_NOTES,
    crate::glyphs::ph::FILM_STRIP,
    crate::glyphs::ph::GAUGE,
    crate::glyphs::ph::FOLDER_SIMPLE,
];

/// The pane's rows as Logic's: labels right-aligned at one column, checkboxes, pop-ups,
/// values and text fields after it (settings.rs's look).
fn rows(app: &mut App, ui: &mut egui::Ui, pane: &str, sub: &str) {
    let list: Vec<&Row> = ROWS.iter().filter(|r| r.0 == pane && r.1 == sub && !custom(r.0, r.1)).filter(|r| !(matches!(r.3, Kind::Text) && key(r).ends_with("Folder:"))).filter(|r| key(r) != crate::recsettings::LEAD_IN).collect();
    if list.is_empty() {
        return;
    }
    let p = ui.painter().clone();
    let f = look::regular(13.0);
    let label_w = list.iter().filter(|r| !matches!(r.3, Kind::Check | Kind::Heading)).map(|r| p.layout_no_wrap(r.2.to_string(), f.clone(), Color32::WHITE).size().x).fold(75.0f32, f32::max);
    let left = ui.max_rect().min.x;
    let label_right = left + label_w + 10.0;
    // (Logic's settings panes give their pop-ups one width: the widest one's)
    let popup_w = list.iter().filter_map(|r| if let Kind::Popup(ch) = r.3 { Some(ch) } else { None }).flat_map(|ch| ch.iter()).map(|c| p.layout_no_wrap(c.to_string(), f.clone(), Color32::WHITE).size().x + 44.0).fold(0.0f32, f32::max).clamp(120.0, 420.0);
    let ctrl_x = label_right + 9.5;
    let fill = gray(0x3d3c3b);
    let text = gray(0xdfdfdf);
    for (k, r) in list.into_iter().enumerate() {
        let key = key(r);
        let cur = app.proj.get(&key);
        let id = ui.scope_id().with(("ps-row", k));
        // (" #2": a second row with the same label)
        let r = &Row(r.0, r.1, r.2.split(" #").next().unwrap_or(r.2), r.3, r.4);
        // (Logic's pitch: checkboxes 23 pt apart, pop-ups and fields 31 pt)
        let h = if matches!(r.3, Kind::Check) { 16.0 } else { 24.0 };
        // (rows another row switches off: dimmed, not editable)
        let live = enabled(&app.proj, &key);
        let text = if live { text } else { gray(0x7a7978) };
        let (row, _) = ui.allocate_exact_size(vec2(ui.max_rect().width(), h), Sense::hover());
        let cy = row.center().y;
        match r.3 {
            Kind::Heading => {
                p.text(pos2(left, cy), Align2::LEFT_CENTER, r.2, look::bold(13.0), text);
            }
            Kind::Check => {
                let b = Rect::from_min_size(pos2(ctrl_x, cy - 8.0), vec2(16.0, 16.0));
                let on = cur == "1";
                p.rect_filled(b, 3.5, if on { gray(0x397bf7) } else { gray(0x424140) });
                if on {
                    p.line_segment([b.center() + vec2(-4.0, 0.0), b.center() + vec2(-1.2, 3.2)], Stroke::new(2.0, Color32::WHITE));
                    p.line_segment([b.center() + vec2(-1.2, 3.2), b.center() + vec2(4.5, -3.8)], Stroke::new(2.0, Color32::WHITE));
                }
                let g = p.layout_no_wrap(r.2.to_string(), f.clone(), text);
                let hit = Rect::from_min_size(b.min, vec2(22.5 + g.size().x, 16.0));
                p.galley(pos2(ctrl_x + 22.5, cy - g.size().y / 2.0), g, text);
                if ui.interact(hit, id, Sense::click()).clicked() {
                    set(&mut app.proj, &key, if on { "0" } else { "1" });
                }
            }
            Kind::Popup(ch) => {
                lead_radio(app, ui, &p, &key, label_right, cy, r.2, &f, id);
                p.text(pos2(label_right, cy), Align2::RIGHT_CENTER, r.2, f.clone(), gray(0xdfdfdf));
                let _ = ch;
                let pr = Rect::from_min_size(pos2(ctrl_x, row.min.y), vec2(popup_w, 24.0));
                let resp = ui.interact(pr, id, if live { Sense::click() } else { Sense::hover() });
                p.rect_filled(pr, 5.0, if resp.hovered() { look::mix(fill, Color32::WHITE, 0.06) } else { fill });
                p.with_clip_rect(pr.shrink2(vec2(10.0, 0.0))).text(pos2(pr.min.x + 12.0, cy), Align2::LEFT_CENTER, &cur, f.clone(), text);
                let m = pos2(pr.max.x - 14.0, cy);
                for sg in [-1.0f32, 1.0] {
                    p.line_segment([m + vec2(-3.5, sg * 1.5), m + vec2(0.0, sg * 5.0)], Stroke::new(1.3, gray(0xe1e1e1)));
                    p.line_segment([m + vec2(0.0, sg * 5.0), m + vec2(3.5, sg * 1.5)], Stroke::new(1.3, gray(0xe1e1e1)));
                }
                egui::Popup::menu(&resp).show(|ui| {
                    ui.set_min_width(pr.width());
                    for c in ch {
                        if ui.add(egui::Button::selectable(cur == *c, *c)).clicked() {
                            set(&mut app.proj, &key, c);
                            ui.close();
                        }
                    }
                });
            }
            Kind::Num(lo, hi, unit) => {
                lead_radio(app, ui, &p, &key, label_right, cy, r.2, &f, id);
                p.text(pos2(label_right, cy), Align2::RIGHT_CENTER, r.2, f.clone(), if lead_row(&key) { gray(0xdfdfdf) } else { text });
                let mut x: f32 = cur.parse().unwrap_or(0.0);
                let fr = Rect::from_min_size(pos2(ctrl_x, row.min.y), vec2(110.0, 24.0));
                p.rect_filled(fr, 5.0, fill);
                let resp = ui.add_enabled_ui(live, |ui| ui.put(fr.shrink2(vec2(8.0, 2.0)), egui::DragValue::new(&mut x).range(lo..=hi).speed((hi - lo) / 400.0).fixed_decimals(1).suffix(format!(" {unit}")))).inner;
                if resp.changed() {
                    set(&mut app.proj, &key, &format!("{x:.1}"));
                }
            }
            Kind::Radio(ch) => {
                p.text(pos2(label_right, cy), Align2::RIGHT_CENTER, r.2, f.clone(), text);
                let mut x = ctrl_x;
                for (i, c) in ch.iter().enumerate() {
                    let on = cur == *c;
                    let g = p.layout_no_wrap(c.to_string(), f.clone(), text);
                    let hit = Rect::from_min_size(pos2(x, cy - 8.0), vec2(22.5 + g.size().x, 16.0));
                    let ctr = pos2(x + 8.0, cy);
                    p.circle_filled(ctr, 8.0, if on { gray(0x397bf7) } else { gray(0x424140) });
                    if on {
                        p.circle_filled(ctr, 3.0, Color32::WHITE);
                    }
                    p.galley(pos2(x + 22.5, cy - g.size().y / 2.0), g, text);
                    if ui.interact(hit, id.with(i), Sense::click()).clicked() {
                        set(&mut app.proj, &key, c);
                    }
                    x = hit.max.x + 18.0;
                }
            }
            Kind::Text => {
                p.text(pos2(label_right, cy), Align2::RIGHT_CENTER, r.2, f.clone(), text);
                let fr = Rect::from_min_size(pos2(ctrl_x, row.min.y), vec2(220.0, 24.0));
                p.rect_filled(fr, 5.0, fill);
                let mut t = cur.clone();
                if ui.add_enabled_ui(live, |ui| ui.put(fr.shrink2(vec2(8.0, 3.0)), egui::TextEdit::singleline(&mut t).frame(egui::Frame::NONE).font(f.clone()))).inner.changed() {
                    set(&mut app.proj, &key, &t);
                }
            }
        }
    }
}

fn lead_row(key: &str) -> bool {
    key == "Recording / General / Count-in:" || key == crate::recsettings::PRE_ROLL
}

/// Recording ▸ Count-in / Pre-roll: Logic's radio buttons before the two labels pick the lead-in.
#[allow(clippy::too_many_arguments)]
fn lead_radio(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, key: &str, label_right: f32, cy: f32, label: &str, f: &egui::FontId, id: egui::Id) {
    if !lead_row(key) {
        return;
    }
    let mine = if key == crate::recsettings::PRE_ROLL { "Pre-roll" } else { "Count-in" };
    let on = app.proj.get(crate::recsettings::LEAD_IN) == mine;
    let w = p.layout_no_wrap(label.to_string(), f.clone(), Color32::WHITE).size().x;
    let ctr = pos2(label_right - w - 14.0, cy);
    p.circle_filled(ctr, 8.0, if on { gray(0x397bf7) } else { gray(0x424140) });
    if on {
        p.circle_filled(ctr, 3.0, Color32::WHITE);
    }
    let hit = Rect::from_min_max(pos2(ctr.x - 8.0, cy - 9.0), pos2(label_right, cy + 9.0));
    if ui.interact(hit, id.with("lead"), Sense::click()).clicked() {
        set(&mut app.proj, crate::recsettings::LEAD_IN, mine);
    }
}

/// Whether a row can be edited now (Count-in's menu with Count-in chosen, Pre-roll's slider with
/// Pre-roll, HMT's rows with Hermode Tuning).
fn enabled(v: &Values, key: &str) -> bool {
    let lead = v.get(crate::recsettings::LEAD_IN);
    match key {
        "Recording / General / Count-in:" => lead != "Pre-roll",
        crate::recsettings::PRE_ROLL => lead == "Pre-roll",
        "Synchronization / General / Bar Position 1 1 1 1 displayed as SMPTE:" => v.on("Synchronization / General / Enable separate SMPTE view offset"),
        "Tuning /  / Type:" => v.get("Tuning /  / Software Instrument Tuning Table") == "Fixed",
        "Tuning /  / HMT Type:" | "Tuning /  / Depth:" => v.get("Tuning /  / Software Instrument Tuning Table") == "Hermode Tuning (HMT)",
        _ => true,
    }
}

fn gray(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// A pane's contents: its own code's controls, then its rows (labels right-aligned).
fn body(app: &mut App, ctx: &egui::Context, ui: &mut egui::Ui, pane: &str, sub: &str) {
    let pane = pane.to_string();
    let pane = &pane;
    let sub = sub.to_string();
        match pane.as_str() {
            "General" => crate::projfile::general_info(app, ui),
            "MIDI" if sub == "Chase" => crate::chase::pane(ui, &mut app.proj),
            "Metronome" => crate::metroset::pane(app, ui),
            "Synchronization" if sub == "MIDI" => crate::syncmidi::pane(app, ui),
            "Score" => {
                crate::scoreset::pane(ui, &mut app.proj, &sub);
                ui.add_space(6.0);
                if ui.button("Score Settings…").on_hover_text("The Score's own settings window (staff size, chords, MIDI Meaning, colours)").clicked() {
                    crate::score::set_editor_tab(ctx, crate::score::EditorTab::Score);
                    app.bottom = crate::app::Bottom::PianoRoll;
                    crate::score::with_state(ctx, |s| s.settings = Some(0));
                }
            }
            "Movie" => movie_pane(app, ui),
            "Audio" if sub == "General" => {
                if let Some(p) = &app.project {
                    ui.label(format!("Sample Rate: {:.1} kHz (the project's; Studio plays at {} kHz)", p.sample_rate as f64 / 1000.0, crate::audio::RATE / 1000));
                }
                crate::surround_ui::project_settings(app, ui);
            }
            "Recording" if sub == "General" => {
                if ui.button("Recording Settings…").clicked() {
                    open_logic_settings(app, "Recording", "");
                }
                ui.add_space(4.0);
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
        rows(app, ui, pane, &sub);
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
            ("Synchronization", "Audio") => "Studio has no word clock: Core Audio's choice is kept with the project only.",
            ("Audio", "Channel Strips") => "Automatic management keeps one channel strip object per track on the Environment's Mixer layer (Window ▸ Open MIDI Environment).",
            ("Tuning", _) => "Applies to every software instrument that tunes by MIDI note. Hermode Tuning retunes each note as it starts, to the chord sounding.",
            ("MIDI", "Clip Length") => "At a MIDI region's end, a reset for each ticked kind the region holds and played.",
            ("Audio", "General") => "Pan Law: the level of a centred mono signal (Compensated: centre unchanged, hard pans raised instead).",
            _ => "",
        };
        if !note.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(note).small().color(crate::look::text_dim()));
        }
}

/// Opens Logic Settings at a tab (and sub-tab).
fn open_logic_settings(app: &mut App, tab: &str, sub: &str) {
    app.settings_tab = tab.into();
    app.settings_sub = sub.into();
    app.show_settings = true;
}

/// SMPTE text of seconds at a frame rate ("01:00:00:00"; a semicolon before the frames marks a
/// drop-frame rate).
pub fn smpte_text(secs: f64, fps: f64, drop: bool) -> String {
    let fr = fps.round().max(1.0);
    let total = (secs.max(0.0) * fr + 1e-6).floor() as i64;
    let f = total % fr as i64;
    let s = total / fr as i64;
    format!("{:02}:{:02}:{:02}{}{:02}", s / 3600, s / 60 % 60, s % 60, if drop { ';' } else { ':' }, f)
}

fn parse_smpte(t: &str, fps: f64) -> Option<f64> {
    let f: Vec<f64> = t.split([':', ';', '.']).map(|x| x.trim().parse().ok()).collect::<Option<Vec<f64>>>()?;
    (f.len() >= 3).then(|| f[0] * 3600.0 + f[1] * 60.0 + f[2] + f.get(3).copied().unwrap_or(0.0) / fps.round().max(1.0))
}

/// Project Settings ▸ Movie: the Movie Position (where the movie starts, SMPTE) and the Movie
/// Settings button.
fn movie_pane(app: &mut App, ui: &mut egui::Ui) {
    let (bar1, fps, drop) = smpte(&app.proj);
    if app.hub.movie.path.is_none() {
        ui.label(egui::RichText::new("No movie in the project (File ▸ Movie ▸ Open Movie…).").color(crate::look::text_dim()));
    } else {
        ui.horizontal(|ui| {
            ui.label("Movie Position:");
            let id = ui.scope_id().with("movie-pos");
            let cur = smpte_text(app.hub.movie.start.unwrap_or(bar1), fps, drop);
            let mut t = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| cur.clone());
            let r = ui.add(egui::TextEdit::singleline(&mut t).desired_width(110.0));
            if r.lost_focus() {
                if let Some(v) = parse_smpte(&t, fps) {
                    app.hub.movie.start = Some(v);
                    app.dirty = true;
                }
                ui.data_mut(|d| d.remove::<String>(id));
            } else if r.has_focus() {
                ui.data_mut(|d| d.insert_temp(id, t));
            }
        });
    }
    if ui.button("Movie Settings…").clicked() {
        open_logic_settings(app, "Movie", "");
    }
}

fn import_sheet(app: &mut App, ctx: &egui::Context) {
    let Some((path, vals, mut ticked)) = app.ps_ui.import.take() else { return };
    let mut done = None;
    egui::Window::new("Import Project Settings").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -40.0)).show(ctx, |ui| {
        ui.label(format!("From “{}”:", path.file_stem().unwrap_or_default().to_string_lossy()));
        let score = crate::scorefile::score_of(&path);
        for p in PANES.iter().filter(|p| !matches!(**p, "Movie")).chain(crate::scorefile::IMPORT_PANES.iter().filter(|_| score.is_some())) {
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
            if let Some(other) = crate::scorefile::score_of(&path) {
                crate::scorefile::import(app, &other, &panes);
            }
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
        assert_eq!(smpte_text(3600.0 + 2.5, 25.0, false), "01:00:02:12");
        assert_eq!(smpte_text(61.0, 29.97, true), "00:01:01;00");
        assert!((parse_smpte("01:00:02:12", 25.0).unwrap_or(0.0) - 3602.48).abs() < 1e-9);
        let n = |pos, pitch| lg_core::events::Note { channel: 0, pitch, velocity: 90, pos, len: 100, muted: false };
        let mut notes = vec![n(0, 60), n(960, 62), n(40, 60), n(1200, 62)];
        erase_near_duplicates(&mut notes);
        assert_eq!(notes.iter().map(|x| x.pos).collect::<Vec<_>>(), vec![960, 40, 1200]);
    }
}
