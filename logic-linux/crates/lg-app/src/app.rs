//! The app: a Logic Pro-style main window (Logic Pro 13, ui-tracks-with-pianoroll.png): the
//! control bar, the Browser and Inspector on the left, the tracks area, and the editor (Piano
//! Roll) or Mixer below it. Opens .logicx projects and plays them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{pos2, vec2, Rect};
use lg_core::{Project, TrackKind};

use crate::audio::{self, Engine, PlayTrack};
use crate::look::{self, Look};

pub const CONTROL_H: f32 = 62.0;
pub const BROWSER_W: f32 = 270.0;
pub const INSPECTOR_W: f32 = 176.0;
pub const EDITOR_H: f32 = 300.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bottom {
    None,
    PianoRoll,
    Mixer,
}

/// How a SoundFont added to a track is used (the user's choice, Settings > SoundFonts).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoundFontMode {
    /// Played as it is by the SoundFont engine (Bramble's own).
    Native,
    /// Converted into a Sampler instrument (Logic's way).
    Sampler,
}

pub struct App {
    pub look: Look,
    pub buttons: kn_ui::winframe::ButtonLayout,
    pub menu_bar_width: Option<f32>,
    pub title_press_at: f64,
    pub corner: f32,
    pub project: Option<Project>,
    /// The built-in demo song is showing (no project opened).
    pub demo: bool,
    pub engine: Arc<Mutex<Engine>>,
    pub output: audio::Output,
    pub show_browser: bool,
    pub show_inspector: bool,
    pub show_control_bar: bool,
    pub bottom: Bottom,
    pub selected_track: Option<usize>,
    pub selected_region: Option<(usize, usize)>,
    /// Points per bar in the tracks area, and its scroll (bars, rows in points).
    pub bar_w: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    /// Piano Roll: points per bar, and the top row's pitch.
    pub roll_bar_w: f32,
    pub roll_top: f32,
    pub roll_scroll_x: f32,
    pub cycle: (u32, u32),
    pub message: Option<(String, f64)>,
    pub show_settings: bool,
    pub show_about: bool,
    pub soundfont_mode: SoundFontMode,
    pub browser_tab: usize,
    pub browser_search: String,
    /// The SoundFont's presets (bank, program, name), for the Browser.
    pub presets: Vec<(u16, u8, String)>,
    /// Each track's sound (GM program, as the Browser sets it), by track index.
    pub programs: Vec<u8>,
    pub drag: Option<Drag>,
    pub automated: bool,
    /// Undo / redo: the song as it was, with what changed it ("Move Region").
    pub undo: Vec<(String, Project, Vec<u8>)>,
    pub redo: Vec<(String, Project, Vec<u8>)>,
    /// Piano Roll: the selected notes (indexes in the region).
    pub selected_notes: Vec<usize>,
    /// The region's notes as they were when a note drag began (the drag moves from these).
    pub drag_notes: Vec<lg_core::events::Note>,
    /// Pane sizes (points): Browser width, track header width, editor and Mixer heights.
    pub browser_w: f32,
    pub header_w: f32,
    pub editor_h: f32,
    pub mixer_h: f32,
    /// The help tag beside the pointer while dragging (Logic's: "Cycle Range / Left: …").
    pub help_tag: Option<Vec<(String, String)>>,
    /// Where the editor (Piano Roll / Mixer) is: keys go to it while the pointer is over it.
    pub editor_rect: Rect,
    /// Copied region (⌘C / ⌘X), pasted at the playhead on the selected track (⌘V).
    pub clipboard: Option<lg_core::Region>,
}

#[derive(Clone, Copy, Debug)]
pub enum Drag {
    /// A region's right edge: its length.
    RegionEnd { track: usize, region: usize, length: u32 },
    /// Selected notes moving: where the press was (tick, pitch), copies with Option.
    Notes { from_tick: f32, from_pitch: f32, copy: bool },
    /// A region moving: which, where the press was (ticks), its start then.
    Region { track: usize, region: usize, from_tick: f32, start: u32 },
    Playhead,
    Cycle { from_tick: f32, start: (u32, u32) },
}

impl App {
    pub fn new(cc: &eframe::CreationContext, path: Option<PathBuf>) -> App {
        let automated = std::env::var_os("LG_SNAP").is_some();
        let mut look = Look::load();
        if let Ok(v) = std::env::var("LG_DARK") {
            look.dark = v != "0";
        } else if automated {
            look = Look { system_title_bar: look.system_title_bar, ..Look::default() };
        }
        look.apply(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| o.input_options.line_scroll_speed = 100.0);
        let engine = Arc::new(Mutex::new(Engine::new()));
        let output = audio::Output::open(engine.clone());
        let sf = settings_soundfont();
        {
            let mut e = engine.lock().unwrap_or_else(|e| e.into_inner());
            e.load_soundfont(sf.as_deref());
        }
        let presets = sf.or_else(audio::system_soundfont).map(|p| soundfont_presets(&p)).unwrap_or_default();
        let mut app = App {
            look,
            buttons: kn_ui::winframe::ButtonLayout::from_desktop(),
            menu_bar_width: None,
            title_press_at: -1.0,
            corner: 0.0,
            project: None,
            demo: false,
            engine,
            output,
            show_browser: true,
            show_inspector: true,
            show_control_bar: true,
            bottom: Bottom::PianoRoll,
            selected_track: Some(0),
            selected_region: None,
            bar_w: 70.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            roll_bar_w: 220.0,
            roll_top: 84.0,
            roll_scroll_x: 0.0,
            cycle: (0, 4 * 3840),
            message: None,
            show_settings: false,
            show_about: false,
            soundfont_mode: SoundFontMode::Native,
            browser_tab: 2,
            browser_search: String::new(),
            presets,
            programs: Vec::new(),
            drag: None,
            automated,
            undo: Vec::new(),
            redo: Vec::new(),
            selected_notes: Vec::new(),
            drag_notes: Vec::new(),
            browser_w: BROWSER_W,
            header_w: crate::tracks::HEADER_W,
            editor_h: EDITOR_H,
            mixer_h: 420.0,
            help_tag: None,
            editor_rect: Rect::NOTHING,
            clipboard: None,
        };
        match path {
            Some(p) => app.open_path(&p),
            None => app.set_project(crate::demo::song(), true),
        }
        app
    }

    pub fn project_title(&self) -> String {
        match &self.project {
            Some(p) if self.demo => format!("{} (demo)", p.name),
            Some(p) => format!("{}.logicx", p.name),
            None => "Untitled".into(),
        }
    }

    pub fn tell(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), 0.0));
    }

    pub fn open_path(&mut self, p: &Path) {
        match lg_core::open(p) {
            Ok(proj) => {
                let n = proj.tracks.len();
                self.set_project(proj, false);
                self.tell(format!("Opened with {n} tracks"));
            }
            Err(e) => {
                self.tell(format!("Couldn't open {}: {e}", p.display()));
                if self.project.is_none() {
                    self.set_project(crate::demo::song(), true);
                }
            }
        }
    }

    /// Shows a project and hands it to the engine.
    pub fn set_project(&mut self, p: Project, demo: bool) {
        self.programs = p.tracks.iter().map(|t| program_for(&t.name, t.kind)).collect();
        self.selected_track = (!p.tracks.is_empty()).then_some(0);
        self.selected_region = p.tracks.iter().enumerate().find(|(_, t)| !t.regions.is_empty()).map(|(i, _)| (i, 0));
        let bar = lg_core::project::bar_ticks(p.signature);
        let end = p.tracks.iter().flat_map(|t| t.regions.iter()).map(|r| r.start + r.length).max().unwrap_or(bar * 4);
        self.cycle = (0, end.max(bar));
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
        self.demo = demo;
        self.project = Some(p);
        self.load_engine();
        if let Some((t, r)) = self.selected_region {
            self.center_roll(t, r);
        }
    }

    /// The song as the engine plays it (audio files read in the background).
    pub fn load_engine(&mut self) {
        let Some(p) = &self.project else { return };
        let mut tracks = Vec::new();
        let mut next_ch = 0u8;
        for (i, t) in p.tracks.iter().enumerate() {
            let channel = if t.kind == TrackKind::Drummer {
                9
            } else {
                let c = next_ch;
                next_ch = (next_ch + 1) % 16;
                if next_ch == 9 {
                    next_ch = 10;
                }
                c
            };
            let mut notes: Vec<(u32, u32, u8, u8)> = t.regions.iter().filter(|r| !r.muted).flat_map(|r| r.notes.iter().map(|n| (n.pos, n.pos + n.len.max(1), n.pitch, n.velocity))).collect();
            notes.sort_by_key(|n| n.0);
            tracks.push(PlayTrack { channel, program: self.programs.get(i).copied().unwrap_or(0), notes, audio: Vec::new(), volume: db_to_gain(t.volume_db), pan: t.pan, mute: t.mute, solo: t.solo });
        }
        let audio_files: Vec<(usize, u32, PathBuf)> = p.tracks.iter().enumerate().flat_map(|(i, t)| t.regions.iter().filter_map(move |r| r.audio.as_ref().map(|a| (i, r.start, a.file.clone())))).collect();
        {
            let mut e = self.engine.lock().unwrap_or_else(|e| e.into_inner());
            e.all_off();
            e.tempo = p.tempo;
            e.tempo_map = p.tempo_map.clone();
            e.beats_per_bar = p.signature.0.max(1) as u32;
            e.tracks = tracks;
            e.cycle = Some(self.cycle);
            e.program_changes();
        }
        if !audio_files.is_empty() {
            let engine = self.engine.clone();
            std::thread::spawn(move || {
                for (track, start, file) in audio_files {
                    if let Some(samples) = audio::load_audio(&file) {
                        let mut e = engine.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(t) = e.tracks.get_mut(track) {
                            t.audio.push((start, Arc::new(samples)));
                        }
                    }
                }
            });
        }
    }

    fn engine(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.engine.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn playing(&self) -> bool {
        self.engine().playing
    }

    pub fn position(&self) -> f64 {
        self.engine().pos
    }

    pub fn cycle_on(&self) -> bool {
        self.engine().cycle_on
    }

    pub fn play_stop(&mut self) {
        let mut e = self.engine();
        if e.playing {
            e.playing = false;
            e.all_off();
        } else {
            if e.cycle_on {
                let a = e.cycle.map_or(0, |c| c.0);
                if e.pos < a as f64 || e.pos >= e.cycle.map_or(0, |c| c.1) as f64 {
                    e.locate(a as f64);
                }
            }
            e.playing = true;
        }
    }

    pub fn stop(&mut self) {
        let mut e = self.engine();
        if !e.playing {
            // stopped already: back to the start (Logic's second Stop)
            e.locate(0.0);
        }
        e.playing = false;
        e.all_off();
    }

    pub fn locate(&mut self, ticks: f64) {
        self.engine().locate(ticks);
    }

    pub fn toggle_cycle(&mut self) {
        let c = self.cycle;
        let mut e = self.engine();
        e.cycle = Some(c);
        e.cycle_on = !e.cycle_on;
    }

    pub fn set_cycle(&mut self, c: (u32, u32)) {
        self.cycle = c;
        self.engine().cycle = Some(c);
    }

    pub fn bar_ticks(&self) -> u32 {
        self.project.as_ref().map_or(3840, |p| lg_core::project::bar_ticks(p.signature))
    }

    /// The tempo at the playhead (the LCD).
    pub fn tempo(&self) -> f64 {
        let pos = self.position() as u32;
        self.project.as_ref().map_or(120.0, |p| p.tempo_at(pos))
    }

    /// Sets a track's sound (the Browser's instruments: the SoundFont's presets).
    pub fn set_program(&mut self, track: usize, program: u8, name: &str) {
        if let Some(p) = self.programs.get_mut(track) {
            *p = program;
        }
        if let Some(t) = self.project.as_mut().and_then(|p| p.tracks.get_mut(track)) {
            if t.kind != TrackKind::Audio && t.kind != TrackKind::Drummer {
                t.name = name.to_string();
            }
        }
        let mut e = self.engine();
        if let Some(t) = e.tracks.get_mut(track) {
            t.program = program;
        }
        e.program_changes();
    }

    /// Mute / solo / volume changed in the interface: the engine follows.
    pub fn sync_mix(&mut self) {
        let Some(p) = &self.project else { return };
        let mix: Vec<(bool, bool, f32, f32)> = p.tracks.iter().map(|t| (t.mute, t.solo, db_to_gain(t.volume_db), t.pan)).collect();
        let mut e = self.engine();
        for (t, (m, s, v, pan)) in e.tracks.iter_mut().zip(mix) {
            t.mute = m;
            t.solo = s;
            t.volume = v;
            t.pan = pan;
        }
    }

    pub fn center_roll(&mut self, track: usize, region: usize) {
        let Some(r) = self.project.as_ref().and_then(|p| p.tracks.get(track)).and_then(|t| t.regions.get(region)) else { return };
        if let (Some(lo), Some(hi)) = (r.notes.iter().map(|n| n.pitch).min(), r.notes.iter().map(|n| n.pitch).max()) {
            let mid = (lo as f32 + hi as f32) / 2.0;
            self.roll_top = (mid + 12.0).clamp(24.0, 127.0);
        }
        self.roll_scroll_x = r.start as f32 / self.bar_ticks() as f32;
    }

    pub fn open_dialog(&mut self) {
        if self.automated {
            return;
        }
        // (a .logicx package is a folder on Linux and Windows)
        if let Some(p) = kn_ui::files::FileDialog::new().set_title("Open a Logic Pro or GarageBand project (.logicx, .band)").pick_folder() {
            // (a GarageBand .band is a folder too)
            self.open_path(&p);
        }
    }

    /// Bounce: the song (the cycle area when cycling), or one track, to a WAV file.
    pub fn bounce(&mut self, only: Option<usize>) {
        let Some(p) = &self.project else { return };
        let name = match only.and_then(|t| p.tracks.get(t)) {
            Some(t) => format!("{} - {}.wav", p.name, t.name),
            None => format!("{}.wav", p.name),
        };
        let Some(path) = (if self.automated { None } else { kn_ui::files::FileDialog::new().set_title("Bounce").set_file_name(&name).add_filter("WAV", &["wav"]).save_file() }) else { return };
        let (from, to) = if self.cycle_on() { self.cycle } else { (0, p.tracks.iter().flat_map(|t| t.regions.iter()).map(|r| r.start + r.length).max().unwrap_or(3840)) };
        let mut tracks = self.engine().tracks.clone();
        if let Some(t) = only {
            for (i, tr) in tracks.iter_mut().enumerate() {
                tr.mute = i != t;
                tr.solo = false;
            }
        }
        let tempo = p.tempo;
        let map = p.tempo_map.clone();
        let sf = self.engine().soundfont.clone();
        self.tell("Bouncing…");
        std::thread::spawn(move || {
            let samples = audio::bounce(tracks, tempo, map, sf.as_deref(), from, to);
            let _ = std::fs::write(&path, audio::wav_bytes(&samples));
        });
    }

    /// Export > All MIDI Tracks as MIDI File…
    pub fn export_midi(&mut self) {
        let Some(p) = &self.project else { return };
        let Some(path) = (if self.automated { None } else { kn_ui::files::FileDialog::new().set_title("Export MIDI File").set_file_name(format!("{}.mid", p.name)).add_filter("MIDI", &["mid"]).save_file() }) else { return };
        match std::fs::write(&path, lg_core::midi::write(p)) {
            Ok(()) => self.tell(format!("Exported {}", path.file_name().unwrap_or_default().to_string_lossy())),
            Err(e) => self.tell(format!("Couldn't export: {e}")),
        }
    }

    /// Delete / Backspace: the selected notes (Piano Roll), else the selected region.
    pub fn delete_selection(&mut self) {
        let Some((t, r)) = self.selected_region else { return };
        if !self.selected_notes.is_empty() && self.bottom == Bottom::PianoRoll {
            self.checkpoint("Delete Notes");
            let mut idx = std::mem::take(&mut self.selected_notes);
            idx.sort_unstable();
            if let Some(rg) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|tr| tr.regions.get_mut(r)) {
                for i in idx.into_iter().rev() {
                    if i < rg.notes.len() {
                        rg.notes.remove(i);
                    }
                }
            }
        } else {
            self.checkpoint("Delete Region");
            if let Some(tr) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                if r < tr.regions.len() {
                    tr.regions.remove(r);
                }
            }
            self.selected_region = None;
        }
        self.after_edit();
    }

    /// Before an edit: the song as it is goes on the undo list.
    pub fn checkpoint(&mut self, what: &str) {
        let Some(p) = &self.project else { return };
        self.undo.push((what.to_string(), p.clone(), self.programs.clone()));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        let Some((what, p, progs)) = self.undo.pop() else { return };
        if let Some(cur) = self.project.take() {
            self.redo.push((what.clone(), cur, self.programs.clone()));
        }
        self.project = Some(p);
        self.programs = progs;
        self.after_edit();
        self.tell(format!("Undo {what}"));
    }

    pub fn redo(&mut self) {
        let Some((what, p, progs)) = self.redo.pop() else { return };
        if let Some(cur) = self.project.take() {
            self.undo.push((what.clone(), cur, self.programs.clone()));
        }
        self.project = Some(p);
        self.programs = progs;
        self.after_edit();
        self.tell(format!("Redo {what}"));
    }

    /// After the song changed: selections still valid, the engine follows.
    pub fn after_edit(&mut self) {
        if let Some(p) = &self.project {
            if self.selected_track.is_some_and(|t| t >= p.tracks.len()) {
                self.selected_track = None;
            }
            if self.selected_region.is_some_and(|(t, r)| p.tracks.get(t).is_none_or(|tr| r >= tr.regions.len())) {
                self.selected_region = None;
                self.selected_notes.clear();
            }
        }
        self.load_engine();
    }

    pub fn menu_label(&self, label: &str) -> String {
        match label {
            "Hide Browser" if !self.show_browser => "Show Browser".into(),
            "Hide Inspector" if !self.show_inspector => "Show Inspector".into(),
            "Show Mixer" if self.bottom == Bottom::Mixer => "Hide Mixer".into(),
            "Show Editor" if self.bottom == Bottom::PianoRoll => "Hide Editor".into(),
            "Hide Control Bar" if !self.show_control_bar => "Show Control Bar".into(),
            l if l.starts_with("Undo ") => match self.undo.last() {
                Some((w, ..)) => format!("Undo {w}"),
                None => "Can’t Undo".into(),
            },
            l if l.starts_with("Can’t Redo") || l.starts_with("Redo") => match self.redo.last() {
                Some((w, ..)) => format!("Redo {w}"),
                None => "Can’t Redo".into(),
            },
            _ => label.to_string(),
        }
    }

    pub fn command(&mut self, ctx: &egui::Context, cmd: &str) {
        match cmd {
            "About Bramble Studio" => self.show_about = true,
            "General…" | "View…" => self.show_settings = true,
            "Quit Bramble Studio" | "Close" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            "Hide Bramble Studio" | "Minimize" => crate::chrome::minimize(ctx),
            "Zoom" => {
                let m = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!m));
            }
            "Enter Full Screen" => {
                let f = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!f));
            }
            "New" | "New from Template…" => {
                self.stop();
                self.set_project(crate::demo::empty(), true);
            }
            "Open…" | "Logic Projects…" => self.open_dialog(),
            "Close Project" => {
                self.stop();
                self.set_project(crate::demo::song(), true);
            }
            "Hide Browser" => self.show_browser = !self.show_browser,
            "Hide Inspector" => self.show_inspector = !self.show_inspector,
            "Hide Control Bar" => self.show_control_bar = !self.show_control_bar,
            "Show Mixer" | "Open Mixer" => self.bottom = if self.bottom == Bottom::Mixer { Bottom::None } else { Bottom::Mixer },
            "Show Editor" | "Open Piano Roll" => self.bottom = if self.bottom == Bottom::PianoRoll { Bottom::None } else { Bottom::PianoRoll },
            "Open Main Window" => self.bottom = Bottom::PianoRoll,
            "Move Locators Forward by Cycle Length" => {
                let (a, b) = self.cycle;
                self.set_cycle((b, b + (b - a)));
            }
            "Project or Section…" => self.bounce(None),
            c if c.starts_with("Undo ") => self.undo(),
            c if c.starts_with("Can’t Redo") || c.starts_with("Redo") => self.redo(),
            "1 Track as Audio File…" => {
                let t = self.selected_track;
                self.bounce(t);
            }
            "All MIDI Tracks as MIDI File…" => self.export_midi(),
            _ => self.tell(format!("“{cmd}” isn't available yet")),
        }
    }

    /// Key presses: Logic's key commands for the focused area (keymap.rs); ⌘Z / ⇧⌘Z undo.
    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let presses: Vec<(egui::Key, egui::Modifiers)> = ctx.input(|i| i.events.iter().filter_map(|e| match e {
            egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
            _ => None,
        }).collect());
        let focus = match (self.bottom, ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| self.editor_rect.contains(p))) {
            (Bottom::Mixer, true) => crate::keymap::Focus::Mixer,
            (Bottom::PianoRoll, true) => crate::keymap::Focus::PianoRoll,
            _ => crate::keymap::Focus::Tracks,
        };
        for (key, m) in presses {
            if m.command && key == egui::Key::Z {
                if m.shift { self.redo() } else { self.undo() }
                continue;
            }
            if m.command && key == egui::Key::O {
                self.open_dialog();
                continue;
            }
            for c in crate::keymap::commands_for(key, m, focus) {
                if self.key_command(ctx, c) {
                    break;
                }
            }
        }
    }

    /// Runs a Logic key command by its name; false when Studio doesn't have it yet.
    pub fn key_command(&mut self, ctx: &egui::Context, name: &str) -> bool {
        let bar = self.bar_ticks() as f64;
        let n_tracks = self.project.as_ref().map_or(0, |p| p.tracks.len());
        match name {
            "Play or Stop" => self.play_stop(),
            "Play" => {
                if !self.playing() {
                    self.play_stop();
                }
            }
            "Stop" => self.stop(),
            "Go to Beginning" => self.locate(0.0),
            "Rewind" | "Fast Rewind" => {
                let k = if name == "Fast Rewind" { 4.0 } else { 1.0 };
                let p = self.position();
                self.locate(((p / bar).ceil() - k).max(0.0) * bar);
            }
            "Forward" | "Fast Forward" => {
                let k = if name == "Fast Forward" { 4.0 } else { 1.0 };
                let p = self.position();
                self.locate(((p / bar).floor() + k) * bar);
            }
            "Go to End of Last Region" => {
                let end = self.project.as_ref().map_or(0, |p| p.tracks.iter().flat_map(|t| t.regions.iter()).map(|r| r.start + r.length).max().unwrap_or(0));
                self.locate(end as f64);
            }
            "Toggle Cycle Mode" => self.toggle_cycle(),
            "Set Locators/Loop by Regions/Events/Marquee and Enable Cycle/Loop" | "Set Rounded Locators/Loop by Regions/Events/Marquee and Enable Cycle/Loop" => {
                let Some((t, r)) = self.selected_region else { return false };
                let Some(rg) = self.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.get(r)) else { return false };
                let mut c = (rg.start, rg.start + rg.length);
                if name.contains("Rounded") {
                    let b = bar as u32;
                    c = (c.0 / b * b, c.1.div_ceil(b) * b);
                }
                self.set_cycle(c);
                if !self.cycle_on() {
                    self.toggle_cycle();
                }
            }
            "Move Locators Forward by Cycle Length / Loop Forward by Loop Length" => {
                let (a, b) = self.cycle;
                self.set_cycle((b, b + (b - a)));
            }
            "Move Locators Backwards by Cycle Length / Loop Backwards by Loop Length" => {
                let (a, b) = self.cycle;
                let l = b - a;
                if a >= l {
                    self.set_cycle((a - l, a));
                }
            }
            "Double Cycle/Loop Length" => {
                let (a, b) = self.cycle;
                self.set_cycle((a, b + (b - a)));
            }
            "Halve Cycle/Loop Length" => {
                let (a, b) = self.cycle;
                if b - a >= 2 {
                    self.set_cycle((a, a + (b - a) / 2));
                }
            }
            "Toggle Metronome Click" => {
                let on = {
                    let mut e = self.engine();
                    e.metronome = !e.metronome;
                    e.metronome
                };
                self.tell(if on { "Metronome on" } else { "Metronome off" });
            }
            "Show/Hide Mixer" | "Open Mixer…" => self.bottom = if self.bottom == Bottom::Mixer { Bottom::None } else { Bottom::Mixer },
            "Show/Hide Piano Roll" | "Show/Hide Editor" | "Open Piano Roll…" => self.bottom = if self.bottom == Bottom::PianoRoll { Bottom::None } else { Bottom::PianoRoll },
            "Open Main Window…" => self.bottom = Bottom::PianoRoll,
            "Show/Hide Browser" => self.show_browser = !self.show_browser,
            "Show/Hide Inspector" => self.show_inspector = !self.show_inspector,
            "Open Settings…" => self.show_settings = true,
            "Toggle Channel Strip Mute" | "Toggle Channel Strip Solo" => {
                let Some(t) = self.selected_track else { return false };
                if let Some(tr) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                    if name.ends_with("Mute") {
                        tr.mute = !tr.mute;
                    } else {
                        tr.solo = !tr.solo;
                    }
                }
                self.sync_mix();
            }
            "Clear/Recall Solo" => {
                if let Some(p) = self.project.as_mut() {
                    for t in &mut p.tracks {
                        t.solo = false;
                    }
                }
                self.sync_mix();
            }
            "Select Previous Track" => self.selected_track = Some(self.selected_track.unwrap_or(0).saturating_sub(1)),
            "Select Next Track" => {
                if n_tracks > 0 {
                    self.selected_track = Some(self.selected_track.map_or(0, |t| t + 1).min(n_tracks - 1));
                }
            }
            "Select Previous Region/Cell on Focused Track" | "Select Next Region/Cell on Focused Track" => {
                let Some(t) = self.selected_track else { return false };
                let count = self.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(0, |tr| tr.regions.len());
                if count == 0 {
                    return true;
                }
                let cur = self.selected_region.filter(|s| s.0 == t).map(|s| s.1);
                let next = match (name.contains("Next"), cur) {
                    (true, Some(r)) => (r + 1).min(count - 1),
                    (false, Some(r)) => r.saturating_sub(1),
                    (true, None) => 0,
                    (false, None) => count - 1,
                };
                self.selected_region = Some((t, next));
                self.center_roll(t, next);
            }
            "Select Previous (Left) Channel Strip" => self.selected_track = Some(self.selected_track.unwrap_or(0).saturating_sub(1)),
            "Select Next (Right) Channel Strip" => {
                if n_tracks > 0 {
                    self.selected_track = Some(self.selected_track.map_or(0, |t| t + 1).min(n_tracks - 1));
                }
            }
            "Delete Track" => {
                let Some(t) = self.selected_track else { return false };
                self.checkpoint("Delete Track");
                if let Some(p) = self.project.as_mut() {
                    if t < p.tracks.len() {
                        p.tracks.remove(t);
                        if t < self.programs.len() {
                            self.programs.remove(t);
                        }
                    }
                }
                self.selected_region = None;
                self.after_edit();
            }
            "New Software Instrument Track" | "New Audio Track" | "New Tracks…" => {
                self.checkpoint("Create New Track");
                let audio = name == "New Audio Track";
                if let Some(p) = self.project.as_mut() {
                    let row = p.tracks.len() as u32 + 1;
                    let n = p.tracks.iter().filter(|t| (t.kind == TrackKind::Audio) == audio).count() + 1;
                    p.tracks.push(lg_core::Track { name: if audio { format!("Audio {n}") } else { "Deluxe Classic".into() }, kind: if audio { TrackKind::Audio } else { TrackKind::Instrument }, channel: 0, row, regions: Vec::new(), volume_db: 0.0, pan: 0.0, mute: false, solo: false });
                    self.programs.push(if audio { 0 } else { 4 });
                    self.selected_track = Some(p.tracks.len() - 1);
                }
                self.after_edit();
            }
            "Nudge Region/Event/Marquee Position Left by Nudge Value" | "Nudge Region/Event/Marquee Position Right by Nudge Value" => {
                // (Logic's default nudge value: a division, a 1/16)
                let step = lg_core::events::PPQ as i64 / 4;
                let d = if name.contains("Right") { step } else { -step };
                if self.bottom == Bottom::PianoRoll && !self.selected_notes.is_empty() {
                    let Some((t, r)) = self.selected_region else { return false };
                    self.checkpoint("Nudge Notes");
                    let sel = self.selected_notes.clone();
                    if let Some(rg) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|tr| tr.regions.get_mut(r)) {
                        for i in sel {
                            if let Some(n) = rg.notes.get_mut(i) {
                                n.pos = (n.pos as i64 + d).max(0) as u32;
                            }
                        }
                    }
                } else {
                    let Some((t, r)) = self.selected_region else { return false };
                    self.checkpoint("Nudge Region");
                    if let Some(rg) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|tr| tr.regions.get_mut(r)) {
                        let nd = (rg.start as i64 + d).max(0) - rg.start as i64;
                        rg.start = (rg.start as i64 + nd) as u32;
                        for n in &mut rg.notes {
                            n.pos = (n.pos as i64 + nd).max(0) as u32;
                        }
                    }
                }
                self.after_edit();
            }
            n if n.starts_with("Transpose KeySignature/Chord/Region/Event") || n.starts_with("Transpose Region/Event") => {
                let up = n.contains('+');
                let amount: i32 = if n.contains("12 Semitone") { 12 } else { 1 };
                let d = if up { amount } else { -amount };
                let Some((t, r)) = self.selected_region else { return false };
                self.checkpoint("Transpose");
                let sel = self.selected_notes.clone();
                let notes_only = self.bottom == Bottom::PianoRoll && !sel.is_empty();
                if let Some(rg) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|tr| tr.regions.get_mut(r)) {
                    for (i, note) in rg.notes.iter_mut().enumerate() {
                        if !notes_only || sel.contains(&i) {
                            note.pitch = (note.pitch as i32 + d).clamp(0, 127) as u8;
                        }
                    }
                }
                self.after_edit();
            }
            "Repeat Regions/Cells/Events" => {
                let Some((t, r)) = self.selected_region else { return false };
                self.checkpoint("Repeat Region");
                if let Some(tr) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                    if let Some(mut copy) = tr.regions.get(r).cloned() {
                        let d = copy.length;
                        copy.start += d;
                        for n in &mut copy.notes {
                            n.pos += d;
                        }
                        tr.regions.push(copy);
                        self.selected_region = Some((t, tr.regions.len() - 1));
                    }
                }
                self.after_edit();
            }
            "Copy" | "Cut" => {
                let Some((t, r)) = self.selected_region else { return false };
                self.clipboard = self.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.get(r)).cloned();
                if name == "Cut" {
                    self.delete_selection();
                }
            }
            "Paste" => {
                // at the playhead, on the selected track (Logic)
                let (Some(t), Some(mut rg)) = (self.selected_track, self.clipboard.clone()) else { return false };
                self.checkpoint("Paste");
                let at = self.position() as u32;
                let d = at as i64 - rg.start as i64;
                rg.start = at;
                for n in &mut rg.notes {
                    n.pos = (n.pos as i64 + d).max(0) as u32;
                }
                if let Some(tr) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                    tr.regions.push(rg);
                    self.selected_region = Some((t, tr.regions.len() - 1));
                }
                self.after_edit();
            }
            "Select All" => {
                if self.bottom == Bottom::PianoRoll {
                    if let Some((t, r)) = self.selected_region {
                        let n = self.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.get(r)).map_or(0, |rg| rg.notes.len());
                        self.selected_notes = (0..n).collect();
                    }
                } else {
                    return false;
                }
            }
            "Zoom Horizontal In" => self.bar_w = (self.bar_w * 1.25).min(1200.0),
            "Zoom Horizontal Out" => self.bar_w = (self.bar_w / 1.25).max(8.0),
            "Export Tracks as Audio Files…" => {
                let t = self.selected_track;
                self.bounce(t);
            }
            "Save" | "Save Project as…" => self.tell("Saving Logic projects isn't available yet (Export > MIDI File, Bounce work)"),
            _ => {
                let _ = ctx;
                return false;
            }
        }
        true
    }


    fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if let Some((_, t)) = &mut self.message {
            *t += ctx.input(|i| i.stable_dt) as f64;
            if *t > 3.5 {
                self.message = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        let window = ui.max_rect();
        let integrated = !kn_ui::winframe::system_title_bar();
        if integrated {
            crate::chrome::window_edges(&ctx, window);
        }
        let r = self.corner.round() as u8;
        let p = ui.painter().clone();
        p.rect_filled(window, r, look::window_bg());
        let title_h = crate::chrome::TITLE_H;
        let title = Rect::from_min_size(window.min, vec2(window.width(), title_h));
        p.rect_filled(title, egui::CornerRadius { nw: r, ne: r, sw: 0, se: 0 }, look::bar_bg());
        ui.scope_builder(egui::UiBuilder::new().max_rect(title.shrink2(vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            if integrated {
                crate::chrome::title_bar(self, ui);
            } else {
                crate::menus::menu_bar(self, ui);
            }
        });
        let mut top = title.max.y;
        if self.show_control_bar {
            let bar = Rect::from_min_size(pos2(window.min.x, top), vec2(window.width(), CONTROL_H));
            crate::controlbar::show(self, ui, bar);
            top = bar.max.y;
        }
        let body = Rect::from_min_max(pos2(window.min.x, top + 1.0), window.max);
        let mut left = body.min.x;
        if self.show_browser {
            let w = self.browser_w.min(body.width() - 360.0 - if self.show_inspector { INSPECTOR_W } else { 0.0 }).max(200.0);
            let r = Rect::from_min_max(pos2(left, body.min.y), pos2(left + w, body.max.y));
            crate::browser::show(self, ui, r);
            left = r.max.x + 1.0;
            // its right edge: drag to resize; past the minimum it snaps closed; past the room left
            // the Inspector hides (Logic, RESIZING.txt)
            let edge = Rect::from_min_max(pos2(r.max.x - 3.0, r.min.y), pos2(r.max.x + 3.0, r.max.y));
            let e = ui.interact(edge, ui.scope_id().with("browser-edge"), egui::Sense::drag());
            if e.hovered() || e.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if let (true, Some(pp)) = (e.dragged(), e.interact_pointer_pos()) {
                let want = pp.x - r.min.x;
                if want < 150.0 {
                    self.show_browser = false;
                    self.browser_w = 320.0;
                } else {
                    self.browser_w = want.min(925.0);
                    if self.show_inspector && body.width() - self.browser_w - INSPECTOR_W < 360.0 {
                        self.show_inspector = false;
                    }
                }
            }
        }
        if self.show_inspector {
            let r = Rect::from_min_max(pos2(left, body.min.y), pos2(left + INSPECTOR_W, body.max.y));
            crate::inspector::show(self, ui, r);
            left = r.max.x + 1.0;
        }
        let main = Rect::from_min_max(pos2(left, body.min.y), body.max);
        // (the Mixer takes more of the window than an editor, as Logic's)
        let editor_h = match self.bottom {
            Bottom::None => 0.0,
            Bottom::PianoRoll => self.editor_h.clamp(120.0, main.height() - 90.0),
            Bottom::Mixer => self.mixer_h.clamp(260.0, main.height() - 90.0),
        };
        let tracks = Rect::from_min_max(main.min, pos2(main.max.x, main.max.y - editor_h - if editor_h > 0.0 { 1.0 } else { 0.0 }));
        crate::tracks::show(self, ui, tracks);
        if editor_h > 0.0 {
            let ed = Rect::from_min_max(pos2(main.min.x, main.max.y - editor_h), main.max);
            self.editor_rect = ed;
            match self.bottom {
                Bottom::PianoRoll => crate::pianoroll::show(self, ui, ed),
                Bottom::Mixer => crate::mixer::show(self, ui, ed),
                Bottom::None => {}
            }
            // the divider between the tracks and the editor: drag up or down
            let edge = Rect::from_min_max(pos2(ed.min.x, ed.min.y - 3.0), pos2(ed.max.x, ed.min.y + 3.0));
            let e = ui.interact(edge, ui.scope_id().with("editor-edge"), egui::Sense::drag());
            if e.hovered() || e.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if let (true, Some(pp)) = (e.dragged(), e.interact_pointer_pos()) {
                let h = (main.max.y - pp.y).clamp(120.0, main.height() - 60.0);
                match self.bottom {
                    Bottom::Mixer => self.mixer_h = h,
                    _ => self.editor_h = h,
                }
            }
        }
        if let Some(tag) = self.help_tag.take() {
            help_tag(&ctx, &tag);
        }
        crate::dialogs::show(self, &ctx);
        if let Some((m, _)) = &self.message {
            let at = pos2(window.center().x, window.max.y - 40.0);
            let g = p.layout_no_wrap(m.clone(), look::regular(13.0), egui::Color32::WHITE);
            let rr = Rect::from_center_size(at, g.size() + vec2(28.0, 14.0));
            p.rect_filled(rr, 10.0, egui::Color32::from_black_alpha(200));
            p.galley(rr.center() - g.size() / 2.0, g, egui::Color32::WHITE);
        }
        if self.playing() {
            ctx.request_repaint();
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        kn_ui::begin_frame(&ctx);
        // a .logicx dropped on the window opens
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if let Some(p) = dropped.first() {
            self.open_path(p);
        }
        self.keys(&ctx);
        let m = if kn_ui::winframe::system_title_bar() { 0.0 } else { kn_ui::winframe::margin() };
        let inner = ui.max_rect().shrink(m);
        self.corner = if m > 0.0 { kn_ui::winframe::RADIUS } else { 0.0 };
        let dark = !kn_ui::theme::is_light();
        if m > 0.0 {
            let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
            kn_ui::winframe::paint_shadow(&ctx, inner, self.corner, focused, dark);
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| self.frame(ui));
        if m > 0.0 {
            kn_ui::winframe::paint_outline(&ctx, inner, self.corner, dark);
        }
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    if db <= -90.0 { 0.0 } else { 10f32.powf(db / 20.0) }
}

/// A General MIDI sound for a Logic patch name (until our own instruments stand in for Apple's
/// by ID: matched-sounds plan).
pub fn program_for(name: &str, kind: TrackKind) -> u8 {
    if kind == TrackKind::Drummer {
        return 0;
    }
    let n = name.to_lowercase();
    let has = |w: &[&str]| w.iter().any(|x| n.contains(x));
    if has(&["grand", "piano", "steinway"]) {
        0
    } else if has(&["classic", "deluxe", "suitcase", "wurli", "electric piano", "e-piano", "ep", "tines", "keys"]) {
        4
    } else if has(&["clav"]) {
        7
    } else if has(&["organ", "tonewheel"]) {
        16
    } else if has(&["strum", "guitar", "acoustic"]) {
        25
    } else if has(&["bass"]) {
        33
    } else if has(&["string"]) {
        48
    } else if has(&["vocal", "choir"]) {
        52
    } else if has(&["brass", "horn"]) {
        61
    } else if has(&["pad", "dream"]) {
        89
    } else if has(&["synth", "lead", "stab"]) {
        81
    } else {
        0
    }
}

fn settings_soundfont() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let text = std::fs::read_to_string(base.join("bramble-studio/settings.txt")).ok()?;
    text.lines().find_map(|l| l.strip_prefix("soundfont=")).map(PathBuf::from).filter(|p| p.exists())
}

pub fn save_soundfont_setting(p: &Path) {
    let Some(base) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))) else { return };
    let dir = base.join("bramble-studio");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("settings.txt"), format!("soundfont={}\n", p.display()));
}

/// A SoundFont's presets (bank, program, name), sorted as General MIDI lists them.
pub fn soundfont_presets(p: &Path) -> Vec<(u16, u8, String)> {
    let Ok(mut f) = std::fs::File::open(p) else { return Vec::new() };
    let Ok(sf) = rustysynth::SoundFont::new(&mut f) else { return Vec::new() };
    let mut v: Vec<(u16, u8, String)> = sf.get_presets().iter().map(|pr| (pr.get_bank_number() as u16, pr.get_patch_number() as u8, pr.get_name().to_string())).collect();
    v.sort();
    v
}

/// Logic's help tag: a small dark box of label / value rows by the pointer while dragging.
fn help_tag(ctx: &egui::Context, rows: &[(String, String)]) {
    let Some(at) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    egui::Area::new(egui::Id::unique("help-tag")).order(egui::Order::Tooltip).fixed_pos(at + vec2(16.0, 18.0)).interactable(false).show(ctx, |ui| {
        egui::Frame::new().fill(egui::Color32::from_rgba_unmultiplied(30, 30, 32, 235)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(9, 6)).show(ui, |ui| {
            egui::Grid::new("help-tag-grid").num_columns(2).spacing([10.0, 2.0]).show(ui, |ui| {
                for (k, v) in rows {
                    ui.label(egui::RichText::new(k).font(look::semibold(11.5)).color(egui::Color32::from_gray(0xc8)));
                    ui.label(egui::RichText::new(v).font(look::regular(11.5)).color(egui::Color32::WHITE));
                    ui.end_row();
                }
            });
        });
    });
}

/// A position as Logic writes it in help tags and the LCD: "bar beat division tick" (1-based;
/// division = 1/16, ticks of 960 per quarter within it).
pub fn bbdt(tick: u32, sig: (u8, u8)) -> String {
    let beat_t = lg_core::events::PPQ * 4 / sig.1.max(1) as u32;
    let bar_t = beat_t * sig.0.max(1) as u32;
    let div_t = lg_core::events::PPQ / 4;
    let bar = tick / bar_t + 1;
    let beat = (tick % bar_t) / beat_t + 1;
    let div = (tick % beat_t) / div_t + 1;
    let t = tick % div_t + 1;
    format!("{bar} {beat} {div} {t}")
}

/// A length the same way, counting from 0 ("5 1 0 0" = five bars and a beat).
pub fn bbdt_len(ticks: u32, sig: (u8, u8)) -> String {
    let beat_t = lg_core::events::PPQ * 4 / sig.1.max(1) as u32;
    let bar_t = beat_t * sig.0.max(1) as u32;
    let div_t = lg_core::events::PPQ / 4;
    format!("{} {} {} {}", ticks / bar_t, (ticks % bar_t) / beat_t, (ticks % beat_t) / div_t, ticks % div_t)
}

/// Logic's note names (C3 = 60).
pub fn note_name(p: u8) -> String {
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", N[(p % 12) as usize], p as i32 / 12 - 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_as_logic_writes_them() {
        assert_eq!(bbdt(0, (4, 4)), "1 1 1 1");
        // bar 2 beat 2, the 4th sixteenth (the research's 40080 - 34560)
        assert_eq!(bbdt(40080 - 34560, (4, 4)), "2 2 4 1");
        assert_eq!(bbdt_len(5 * 3840 + 960, (4, 4)), "5 1 0 0");
        assert_eq!(note_name(60), "C3");
        assert_eq!(note_name(61), "C#3");
    }
}
