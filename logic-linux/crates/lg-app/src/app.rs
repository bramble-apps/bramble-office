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

    /// Logic's key commands (U.S. preset) that Studio has.
    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        use egui::Key;
        let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        if pressed(Key::Space) {
            self.play_stop();
        }
        if pressed(Key::Enter) {
            self.locate(0.0);
        }
        if cmd && pressed(Key::O) {
            self.open_dialog();
        }
        if cmd && pressed(Key::Z) {
            if shift {
                self.redo();
            } else {
                self.undo();
            }
        }
        if !cmd && (pressed(Key::Delete) || pressed(Key::Backspace)) {
            self.delete_selection();
        }
        if !cmd {
            if pressed(Key::C) {
                self.toggle_cycle();
            }
            if pressed(Key::X) {
                self.command(ctx, "Show Mixer");
            }
            if pressed(Key::E) || pressed(Key::P) {
                self.command(ctx, "Show Editor");
            }
            if pressed(Key::B) {
                self.show_browser = !self.show_browser;
            }
            if pressed(Key::I) {
                self.show_inspector = !self.show_inspector;
            }
            let bar = self.bar_ticks() as f64;
            if pressed(Key::Comma) {
                let p = self.position();
                self.locate(((p / bar).ceil() - 1.0).max(0.0) * bar);
            }
            if pressed(Key::Period) {
                let p = self.position();
                self.locate(((p / bar).floor() + 1.0) * bar);
            }
            if let Some(t) = self.selected_track {
                if pressed(Key::M) {
                    if let Some(tr) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                        tr.mute = !tr.mute;
                    }
                    self.sync_mix();
                }
                if pressed(Key::S) && !shift {
                    if let Some(tr) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                        tr.solo = !tr.solo;
                    }
                    self.sync_mix();
                }
            }
            let n = self.project.as_ref().map_or(0, |p| p.tracks.len());
            if pressed(Key::ArrowUp) {
                self.selected_track = Some(self.selected_track.unwrap_or(0).saturating_sub(1));
            }
            if pressed(Key::ArrowDown) && n > 0 {
                self.selected_track = Some((self.selected_track.map_or(0, |t| t + 1)).min(n - 1));
            }
        } else {
            // ⌘← / ⌘→: zoom the tracks area out / in
            if pressed(Key::ArrowRight) {
                self.bar_w = (self.bar_w * 1.25).min(1200.0);
            }
            if pressed(Key::ArrowLeft) {
                self.bar_w = (self.bar_w / 1.25).max(8.0);
            }
        }
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
            let r = Rect::from_min_max(pos2(left, body.min.y), pos2(left + BROWSER_W, body.max.y));
            crate::browser::show(self, ui, r);
            left = r.max.x + 1.0;
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
            Bottom::PianoRoll => EDITOR_H.min(main.height() * 0.6),
            Bottom::Mixer => (main.height() * 0.62).max(EDITOR_H.min(main.height() * 0.6)),
        };
        let tracks = Rect::from_min_max(main.min, pos2(main.max.x, main.max.y - editor_h - if editor_h > 0.0 { 1.0 } else { 0.0 }));
        crate::tracks::show(self, ui, tracks);
        if editor_h > 0.0 {
            let ed = Rect::from_min_max(pos2(main.min.x, main.max.y - editor_h), main.max);
            match self.bottom {
                Bottom::PianoRoll => crate::pianoroll::show(self, ui, ed),
                Bottom::Mixer => crate::mixer::show(self, ui, ed),
                Bottom::None => {}
            }
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
