//! Logic's New Tracks dialog (⌥⌘N, the + over the track headers): the Audio / MIDI / Pattern /
//! Session Player buttons at the top with their choices (Mic or Line, Guitar or Bass; Software
//! Instrument, External MIDI; Drummer, Bass Player, Keyboard Player), the Details area behind
//! its disclosure arrow (searchable Audio Input and Output with Ascending, the devices with an
//! arrow to the Audio settings, Load Default Patch, Open Browser, Input Monitoring, Record
//! Enable; Instrument, Multi-timbral; Use External Instrument plug-in, MIDI Destination with
//! Ascending; Style, Use Default Chord Progression, Open Editor), the number of tracks, Create.
//! The track key commands and the Add button's modifier clicks use the same Details.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::trackmix::{External, RegionType};
use lg_core::TrackKind;

use crate::app::App;
use crate::audio::Out;
use crate::look;
use crate::mixstate::Input;

#[allow(dead_code)] // (read by build.rs and tool/checklist.py)
pub const COMMANDS: &[&str] = &["New Empty Track of Focused Type", "New Empty Track of Opposite Type", "New Track Like Selected"];

/// The card chosen (kept for the older callers: the control bar and key commands set it).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    #[default]
    Instrument,
    Pattern,
    Drummer,
    Bass,
    Keys,
    Audio,
    Guitar,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Main {
    Audio,
    #[default]
    Midi,
    Pattern,
    Player,
}

/// What the Instrument pop-up loads.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Inst {
    #[default]
    DefaultPatch,
    Empty,
    Plugin(String),
}

/// The New Tracks dialog's choices (kept between uses; the key commands read them).
#[derive(Clone, Debug, PartialEq)]
pub struct Details {
    pub open: bool,
    pub main: Main,
    /// Which choice under the type button (Mic or Line / Guitar or Bass; Software Instrument /
    /// External MIDI; Drummer / Bass Player / Keyboard Player).
    pub sub: usize,
    pub input: Input,
    pub input_ascending: bool,
    pub output: Out,
    pub output_ascending: bool,
    pub load_default_patch: bool,
    pub open_browser: bool,
    pub monitoring: bool,
    pub record_enable: bool,
    pub instrument: Inst,
    pub multi_timbral: bool,
    pub parts: u8,
    pub external_plugin: bool,
    pub midi_port: String,
    pub midi_channel: u8,
    pub midi_ascending: bool,
    pub style: usize,
    pub default_chords: bool,
    pub open_editor: bool,
    search: String,
}

impl Default for Details {
    fn default() -> Self {
        Details { open: false, main: Main::Midi, sub: 0, input: Input::Mono0, input_ascending: false, output: Out::Stereo, output_ascending: false, load_default_patch: true, open_browser: false, monitoring: false, record_enable: false, instrument: Inst::DefaultPatch, multi_timbral: false, parts: 2, external_plugin: false, midi_port: String::new(), midi_channel: 1, midi_ascending: true, style: 0, default_chords: true, open_editor: true, search: String::new() }
    }
}

impl Details {
    pub fn kind(&self) -> Kind {
        match (self.main, self.sub) {
            (Main::Audio, 1) => Kind::Guitar,
            (Main::Audio, _) => Kind::Audio,
            (Main::Midi, _) => Kind::Instrument,
            (Main::Pattern, _) => Kind::Pattern,
            (Main::Player, 1) => Kind::Bass,
            (Main::Player, 2) => Kind::Keys,
            (Main::Player, _) => Kind::Drummer,
        }
    }
    pub fn set_kind(&mut self, k: Kind) {
        (self.main, self.sub) = match k {
            Kind::Audio => (Main::Audio, 0),
            Kind::Guitar => (Main::Audio, 1),
            Kind::Instrument => (Main::Midi, if self.main == Main::Midi { self.sub.min(1) } else { 0 }),
            Kind::Pattern => (Main::Pattern, if self.main == Main::Pattern { self.sub.min(1) } else { 0 }),
            Kind::Drummer => (Main::Player, 0),
            Kind::Bass => (Main::Player, 1),
            Kind::Keys => (Main::Player, 2),
        };
    }
    /// External MIDI chosen (MIDI or Pattern).
    pub fn external(&self) -> bool {
        matches!(self.main, Main::Midi | Main::Pattern) && self.sub == 1
    }
}

fn subs(m: Main) -> &'static [&'static str] {
    match m {
        Main::Audio => &["Mic or Line", "Guitar or Bass"],
        Main::Midi | Main::Pattern => &["Software Instrument", "External MIDI"],
        Main::Player => &["Drummer", "Bass Player", "Keyboard Player"],
    }
}

/// The Session Player styles offered for the chosen player.
pub fn styles(sub: usize) -> Vec<String> {
    match sub {
        1 => lg_players::bass::BassStyle::ALL.iter().map(|s| s.id().to_string()).collect(),
        2 => lg_players::keys::KeysStyle::ALL.iter().map(|s| s.id().to_string()).collect(),
        _ => lg_players::character::DRUMMERS.iter().map(|c| format!("{} ({})", c.name, c.personality)).collect(),
    }
}

fn inputs(app: &App) -> Vec<(Input, String)> {
    let mut v = vec![(Input::None, "No Input".to_string())];
    for k in 0..8 {
        v.push((if k == 0 { Input::Mono0 } else { Input::Mono(k) }, crate::mixext::io_label(app, crate::mixext::Io::Input(k), true)));
    }
    for k in 0..4 {
        v.push((Input::Stereo(2 * k), format!("Input {}-{}", 2 * k + 1, 2 * k + 2)));
    }
    for b in 0..8 {
        v.push((Input::Bus(b), crate::mixext::io_label(app, crate::mixext::Io::Bus(b), true)));
    }
    v
}

fn outputs(app: &App) -> Vec<(Out, String)> {
    let mut v = vec![(Out::Stereo, "Stereo Out".to_string()), (Out::Off, "No Output".to_string())];
    for b in 0..16 {
        v.push((Out::Bus(b), crate::mixext::io_label(app, crate::mixext::Io::Bus(b), true)));
    }
    v
}

fn midi_destinations(app: &App) -> Vec<(String, String)> {
    let mut v = vec![(String::new(), crate::midiout::VIRTUAL.to_string())];
    for p in app.hub.outs.ports() {
        if p != crate::midiout::VIRTUAL {
            v.push((p.clone(), p));
        }
    }
    v
}

/// A pop-up with a search field at its top; the choice made.
pub fn searchable<T: Clone + PartialEq>(ui: &mut egui::Ui, current: &T, items: &[(T, String)], search: &mut String) -> Option<T> {
    let shown = items.iter().find(|x| x.0 == *current).map_or("—", |x| x.1.as_str());
    let mut pick = None;
    ui.menu_button(format!("{shown}  ⌄"), |ui| {
        ui.set_min_width(220.0);
        ui.add(egui::TextEdit::singleline(search).hint_text("Search").desired_width(200.0)).request_focus();
        ui.separator();
        let q = search.to_lowercase();
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for (v, name) in items.iter().filter(|x| q.is_empty() || x.1.to_lowercase().contains(&q)) {
                if ui.add(egui::Button::selectable(v == current, name.as_str())).clicked() {
                    pick = Some(v.clone());
                    ui.close();
                }
            }
        });
    });
    if pick.is_some() {
        search.clear();
    }
    pick
}

fn instruments() -> Vec<(Inst, String)> {
    let mut v = vec![(Inst::DefaultPatch, "Default Patch".to_string()), (Inst::Empty, "Empty Channel Strip".to_string())];
    for i in lg_synth::list() {
        if let Some(n) = i.logic_names.first() {
            v.push((Inst::Plugin(n.to_string()), n.to_string()));
        }
    }
    v
}

pub fn sheet(app: &mut App, ctx: &egui::Context) {
    if !app.new_track_open {
        return;
    }
    // (callers that set the kind directly: the dialog opens on it)
    if app.tm.nt.kind() != app.new_track_kind {
        let k = app.new_track_kind;
        app.tm.nt.set_kind(k);
    }
    let mut d = app.tm.nt.clone();
    let mut create = false;
    let mut cancel = false;
    let mut open_audio_settings = false;
    let screen = ctx.content_rect();
    egui::Window::new("New Track").id(egui::Id::unique("new-track")).title_bar(false).resizable(false).collapsible(false).default_pos(pos2(screen.center().x - 330.0, screen.min.y + 80.0)).show(ctx, |ui| {
        ui.set_width(640.0);
        let (r, _) = ui.allocate_exact_size(vec2(640.0, 176.0), Sense::hover());
        let p = ui.painter().clone();
        p.text(pos2(r.center().x, r.min.y + 16.0), Align2::CENTER_CENTER, "Choose a track type", look::semibold(14.0), look::text());
        // the four type buttons
        let bw = 148.0;
        for (k, (m, name)) in [(Main::Audio, "Audio"), (Main::Midi, "MIDI"), (Main::Pattern, "Pattern"), (Main::Player, "Session Player")].into_iter().enumerate() {
            let br = Rect::from_min_size(pos2(r.min.x + 16.0 + k as f32 * (bw + 6.0), r.min.y + 36.0), vec2(bw, 78.0));
            let on = d.main == m;
            let resp = ui.interact(br, ui.scope_id().with(("nt-main", k)), Sense::click());
            p.rect_filled(br, 10.0, if on { look::accent().gamma_multiply(0.35) } else if resp.hovered() { look::button_on() } else { look::group_bg() });
            if on {
                p.rect_stroke(br, 10.0, Stroke::new(2.0, look::accent()), egui::StrokeKind::Inside);
            }
            main_icon(&p, Rect::from_center_size(br.center() - vec2(0.0, 10.0), vec2(34.0, 34.0)), m);
            p.text(pos2(br.center().x, br.max.y - 14.0), Align2::CENTER_CENTER, name, look::regular(12.0), look::text());
            if resp.clicked() && !on {
                d.main = m;
                d.sub = 0;
                d.style = 0;
            }
            if resp.double_clicked() {
                create = true;
            }
        }
        // the choices under the chosen type
        let list = subs(d.main);
        let sw = 150.0;
        let x0 = r.center().x - list.len() as f32 * (sw + 6.0) / 2.0;
        for (k, name) in list.iter().enumerate() {
            let sr = Rect::from_min_size(pos2(x0 + k as f32 * (sw + 6.0), r.min.y + 126.0), vec2(sw, 26.0));
            let on = d.sub == k;
            let resp = ui.interact(sr, ui.scope_id().with(("nt-sub", k)), Sense::click());
            p.rect_filled(sr, 7.0, if on { look::accent() } else if resp.hovered() { look::button_on() } else { look::group_bg() });
            p.text(sr.center(), Align2::CENTER_CENTER, *name, look::regular(12.0), if on { Color32::WHITE } else { look::text() });
            if resp.clicked() {
                d.sub = k;
                d.style = 0;
            }
        }
        p.hline(r.min.x + 12.0..=r.max.x - 12.0, r.max.y - 6.0, Stroke::new(1.0, look::line()));
        // Details: the disclosure arrow
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            if ui.add(egui::Button::new(if d.open { "▾  Details" } else { "▸  Details" }).frame(false)).clicked() {
                d.open = !d.open;
            }
            if !d.open {
                ui.label(egui::RichText::new(summary(&d)).color(look::text_dim()));
            }
        });
        if d.open {
            ui.indent("nt-details", |ui| details_ui(app, ui, &mut d, &mut open_audio_settings));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label("Number of tracks:");
            let mut n = app.new_track_count.max(1);
            if ui.add(egui::DragValue::new(&mut n).range(1..=64)).changed() {
                app.new_track_count = n;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(12.0);
                if ui.add(egui::Button::new(egui::RichText::new("Create").color(Color32::WHITE)).fill(look::accent()).min_size(vec2(88.0, 26.0))).clicked() {
                    create = true;
                }
                if ui.add(egui::Button::new("Cancel").min_size(vec2(88.0, 26.0))).clicked() {
                    cancel = true;
                }
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !egui::Popup::is_any_open(ui.ctx()) {
            cancel = true;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !egui::Popup::is_any_open(ui.ctx()) {
            create = true;
        }
    });
    app.new_track_kind = d.kind();
    app.tm.nt = d.clone();
    if open_audio_settings {
        app.settings_tab = "Audio".into();
        app.settings_sub = "Devices".into();
        app.show_settings = true;
    }
    if create {
        app.new_track_open = false;
        let n = app.new_track_count.max(1) as usize;
        create_tracks(app, ctx, &d, n);
    } else if cancel {
        app.new_track_open = false;
    }
}

fn summary(d: &Details) -> String {
    match (d.main, d.sub) {
        (Main::Audio, _) => format!("Input: {}, Output: {}", crate::mixstate::in_label(d.input), crate::mixstate::out_label(d.output)),
        (Main::Player, _) => format!("Style: {}", styles(d.sub).get(d.style).cloned().unwrap_or_default()),
        (_, 1) => format!("MIDI Destination: {} channel {}", if d.midi_port.is_empty() { crate::midiout::VIRTUAL } else { &d.midi_port }, d.midi_channel),
        _ => format!("Instrument: {}", match &d.instrument { Inst::DefaultPatch => "Default Patch", Inst::Empty => "Empty Channel Strip", Inst::Plugin(n) => n }),
    }
}

fn details_ui(app: &mut App, ui: &mut egui::Ui, d: &mut Details, open_audio_settings: &mut bool) {
    let mut search = std::mem::take(&mut d.search);
    match d.main {
        Main::Audio => {
            ui.horizontal(|ui| {
                ui.label("Audio Input:");
                if let Some(v) = searchable(ui, &d.input, &inputs(app), &mut search) {
                    d.input = v;
                }
                ui.checkbox(&mut d.input_ascending, "Ascending");
            });
            ui.horizontal(|ui| {
                ui.label("Audio Output:");
                if let Some(v) = searchable(ui, &d.output, &outputs(app), &mut search) {
                    d.output = v;
                }
                ui.checkbox(&mut d.output_ascending, "Ascending");
            });
            // the devices in use, with an arrow to the Audio settings
            let dev = |key: &str| {
                let v = app.logic_settings.get(key);
                if v.is_empty() { "System Default".to_string() } else { v }
            };
            for (label, key) in [("Input Device", "Audio / Devices / Input Device:"), ("Output Device", "Audio / Devices / Output Device:")] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{label}: {}", dev(key))).color(look::text_dim()));
                    if ui.add(egui::Button::new("→").frame(false)).on_hover_text("Open Settings ▸ Audio ▸ Devices").clicked() {
                        *open_audio_settings = true;
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.checkbox(&mut d.load_default_patch, "Load Default Patch").on_hover_text("Off: an empty track with no plug-ins");
                ui.checkbox(&mut d.open_browser, "Open Browser");
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut d.monitoring, "Input Monitoring");
                ui.checkbox(&mut d.record_enable, "Record Enable");
            });
        }
        Main::Midi | Main::Pattern if d.sub == 0 => {
            ui.horizontal(|ui| {
                ui.label("Instrument:");
                if let Some(v) = searchable(ui, &d.instrument, &instruments(), &mut search) {
                    d.instrument = v;
                }
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut d.multi_timbral, "Multi-timbral");
                ui.add_enabled(d.multi_timbral, egui::DragValue::new(&mut d.parts).range(2..=16).suffix(" parts"));
            });
            ui.horizontal(|ui| {
                ui.label("Audio Output:");
                if let Some(v) = searchable(ui, &d.output, &outputs(app), &mut search) {
                    d.output = v;
                }
                ui.checkbox(&mut d.output_ascending, "Ascending");
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut d.open_browser, "Open Browser");
                if d.main == Main::Pattern {
                    ui.checkbox(&mut d.open_editor, "Open Editor");
                }
            });
        }
        Main::Midi | Main::Pattern => {
            // External MIDI (a Pattern track always plays through the External Instrument plug-in)
            let mut always = true;
            if d.main == Main::Pattern {
                ui.add_enabled(false, egui::Checkbox::new(&mut always, "Use External Instrument plug-in"));
            } else {
                ui.checkbox(&mut d.external_plugin, "Use External Instrument plug-in");
            }
            ui.horizontal(|ui| {
                ui.label("MIDI Destination:");
                if let Some(v) = searchable(ui, &d.midi_port, &midi_destinations(app), &mut search) {
                    d.midi_port = v;
                }
                ui.label("Channel:");
                ui.add(egui::DragValue::new(&mut d.midi_channel).range(1..=16));
            });
            ui.checkbox(&mut d.midi_ascending, "Ascending").on_hover_text("Each new track takes the next MIDI channel");
            if d.main == Main::Pattern {
                ui.checkbox(&mut d.open_editor, "Open Editor");
            }
        }
        Main::Player => {
            let list = styles(d.sub);
            ui.horizontal(|ui| {
                ui.label("Style:");
                egui::ComboBox::from_id_salt("nt-style").width(240.0).selected_text(list.get(d.style).cloned().unwrap_or_default()).show_ui(ui, |ui| {
                    for (k, s) in list.iter().enumerate() {
                        ui.selectable_value(&mut d.style, k, s);
                    }
                });
            });
            ui.checkbox(&mut d.default_chords, "Use Default Chord Progression for New Regions");
            ui.horizontal(|ui| {
                ui.checkbox(&mut d.open_editor, "Open Editor");
                ui.checkbox(&mut d.open_browser, "Open Browser");
            });
        }
    }
    d.search = search;
}

fn ascend_input(i: Input, k: usize) -> Input {
    match i {
        Input::Mono0 if k == 0 => Input::Mono0,
        Input::Mono0 => Input::Mono(k),
        Input::Mono(n) => Input::Mono(n + k),
        Input::Stereo(n) => Input::Stereo(n + 2 * k),
        Input::Bus(b) => Input::Bus(b + k),
        Input::None => Input::None,
    }
}

fn ascend_output(o: Out, k: usize) -> Out {
    match o {
        Out::Bus(b) => Out::Bus(b + k),
        o => o,
    }
}

fn push_track(app: &mut App, t: lg_core::Track, program: u8) -> usize {
    let Some(p) = app.project.as_mut() else { return 0 };
    p.tracks.push(t);
    app.programs.push(program);
    let i = p.tracks.len() - 1;
    crate::trackarea::sync(app);
    app.selected_track = Some(i);
    i
}

/// Creates `n` tracks as the Details say; the new tracks' indexes.
pub fn create_tracks(app: &mut App, ctx: &egui::Context, d: &Details, n: usize) -> Vec<usize> {
    let mut made = Vec::new();
    if app.project.is_none() {
        return made;
    }
    match d.main {
        Main::Player => {
            for _ in 0..n {
                match d.sub {
                    0 => {
                        app.new_drummer();
                        let t = app.selected_track.unwrap_or(0);
                        if let Some(c) = lg_players::character::DRUMMERS.get(d.style).filter(|_| d.style > 0) {
                            let seed = app.drum_params.get(&(t, 0)).map_or(1, |p| p.seed);
                            app.drum_params.insert((t, 0), crate::drummer::Params { seed, ..crate::drummer::Params::for_character(c) });
                            if let Some(r) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.first_mut()) {
                                r.name = format!("Drummer - {}", c.name);
                            }
                            crate::drummer::regenerate(app, t, 0);
                        }
                    }
                    k => {
                        app.spx.tonic_only = !d.default_chords;
                        crate::players::new_track(app, k == 2);
                        let t = app.selected_track.unwrap_or(0);
                        if let Some(pl) = app.players.get_mut(&(t, 0)) {
                            match pl {
                                crate::players::Player::Bass(b) => b.style = lg_players::bass::BassStyle::ALL[d.style.min(lg_players::bass::BassStyle::ALL.len() - 1)],
                                crate::players::Player::Keys(b) => b.style = lg_players::keys::KeysStyle::ALL[d.style.min(lg_players::keys::KeysStyle::ALL.len() - 1)],
                            }
                            let name = pl.region_name();
                            if let Some(r) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.first_mut()) {
                                r.name = name;
                            }
                            crate::players::regenerate(app, t, 0);
                        }
                    }
                }
                let t = app.selected_track.unwrap_or(0);
                if let Some(s) = crate::tm::set_mut(app, t) {
                    s.default_region = RegionType::SessionPlayer;
                }
                if !d.open_editor && app.bottom == crate::app::Bottom::PianoRoll {
                    app.bottom = crate::app::Bottom::None;
                }
                made.push(t);
            }
            app.after_edit();
        }
        Main::Audio => {
            app.checkpoint("New Tracks");
            for k in 0..n {
                let count = app.project.as_ref().map_or(0, |p| p.tracks.iter().filter(|t| t.kind == TrackKind::Audio).count());
                let name = if d.sub == 1 { format!("Guitar {}", k + 1) } else { format!("Audio {}", count + 1) };
                let t = push_track(app, lg_core::Track::new(&name, TrackKind::Audio), 0);
                app.mix.inputs.insert(t, if d.input_ascending { ascend_input(d.input, k) } else { d.input });
                let out = if d.output_ascending { ascend_output(d.output, k) } else { d.output };
                if out != Out::Stereo {
                    app.mix.outputs.insert(t, out);
                }
                if d.monitoring {
                    app.monitoring.insert(t);
                }
                if d.record_enable {
                    app.armed.insert(t);
                }
                if d.load_default_patch && !crate::trackops::load_default_patch(app, t) && d.sub == 1 {
                    // Guitar or Bass without a stored default: stompboxes and an amp
                    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                        tr.plugins.push(crate::mixstate::new_plugin("Pedalboard"));
                        tr.plugins.push(crate::mixstate::new_plugin("Amp Designer"));
                    }
                }
                made.push(t);
            }
            app.after_edit();
        }
        Main::Midi | Main::Pattern => {
            app.checkpoint("New Tracks");
            let pattern = d.main == Main::Pattern;
            for k in 0..n {
                let count = app.project.as_ref().map_or(0, |p| p.tracks.iter().filter(|t| t.kind != TrackKind::Audio).count());
                let mut nt = lg_core::Track::new(&format!("Inst {}", count + 1), TrackKind::Instrument);
                if pattern {
                    nt.extra.set.default_region = RegionType::Pattern;
                }
                if d.external() {
                    let ch = if d.midi_ascending { (d.midi_channel as usize - 1 + k) % 16 + 1 } else { d.midi_channel as usize } as u8;
                    nt.name = format!("MIDI {ch}");
                    nt.extra.set.external = Some(External { port: d.midi_port.clone(), channel: ch, use_plugin: pattern || d.external_plugin, ..Default::default() });
                    made.push(push_track(app, nt, 0));
                    continue;
                }
                let t = push_track(app, nt, 0);
                made.push(t);
                let out = if d.output_ascending { ascend_output(d.output, k) } else { d.output };
                if out != Out::Stereo {
                    app.mix.outputs.insert(t, out);
                }
                match &d.instrument {
                    Inst::Empty => {}
                    Inst::Plugin(name) => {
                        if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                            let mut u = crate::mixstate::new_plugin(name);
                            u.slot = 2;
                            tr.plugins.push(u);
                            tr.name = name.clone();
                        }
                    }
                    Inst::DefaultPatch => {
                        if !crate::trackops::load_default_patch(app, t) {
                            if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                                tr.name = "Deluxe Classic".into();
                            }
                            if let Some(pg) = app.programs.get_mut(t) {
                                *pg = 4;
                            }
                        }
                    }
                }
                // Multi-timbral: further tracks on the same channel strip, one MIDI channel each
                if d.multi_timbral {
                    crate::trackarea::sync(app);
                    let (id, name) = app.project.as_ref().map(|p| (p.tracks[t].extra.id, p.tracks[t].name.clone())).unwrap_or((0, String::new()));
                    if let Some(s) = crate::tm::set_mut(app, t) {
                        s.midi.out_channel = 1;
                    }
                    for part in 1..d.parts.max(2) {
                        let mut pt = lg_core::Track::new(&format!("{name} {}", part + 1), TrackKind::Instrument);
                        pt.extra.set.same_strip = Some(id);
                        pt.extra.set.midi.out_channel = part + 1;
                        pt.extra.set.default_region = if pattern { RegionType::Pattern } else { RegionType::Midi };
                        made.push(push_track(app, pt, 0));
                    }
                }
            }
            app.after_edit();
            if pattern {
                let at = app.position() as u32;
                for &t in &made {
                    crate::stepseq::new_pattern_region(app, ctx, t, at);
                }
                if d.open_editor {
                    if let Some(&t) = made.last() {
                        app.selected_track = Some(t);
                        app.selected_region = Some((t, 0));
                        app.key_command(ctx, "Open Step Sequencer");
                    }
                }
                app.after_edit();
            }
        }
    }
    if !made.is_empty() {
        app.selected_track = made.last().copied();
        app.ta.selected = made.clone();
    }
    if d.open_browser {
        app.show_browser = true;
    }
    made
}

/// The Add Tracks button with a modifier, a double-click under the headers, and their key
/// commands: an empty track of the focused track's type, of the opposite type, or one like
/// the selected track — with the default patch when New Tracks was last set to load it.
pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    let focused_audio = app.selected_track.and_then(|t| app.project.as_ref()?.tracks.get(t)).is_some_and(|t| t.kind == TrackKind::Audio);
    let audio = match name {
        "New Empty Track of Focused Type" | "New Track Like Selected" => focused_audio,
        "New Empty Track of Opposite Type" => !focused_audio,
        _ => return false,
    };
    let last = app.tm.nt.clone();
    let default_patch = last.load_default_patch && last.instrument == Inst::DefaultPatch;
    let d = Details { main: if audio { Main::Audio } else { Main::Midi }, sub: 0, load_default_patch: default_patch, instrument: if default_patch { Inst::DefaultPatch } else { Inst::Empty }, multi_timbral: false, open_browser: false, ..last };
    // under the selected track
    let before = app.selected_track;
    let made = create_tracks(app, ctx, &d, 1);
    if let (Some(sel), Some(&new)) = (before, made.first()) {
        let to = app.project.as_ref().map_or(new, |p| lg_core::arrange::group(&p.tracks, sel).end);
        if to < new {
            if let Some(p) = app.project.as_mut() {
                let t = p.tracks.remove(new);
                p.tracks.insert(to, t);
            }
            app.after_edit();
            app.selected_track = Some(to);
            app.ta.selected = vec![to];
        }
    }
    !made.is_empty()
}

fn main_icon(p: &egui::Painter, r: Rect, m: Main) {
    use crate::glyphs::{br, ph};
    let (green, yellow, blue) = (Color32::from_rgb(0x52, 0xc0, 0x3c), Color32::from_rgb(0xe3, 0xb3, 0x2b), Color32::from_rgb(0x3d, 0x8f, 0xe0));
    let (icon, col) = match m {
        Main::Audio => (ph::MICROPHONE, blue),
        Main::Midi => (ph::PIANO_KEYS, green),
        Main::Pattern => (ph::SQUARES_FOUR, green),
        Main::Player => (br::DRUM, yellow),
    };
    crate::glyphs::paint(p, r, icon, 32.0, col);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_choices() {
        let mut d = Details::default();
        assert_eq!(d.kind(), Kind::Instrument);
        d.set_kind(Kind::Guitar);
        assert_eq!((d.main, d.sub, d.kind()), (Main::Audio, 1, Kind::Guitar));
        d.set_kind(Kind::Keys);
        assert_eq!((d.main, d.sub), (Main::Player, 2));
        assert_eq!(subs(Main::Audio), ["Mic or Line", "Guitar or Bass"]);
        assert_eq!(subs(Main::Midi), ["Software Instrument", "External MIDI"]);
        assert_eq!(subs(Main::Pattern), ["Software Instrument", "External MIDI"]);
        assert!(!styles(0).is_empty() && !styles(1).is_empty() && !styles(2).is_empty());
        assert_eq!(ascend_input(Input::Mono0, 2), Input::Mono(2));
        assert_eq!(ascend_input(Input::Stereo(0), 1), Input::Stereo(2));
        assert_eq!(ascend_output(Out::Bus(3), 2), Out::Bus(5));
        assert_eq!(ascend_output(Out::Stereo, 2), Out::Stereo);
    }

    #[test]
    fn audio_tracks_take_the_details() {
        let mut app = crate::tm::test_app();
        let ctx = egui::Context::default();
        let n = app.project.as_ref().unwrap().tracks.len();
        let d = Details { main: Main::Audio, input: Input::Mono(1), input_ascending: true, output: Out::Bus(2), output_ascending: true, monitoring: true, record_enable: true, load_default_patch: false, ..Default::default() };
        let made = create_tracks(&mut app, &ctx, &d, 3);
        assert_eq!(made, vec![n, n + 1, n + 2]);
        assert_eq!(app.mix.inputs[&(n + 2)], Input::Mono(3), "Ascending: the next input each");
        assert_eq!(app.mix.outputs[&(n + 1)], Out::Bus(3));
        assert!(app.monitoring.contains(&n) && app.armed.contains(&(n + 2)));
        let p = app.project.as_ref().unwrap();
        assert!(p.tracks[n].plugins.is_empty(), "no default patch: an empty track");
        assert_eq!(p.tracks[n].kind, TrackKind::Audio);
        app.undo();
        assert_eq!(app.project.as_ref().unwrap().tracks.len(), n);
    }

    #[test]
    fn external_midi_and_multi_timbral_tracks() {
        let mut app = crate::tm::test_app();
        let ctx = egui::Context::default();
        let n = app.project.as_ref().unwrap().tracks.len();
        let d = Details { main: Main::Midi, sub: 1, midi_channel: 15, midi_ascending: true, midi_port: "Synth".into(), ..Default::default() };
        let made = create_tracks(&mut app, &ctx, &d, 3);
        let p = app.project.as_ref().unwrap();
        let chans: Vec<u8> = made.iter().map(|&t| p.tracks[t].extra.set.external.as_ref().unwrap().channel).collect();
        assert_eq!(chans, vec![15, 16, 1]);
        assert_eq!(p.tracks[n].extra.set.external.as_ref().unwrap().port, "Synth");
        let d = Details { main: Main::Midi, sub: 0, multi_timbral: true, parts: 4, instrument: Inst::Plugin("Sampler".into()), ..Default::default() };
        let made = create_tracks(&mut app, &ctx, &d, 1);
        assert_eq!(made.len(), 4);
        let p = app.project.as_ref().unwrap();
        let owner = p.tracks[made[0]].extra.id;
        assert!(owner != 0 && made[1..].iter().all(|&t| p.tracks[t].extra.set.same_strip == Some(owner)));
        assert_eq!(p.tracks[made[3]].extra.set.midi.out_channel, 4);
        assert!(p.tracks[made[0]].plugins.iter().any(|u| u.slot == 2 && u.name == "Sampler"));
    }

    #[test]
    fn empty_track_of_the_opposite_type() {
        let mut app = crate::tm::test_app();
        let ctx = egui::Context::default();
        let t = app.project.as_ref().unwrap().tracks.iter().position(|x| x.kind != TrackKind::Audio).unwrap();
        app.selected_track = Some(t);
        assert!(command(&mut app, &ctx, "New Empty Track of Opposite Type"));
        let s = app.selected_track.unwrap();
        assert_eq!(app.project.as_ref().unwrap().tracks[s].kind, TrackKind::Audio);
        assert_eq!(s, t + 1, "under the selected track");
    }
}
