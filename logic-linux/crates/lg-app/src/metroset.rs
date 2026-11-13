//! Project Settings ▸ Metronome laid out as Logic's (Simple mode and its click choices,
//! Polyphonic clicks, the Audio Click and MIDI Click sources with a Bar / Group / Beat / Division
//! table of notes, velocities and MIDI channels, Tone, Volume, the MIDI Click port), the MIDI
//! click itself, and the control bar Metronome button's Control-click menu.
//!
//! As in Logic, the source choice (Audio Click / MIDI Click) and the last Simple mode are kept in
//! the app's settings: the source applies to every project, Simple mode to projects without
//! their own metronome settings.

use crate::app::App;
use crate::projsettings::{set, Values};

const P: &str = "Metronome /  / ";
const LEVELS: [&str; 4] = ["Bar", "Group", "Beat", "Division"];
/// The app-wide copies (Logic keeps these in its preferences).
const PREF_SIMPLE: &str = "Metronome / Simple mode";
const PREF_AUDIO: &str = "Metronome / Audio Click";
const PREF_MIDI: &str = "Metronome / MIDI Click";

fn k(l: &str) -> String {
    format!("{P}{l}")
}

/// A changed project value that Logic also keeps app-wide.
fn remember(app: &mut App, l: &str, on: bool) {
    let pref = match l {
        "Simple mode" => PREF_SIMPLE,
        "Audio Click (Klopfgeist)" => PREF_AUDIO,
        "MIDI Click" => PREF_MIDI,
        _ => return,
    };
    app.logic_settings.0.insert(pref.into(), (on as u8).to_string());
    if !app.automated {
        app.logic_settings.save();
    }
}

/// A project opened: the app-wide source, and the last Simple mode unless the project has its
/// own.
pub fn after_open(app: &mut App) {
    let prefs = app.logic_settings.0.clone();
    if let Some(v) = prefs.get(PREF_SIMPLE).filter(|_| !app.proj.0.contains_key(&k("Simple mode"))) {
        set(&mut app.proj, &k("Simple mode"), v);
    }
    for (pref, l) in [(PREF_AUDIO, "Audio Click (Klopfgeist)"), (PREF_MIDI, "MIDI Click")] {
        if let Some(v) = prefs.get(pref) {
            set(&mut app.proj, &k(l), v);
        }
    }
}

fn check(app: &mut App, ui: &mut egui::Ui, l: &str, text: &str, enabled: bool) -> bool {
    let mut on = app.proj.on(&k(l));
    let r = ui.add_enabled(enabled, egui::Checkbox::new(&mut on, text));
    if r.changed() {
        set(&mut app.proj, &k(l), if on { "1" } else { "0" });
        remember(app, l, on);
    }
    on
}

fn num(v: &mut Values, ui: &mut egui::Ui, l: &str, lo: f32, hi: f32, enabled: bool, show: fn(f32) -> String) {
    let mut x = v.num(&k(l));
    let r = ui.add_enabled(enabled, egui::DragValue::new(&mut x).range(lo..=hi).speed(0.25).custom_formatter(move |n, _| show(n as f32)).custom_parser(parse_note));
    if r.changed() {
        set(v, &k(l), &format!("{}", x.round()));
    }
}

fn plain(n: f32) -> String {
    format!("{}", n.round())
}

/// Note names as Logic shows them (C3 = 60).
pub fn note_name(n: f32) -> String {
    const N: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
    let n = n.round().clamp(0.0, 127.0) as i32;
    format!("{}{}", N[(n % 12) as usize], n / 12 - 2)
}

fn parse_note(s: &str) -> Option<f64> {
    if let Ok(n) = s.trim().parse::<f64>() {
        return Some(n);
    }
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let t = s.trim().replace('♯', "#");
    let (name, oct) = t.split_at(t.find(|c: char| c == '-' || c.is_ascii_digit())?);
    let pc = N.iter().position(|x| x.eq_ignore_ascii_case(name))? as f64;
    Some(pc + (oct.parse::<f64>().ok()? + 2.0) * 12.0)
}

/// The Metronome pane.
pub fn pane(app: &mut App, ui: &mut egui::Ui) {
    let simple = check(app, ui, "Simple mode", "Simple mode", true);
    ui.indent("metro-simple", |ui| {
        let rec = check(app, ui, "Click while recording", "Click while recording", !simple);
        ui.indent("metro-rec", |ui| check(app, ui, "Only during count-in", "Only during count-in", !simple && rec));
        // Click while playing: the Metronome button's own state (Simple mode off)
        let mut on = app.engine.lock().map(|e| e.metronome).unwrap_or(false);
        if ui.add_enabled(!simple, egui::Checkbox::new(&mut on, "Click while playing")).changed() {
            if let Ok(mut e) = app.engine.lock() {
                e.metronome = on;
            }
        }
    });
    check(app, ui, "Polyphonic clicks", "Polyphonic clicks", true);
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Source").strong());
    let audio = check(app, ui, "Audio Click (Klopfgeist)", "Audio Click (Klopfgeist)", true);
    egui::Grid::new("metro-audio").num_columns(3).spacing([12.0, 3.0]).show(ui, |ui| {
        ui.label("");
        ui.label(egui::RichText::new("Note").small());
        ui.label(egui::RichText::new("Velocity").small());
        ui.end_row();
        for l in LEVELS {
            check(app, ui, l, l, audio);
            num(&mut app.proj, ui, &format!("{l} Note"), 0.0, 127.0, audio, note_name);
            num(&mut app.proj, ui, &format!("{l} Velocity"), 1.0, 127.0, audio, plain);
            ui.end_row();
        }
    });
    egui::Grid::new("metro-tone").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
        for (l, lo, hi, unit) in [("Tone:", 0.0, 100.0, " %"), ("Volume:", -60.0, 6.0, " dB")] {
            ui.label(l);
            let mut x = app.proj.num(&k(l));
            if ui.add_enabled(audio, egui::Slider::new(&mut x, lo..=hi).suffix(unit)).changed() {
                set(&mut app.proj, &k(l), &format!("{x:.1}"));
            }
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    let midi = check(app, ui, "MIDI Click", "MIDI Click", true);
    egui::Grid::new("metro-midi").num_columns(4).spacing([12.0, 3.0]).show(ui, |ui| {
        ui.label("");
        for h in ["Channel", "Note", "Velocity"] {
            ui.label(egui::RichText::new(h).small());
        }
        ui.end_row();
        for l in LEVELS {
            check(app, ui, &format!("MIDI {l}"), l, midi);
            num(&mut app.proj, ui, &format!("MIDI {l} Channel"), 1.0, 16.0, midi, plain);
            num(&mut app.proj, ui, &format!("MIDI {l} Note"), 0.0, 127.0, midi, note_name);
            num(&mut app.proj, ui, &format!("MIDI {l} Velocity"), 1.0, 127.0, midi, plain);
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        ui.label("Port:");
        let cur = app.proj.get(&k("MIDI Port:"));
        let mut ports = vec!["All".to_string()];
        ports.extend(app.hub.outs.ports());
        ui.add_enabled_ui(midi, |ui| {
            egui::ComboBox::from_id_salt("metro-port").selected_text(crate::live::port_label(&cur)).width(240.0).show_ui(ui, |ui| {
                for p in &ports {
                    if ui.selectable_label(cur == *p, crate::live::port_label(p)).clicked() {
                        set(&mut app.proj, &k("MIDI Port:"), p);
                    }
                }
            });
        });
    });
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Klopfgeist is Studio's own click; Group clicks the beat groups of compound meters (6/8, 9/8, 12/8). The source and the last Simple mode are kept for every project, as in Logic.").small().color(crate::look::text_dim()));
}

/// The MIDI Click: the notes for a click line (0 bar, 1 beat, 2 division; syncmidi.rs finds
/// them while playing) to the chosen port.
pub fn midi_click(app: &mut App, which: u8, group: bool) {
    let v = &app.proj;
    if !v.on(&k("MIDI Click")) {
        return;
    }
    let cfg = crate::audio::ClickCfg { bar: v.on(&k("MIDI Bar")), group: v.on(&k("MIDI Group")), beat: v.on(&k("MIDI Beat")), division: v.on(&k("MIDI Division")), poly: v.on(&k("Polyphonic clicks")), ..Default::default() };
    let port = match v.get(&k("MIDI Port:")).as_str() {
        "" | "All" => "*".to_string(),
        p => p.to_string(),
    };
    for lv in cfg.levels(which == 0, which == 0 || group, which <= 1) {
        let l = LEVELS[lv];
        let ch = (v.num(&k(&format!("MIDI {l} Channel"))).clamp(1.0, 16.0) as u8 - 1) & 15;
        let note = v.num(&k(&format!("MIDI {l} Note"))).clamp(0.0, 127.0) as u8;
        let vel = v.num(&k(&format!("MIDI {l} Velocity"))).clamp(1.0, 127.0) as u8;
        app.hub.outs.send(&port, &[0x90 | ch, note, vel]);
        app.hub.outs.send(&port, &[0x80 | ch, note, 0]);
    }
}

/// Click only while recording (Simple mode off, Click while recording on, the button off): the
/// button shows grey with a purple symbol, as Logic's.
pub fn recording_only(app: &App, metronome_on: bool) -> bool {
    let v = &app.proj;
    !metronome_on && !v.on(&k("Simple mode")) && v.on(&k("Click while recording"))
}

/// The Metronome button's Control-click (right-click) menu.
pub fn menu(app: &mut App, resp: &egui::Response) {
    resp.context_menu(|ui| {
        for l in ["Simple mode", "Click while recording", "Only during count-in", "Polyphonic clicks", "Audio Click (Klopfgeist)", "MIDI Click"] {
            let enabled = match l {
                "Click while recording" => !app.proj.on(&k("Simple mode")),
                "Only during count-in" => !app.proj.on(&k("Simple mode")) && app.proj.on(&k("Click while recording")),
                _ => true,
            };
            let mut on = app.proj.on(&k(l));
            let text = match l {
                "Simple mode" => "Simple Mode",
                "Click while recording" => "Click While Recording",
                "Only during count-in" => "Only During Count-in",
                "Polyphonic clicks" => "Polyphonic Clicks",
                "Audio Click (Klopfgeist)" => "Audio Click",
                _ => "MIDI Click",
            };
            if ui.add_enabled(enabled, egui::Checkbox::new(&mut on, text)).changed() {
                set(&mut app.proj, &k(l), if on { "1" } else { "0" });
                remember(app, l, on);
                crate::projsettings::apply(app);
                app.dirty = true;
            }
            if l == "Only during count-in" {
                // (Click While Playing: what the button turns on and off; the button turns
                // purple)
                let mut on = app.engine.lock().map(|e| e.metronome).unwrap_or(false);
                if ui.add_enabled(!app.proj.on(&k("Simple mode")), egui::Checkbox::new(&mut on, "Click While Playing")).changed() {
                    if let Ok(mut e) = app.engine.lock() {
                        e.metronome = on;
                    }
                }
            }
            if l == "Polyphonic clicks" {
                ui.separator();
            }
        }
        ui.separator();
        if ui.button("Metronome Settings…").clicked() {
            app.pf.settings = Some("Metronome".into());
            ui.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::ClickCfg;

    #[test]
    fn note_names_round_trip() {
        assert_eq!(note_name(60.0), "C3");
        assert_eq!(note_name(37.0), "C♯1");
        assert_eq!(parse_note("C3"), Some(60.0));
        assert_eq!(parse_note("c#1"), Some(37.0));
        assert_eq!(parse_note("93"), Some(93.0));
    }

    #[test]
    fn click_levels() {
        let c = ClickCfg { division: true, group: true, ..Default::default() };
        // one note at a time: the bar's
        assert_eq!(c.levels(true, true, true), vec![0]);
        // polyphonic: bar, group, beat together
        let p = ClickCfg { poly: true, ..c };
        assert_eq!(p.levels(true, true, true), vec![0, 1, 2]);
        assert_eq!(p.levels(false, false, true), vec![2]);
        assert_eq!(p.levels(false, false, false), vec![3]);
        // bar off: the bar line clicks as a group or beat
        let nb = ClickCfg { bar: false, ..c };
        assert_eq!(nb.levels(true, true, true), vec![1]);
    }

    #[test]
    fn values_feed_the_click() {
        let mut v = Values::default();
        set(&mut v, &k("Bar Note"), "60");
        set(&mut v, &k("Polyphonic clicks"), "1");
        let c = crate::projsettings::click_cfg(&v);
        assert_eq!(c.notes[0], 60);
        assert!(c.poly && !c.group);
        assert_eq!(c.vels, [112, 100, 100, 88]);
    }
}
