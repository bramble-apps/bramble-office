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
pub const BROWSER_W: f32 = 520.0;
pub const INSPECTOR_W: f32 = 236.0;
/// The panes on the right (List Editors, Notes, Loop Browser, Browsers).
/// The right panes (List Editors, Notes, Loops, Browsers): Logic's 450 pt (ui-list-editors-event.png).
pub const RIGHT_W: f32 = 450.0;
pub const EDITOR_H: f32 = 300.0;

const PPQ_F: f64 = lg_core::events::PPQ as f64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bottom {
    None,
    PianoRoll,
    Mixer,
    SmartControls,
}

/// The Track inspector's parameters (Logic's: applied as the track plays).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackProps {
    pub transpose: i8,
    pub velocity: i8,
    pub delay: i32,
    pub key_limit: (u8, u8),
    pub vel_limit: (u8, u8),
    pub no_transpose: bool,
}

impl Default for TrackProps {
    fn default() -> Self {
        TrackProps { transpose: 0, velocity: 0, delay: 0, key_limit: (0, 127), vel_limit: (1, 127), no_transpose: false }
    }
}

/// An aux channel strip (made when a send to a new bus is set up, as Logic does): its input bus,
/// effects, fader, pan, mute and solo.
#[derive(Clone, Debug, PartialEq)]
pub struct AuxStrip {
    pub name: String,
    pub bus: usize,
    pub volume_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Its Audio FX (Logic's plug-in name and settings, bypass).
    pub inserts: Vec<lg_core::plugin::Plugin>,
    /// Where its output goes.
    pub output: crate::audio::Out,
}

/// One send on a channel strip: the bus, its level (dB), before the fader or after it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SendSlot {
    pub bus: usize,
    pub level_db: f32,
    pub pre: bool,
}

/// The Mixer's view (Logic: Single, Tracks, All).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixerView {
    Single,
    Tracks,
    All,
}

/// The panes on the window's right (Logic: List Editors, Notes, Loop Browser, Browsers).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RightPane {
    Lists,
    Notes,
    Loops,
    Files,
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
    /// Control bar toggles: Quick Help, the Toolbar row, Replace, Low Latency, the Tuner, Solo
    /// mode, Count-in; the LCD's display; the pane open on the right.
    pub quick_help: bool,
    pub show_toolbar: bool,
    pub replace_mode: bool,
    pub low_latency: bool,
    pub show_tuner: bool,
    pub solo_mode: bool,
    pub count_in: bool,
    pub lcd_mode: crate::controlbar::LcdMode,
    /// The LCD's Custom columns and its Division / Project End fields (lcdcustom.rs).
    pub lcdc: crate::lcdcustom::State,
    pub right_pane: Option<RightPane>,
    /// Recording (MIDI from the inputs and Musical Typing): where it began.
    pub record_from: Option<f64>,
    /// Notes played in while recording (and, for Capture as Recording, the last ones played
    /// while not): song ticks.
    pub recorded: Vec<lg_core::events::Note>,
    /// Musical Typing and MIDI keyboards.
    pub live: crate::live::Live,
    pub played: Vec<lg_core::events::Note>,
    /// The Inspector's Region and Track sections open; the Track parameters by track.
    pub inspector_region_open: bool,
    pub inspector_track_open: bool,
    pub track_props: std::collections::HashMap<usize, TrackProps>,
    /// The instrument picked for a track in the Library (Logic's patch or plug-in name).
    pub instrument_choice: std::collections::HashMap<usize, String>,
    /// List Editors and Notes panes: their tabs; the project's and tracks' notes; markers
    /// (position, name).
    pub list_tab: usize,
    pub notes_tab: usize,
    pub project_notes: String,
    pub track_notes: std::collections::HashMap<usize, String>,
    pub markers: Vec<lg_core::markers::Marker>,
    /// The Loop Browser and All Files: search text, the folder shown, the file selected.
    pub files_search: String,
    pub files_dir: Option<PathBuf>,
    pub files_selected: Option<PathBuf>,
    pub bottom: Bottom,
    pub selected_track: Option<usize>,
    pub selected_region: Option<(usize, usize)>,
    /// Every selected region (Shift-click, Select All…); `selected_region` is the one in focus.
    pub selected_regions: Vec<(usize, usize)>,
    /// The nudge value in ticks (Set Nudge Value to…; Logic's default: a division).
    pub nudge: u32,
    /// The Tracks area's tool (T menu) and the Command-click tool.
    pub tool: crate::edit::Tool,
    pub cmd_tool: crate::edit::Tool,
    pub snap_on: bool,
    /// The Settings window's tab and sub-tab, and the Logic settings' values.
    pub settings_tab: String,
    pub settings_sub: String,
    pub logic_settings: crate::settings::Values,
    /// The Key Commands window and the key command set in use (keycmds.rs).
    pub keycmds: crate::keycmds::State,
    /// The MIDI Environment, control surfaces, sync and the movie (envhub.rs).
    pub hub: crate::envhub::Hub,
    /// The open project's Project Settings and their window's state (projsettings.rs).
    pub proj: crate::projsettings::Values,
    pub ps_ui: crate::projsettings::Ui,
    /// Surround and Spatial Audio (surround.rs) and its windows (surround_ui.rs).
    pub sur: crate::surround::State,
    pub sur_ui: crate::surround_ui::Ui,
    /// The global tracks shown (G): Arrangement, Marker, Tempo, Signature…
    pub global_tracks: bool,
    /// Catch is off: the user scrolled while playing (back on at stop, or with `).
    pub catch_off: bool,
    /// The playhead was moved (Go to Beginning, a locate…): the Tracks area scrolls to show it.
    pub reveal_playhead: bool,
    /// Live Loops: the grid, its cells, what plays and what's queued.
    pub live_loops: crate::liveloops::State,
    /// Bramble's Piano Roll extras (ghost notes, more MIDI Draw lanes).
    pub rx: crate::rollextras::State,
    /// Bramble's custom colour palette (its editing state).
    pub pal: crate::palette::State,
    /// The audio editors (Track, File, Smart Tempo): their state and the regions' audio edits.
    pub audio_edit: crate::audioedit::AudioEdit,
    /// Flex Time / Flex Pitch: the tracks' settings, pitch analyses (flex.rs).
    pub flex: crate::flex::State,
    /// Automation shown on the tracks (A); each track's mode and the lane its header shows.
    pub show_automation: bool,
    pub auto_mode: std::collections::HashMap<usize, crate::automation::AutoMode>,
    pub auto_param: std::collections::HashMap<usize, u8>,
    /// The window's egui context (for state kept in egui's memory, e.g. the Step Sequencer's).
    pub ctx: egui::Context,
    /// The Mixer's aux strips, each track's sends, the Mixer's view and Sends on Faders (which
    /// send the faders show: None = the channels' own).
    pub auxes: Vec<AuxStrip>,
    pub sends: std::collections::HashMap<usize, Vec<SendSlot>>,
    pub mixer_view: MixerView,
    pub sends_on_faders: Option<usize>,
    /// Bass / Keyboard Player regions' settings; the chord track (one chord per bar, "C Am F G").
    pub players: std::collections::HashMap<(usize, usize), crate::players::Player>,
    pub chord_text: String,
    /// The Create New Track sheet.
    pub new_track_open: bool,
    pub new_track_kind: crate::newtrack::Kind,
    pub new_track_count: u32,
    /// Arrangement markers: start, length (ticks), name.
    pub arrangement: Vec<(u32, u32, String)>,
    /// The Browser: linked to the selected track, replacing its patch; its category filter and
    /// sort (by name, else Logic's relevance).
    pub browser_link: bool,
    pub browser_replace: bool,
    pub browser_filter: Option<String>,
    pub browser_sort_name: bool,
    /// The Piano Roll's inspector: shown, its Time Quantize value, Strength and Swing.
    pub roll_inspector: bool,
    pub roll_quantize: u32,
    pub roll_strength: u8,
    pub roll_swing: u8,
    /// Tracks record-enabled (R) and input-monitored (I) in their headers.
    pub armed: std::collections::HashSet<usize>,
    pub monitoring: std::collections::HashSet<usize>,
    /// The Piano Roll's tool; Autopunch; the Quantize value (ticks) for the Q key.
    pub roll_tool: crate::edit::Tool,
    pub autopunch: bool,
    pub quantize_grid: u32,
    /// Where the pointer is (keys go to the area under it), the tracks area's width.
    pub pointer: egui::Pos2,
    pub tracks_w: f32,
    /// A region or marker being renamed: what's typed so far.
    pub renaming_region: Option<(usize, usize, String)>,
    pub renaming_marker: Option<(usize, String)>,
    pub tool_menu_open: bool,
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
    /// Every region Copy took (regionclip.rs).
    pub region_clip: Vec<crate::regionclip::Clip>,
    /// The Logic file the project came from (Save writes back into it): the alternative's
    /// folder and its ProjectData as read.
    pub file: Option<(PathBuf, Vec<u8>)>,
    /// Edited since opened or saved.
    pub dirty: bool,
    /// The LCD's tempo being typed (double-click), and a track being renamed (double-click its
    /// name): what's typed so far.
    pub tempo_edit: Option<String>,
    pub renaming: Option<(usize, String)>,
    /// Bramble's drummer: each drummer region's settings, and the editor's tab (Main, Details,
    /// Manual).
    pub drum_params: std::collections::HashMap<(usize, usize), crate::drummer::Params>,
    pub drum_tab: usize,
    /// Each track's Channel EQ (by track), and the track whose EQ window is open.
    pub eqs: std::collections::HashMap<usize, crate::eq::Params>,
    pub eq_window: Option<usize>,
    /// The instrument plug-in window open (by track); bypassed instruments; Copy / Compare.
    pub plugin_window: Option<usize>,
    /// The MIDI FX windows (midifx.rs).
    pub mfx: crate::midifx::Ui,
    /// Snap and Drag modes, the tempo and signature editors (arranging.rs).
    pub arr: crate::arranging::State,
    /// Markers: selection, focus, the Marker List / Text windows (markers.rs).
    pub mk: crate::markers::State,
    /// The global tracks shown, their order and heights (globals.rs).
    pub gt: crate::globals::State,
    pub arrm: crate::arrmarks::State,
    pub ti: crate::tempointerp::State,
    pub stm: crate::stmulti::State,
    pub gc: crate::globalclip::State,
    pub bm: crate::beatmapping::State,
    /// Project Tempo mode, free tempo recording (tempomode.rs).
    pub tm2: crate::tempomode::State,
    /// The Region inspector's extras, section clipboard, Region Solo, Gain tool (regionparams.rs).
    pub rp: crate::regionparams::State,
    pub plugin_bypass: std::collections::HashSet<usize>,
    /// The instruments' Side Chain choices (sidechain.rs).
    pub side: crate::sidechain::State,
    pub plugin_clipboard: Option<Vec<f32>>,
    pub plugin_compare: std::collections::HashMap<usize, Vec<f32>>,
    /// The instrument editors (Sampler, Quick Sampler, Drum Machine / Kit Designer, Alchemy).
    pub inst: crate::instwin::State,
    /// The Mixer's groups, VCAs, routing, master chain, filters, selection and undo (mixstate).
    pub mix: crate::mixstate::Mix,
    /// MIDI editing: the Piano Roll's focus, note clipboard, MIDI Transform window, View options.
    pub midi: crate::midifunc::Midi,
    /// Recording (audio and MIDI), its Record menu options, take folders' state.
    pub rec: crate::recording::Recording,
    /// Stop Button Options and the last locate position (stopbutton.rs).
    pub stopbtn: crate::stopbutton::Opts,
    /// The control bar's optional transport buttons (Customize Control Bar).
    pub cb_show: crate::controlbar::Shown,
    /// The Tracks area's own state: track selection, stacks entered, zoom, the marquee, fades,
    /// automation selection (trackarea.rs, regionops.rs, automation.rs).
    /// File menu: templates, alternatives, backups, Project Settings (projfile.rs).
    pub pf: crate::projfile::State,
    pub ta: crate::trackarea::State,
    /// The bars' own controls (Catch, Auto Zoom…; barfit.rs).
    pub fit: crate::barfit::State,
    /// The Browser (browser.rs, soundlib.rs), Smart Controls and the Arpeggiators
    /// (smartcontrols.rs, arp.rs), Project Audio and All Files (projaudio.rs, allfiles.rs), the
    /// Project Files pane's tab (0 Project, 1 All Files), the browsers' preview.
    pub br: crate::browser::State,
    pub sc: crate::smartcontrols::State,
    /// Automation beyond one lane: Trim / Relative, region automation, subtracks, snap, Quick
    /// Access, the Event List (autoext.rs).
    pub ax: crate::autoext::State,
    /// Session Player presets, custom patterns, hands (spx.rs).
    pub spx: crate::spx::State,
    pub spdrum: crate::spdrum::State,
    pub artic: crate::artic::State,
    pub sbp: crate::sbp::State,
    pub keyx: crate::keyextra::State,
    /// Track headers and the Mixer beyond the first draft (tm.rs).
    pub tm: crate::tm::State,
    /// The Play Surface windows (playsurface.rs).
    pub ps: crate::playsurface::State,
    /// Optional transport buttons, shuttling, giant displays, Project Information (transportx.rs).
    pub tx: crate::transportx::State,
    pub pa: crate::projaudio::State,
    pub af: crate::allfiles::State,
    /// The All Files Browser's search conditions, recent searches and bookmarks (afsearch.rs).
    pub afs: crate::afsearch::State,
    /// Track Import (trackimport.rs).
    pub tim: crate::trackimport::State,
    /// The Browser Focus triangle (brfocus.rs).
    pub brf: crate::brfocus::State,
    /// Final Cut Pro XML export and import (fcpxml.rs).
    pub fcp: crate::fcpxml::State,
    /// Autosave (autosave.rs).
    pub autosave: crate::autosave::State,
    pub pf_tab: usize,
    pub preview: crate::preview::Preview,
    /// Navigation, windows, screensets, the toolbar (nav.rs, screensets.rs, toolbar.rs).
    pub nav: crate::nav::State,
    /// Logic's key focus: the area clicked last (focus.rs).
    pub kf: crate::focus::KeyFocus,
    pub screens: crate::screensets::State,
    pub bars: crate::toolbar::State,
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
            quick_help: false,
            show_toolbar: false,
            replace_mode: false,
            low_latency: false,
            show_tuner: false,
            solo_mode: false,
            count_in: true,
            lcd_mode: crate::controlbar::LcdMode::BeatsProject,
            lcdc: Default::default(),
            right_pane: None,
            record_from: None,
            selected_regions: Vec::new(),
            nudge: 240,
            tool: crate::edit::Tool::Pointer,
            cmd_tool: crate::edit::Tool::Marquee,
            snap_on: true,
            settings_tab: "General".into(),
            settings_sub: String::new(),
            logic_settings: crate::settings::Values::load(),
            keycmds: crate::keycmds::State::load(),
            hub: Default::default(),
            proj: Default::default(),
            ps_ui: Default::default(),
            sur: Default::default(),
            sur_ui: Default::default(),
            global_tracks: false,
            catch_off: false,
            reveal_playhead: false,
            live_loops: Default::default(),
            rx: Default::default(),
            pal: crate::palette::State { name: crate::palette::load_saved(automated), ..Default::default() },
            audio_edit: Default::default(),
            flex: Default::default(),
            show_automation: false,
            auto_mode: Default::default(),
            auto_param: Default::default(),
            ctx: cc.egui_ctx.clone(),
            auxes: Vec::new(),
            sends: Default::default(),
            mixer_view: MixerView::Tracks,
            sends_on_faders: None,
            players: Default::default(),
            chord_text: String::new(),
            new_track_open: false,
            new_track_kind: Default::default(),
            new_track_count: 1,
            arrangement: Vec::new(),
            browser_link: true,
            browser_replace: true,
            browser_filter: None,
            browser_sort_name: false,
            roll_inspector: true,
            roll_quantize: 240,
            roll_strength: 100,
            roll_swing: 50,
            armed: Default::default(),
            monitoring: Default::default(),
            roll_tool: crate::edit::Tool::Pointer,
            autopunch: false,
            quantize_grid: 240,
            pointer: egui::Pos2::ZERO,
            tracks_w: 800.0,
            renaming_region: None,
            renaming_marker: None,
            tool_menu_open: false,
            recorded: Vec::new(),
            live: crate::live::Live::new(),
            played: Vec::new(),
            inspector_region_open: false,
            inspector_track_open: false,
            track_props: Default::default(),
            instrument_choice: Default::default(),
            list_tab: 0,
            notes_tab: 0,
            project_notes: String::new(),
            track_notes: Default::default(),
            markers: Vec::new(),
            files_search: String::new(),
            files_dir: None,
            files_selected: None,
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
            header_w: crate::tracks::HEADER_W_DEFAULT,
            editor_h: EDITOR_H,
            mixer_h: 420.0,
            help_tag: None,
            editor_rect: Rect::NOTHING,
            clipboard: None,
            region_clip: Vec::new(),
            file: None,
            dirty: false,
            tempo_edit: None,
            renaming: None,
            drum_params: std::collections::HashMap::new(),
            drum_tab: 0,
            eqs: std::collections::HashMap::new(),
            eq_window: None,
            plugin_window: None,
            mfx: Default::default(),
            arr: Default::default(),
            mk: Default::default(),
            gt: Default::default(),
            arrm: Default::default(),
            ti: Default::default(),
            stm: Default::default(),
            gc: Default::default(),
            bm: Default::default(),
            tm2: Default::default(),
            rp: Default::default(),
            mix: Default::default(),
            plugin_bypass: Default::default(),
            side: Default::default(),
            plugin_clipboard: None,
            rec: Default::default(),
            plugin_compare: Default::default(),
            inst: Default::default(),
            midi: Default::default(),
            stopbtn: Default::default(),
            cb_show: Default::default(),
            ta: Default::default(),
            fit: Default::default(),
            br: Default::default(),
            sc: Default::default(),
            ax: Default::default(),
            ps: Default::default(),
            tx: Default::default(),
            spx: Default::default(),
            spdrum: Default::default(),
            artic: Default::default(),
            sbp: Default::default(),
            keyx: Default::default(),
            tm: Default::default(),
            pa: Default::default(),
            af: Default::default(),
            afs: Default::default(),
            tim: Default::default(),
            brf: Default::default(),
            fcp: Default::default(),
            autosave: Default::default(),
            pf_tab: 0,
            preview: Default::default(),
            nav: Default::default(),
            kf: Default::default(),
            screens: Default::default(),
            bars: crate::toolbar::State::load(),
            pf: Default::default(),
        };
        // (Settings: the saved values into the app, the audio device; Startup Action)
        crate::prefs::apply_all(&mut app);
        match path {
            Some(p) => app.open_path(&p),
            None => {
                app.set_project(crate::demo::song(), true);
                crate::prefs::startup(&mut app);
            }
        }
        app
    }

    pub fn project_title(&self) -> String {
        match &self.project {
            Some(p) if self.demo => format!("{} (demo)", p.name),
            Some(p) => format!("{}.logicx{}", p.name, if self.dirty { " — Edited" } else { "" }),
            None => "Untitled".into(),
        }
    }

    pub fn tell(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), 0.0));
    }

    pub fn open_path(&mut self, p: &Path) {
        crate::prefs::note_opened(p);
        match lg_core::open(p) {
            Ok(proj) => {
                let n = proj.tracks.len();
                // the ProjectData Save will write back into (.logicx; GarageBand's next)
                let pkg = lg_core::project::package_of(p);
                self.file = lg_core::project::alternatives(&pkg).into_iter().next().and_then(|(_, alt)| std::fs::read(alt.join("ProjectData")).ok().map(|d| (alt, d)));
                self.set_project(proj, false);
                crate::projfile::after_open(self, p);
                self.dirty = false;
                self.tell(format!("Opened with {n} track{}", if n == 1 { "" } else { "s" }));
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
    pub fn set_project(&mut self, mut p: Project, demo: bool) {
        // (no Project End read: Logic's 128 bars, or past a longer song)
        lg_core::projend::fit(&mut p);
        // (a project's settings: Logic's defaults until projsettings::after_open reads its own)
        self.proj = Default::default();
        crate::projsettings::apply(self);
        self.programs = p.tracks.iter().map(|t| program_for(&t.name, t.kind)).collect();
        // each track's Channel EQ as saved in the project (Bramble's Channel EQ stands in)
        self.eqs = p.tracks.iter().enumerate().filter_map(|(i, t)| t.plugins.iter().find(|u| u.name == "Channel EQ").and_then(|u| crate::eq::Params::from_logic(&u.params)).map(|e| (i, e))).collect();
        self.drum_params.clear();
        crate::trackarea::reset(self);
        crate::autoext::reset(self);
        self.undo.clear();
        self.redo.clear();
        self.selected_track = (!p.tracks.is_empty()).then_some(0);
        self.selected_region = p.tracks.iter().enumerate().find(|(_, t)| !t.regions.is_empty()).map(|(i, _)| (i, 0));
        let bar = lg_core::project::bar_ticks(p.signature);
        let end = p.tracks.iter().flat_map(|t| t.regions.iter()).map(|r| r.start + r.length).max().unwrap_or(bar * 4);
        self.cycle = (0, end.max(bar));
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
        self.demo = demo;
        self.project = Some(p);
        crate::mixstate::from_project(self);
        self.load_engine();
        if let Some((t, r)) = self.selected_region {
            self.center_roll(t, r);
        }
    }

    /// The song as the engine plays it (audio files read in the background).
    pub fn load_engine(&mut self) {
        // aliases and clones follow their originals (lg-core regions)
        if let Some(p) = self.project.as_mut() {
            lg_core::regions::sync_aliases(&mut p.tracks);
        }
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
            // each region as it plays: its inspector parameters applied, and looped (Loop) up to
            // the next region on the track or the project's end
            let song_end = p.tracks.iter().flat_map(|t| t.regions.iter().map(|r| r.start + r.length)).max().unwrap_or(0).max(64 * self.bar_ticks());
            let mut notes: Vec<(u32, u32, u8, u8)> = Vec::new();
            // (Region Solo: only the soloed regions play, regionparams.rs)
            for (_, r) in t.regions.iter().enumerate().filter(|(ri, r)| !r.muted && crate::regionparams::plays(self, i, *ri)) {
                let mut until = if r.props.looped { t.regions.iter().map(|o| o.start).filter(|&s| s >= r.start + r.length).min().unwrap_or(song_end) } else { r.start + r.length };
                // (a loop dragged out at the region's corner ends there)
                if r.props.looped && r.props.loop_to > r.start + r.length {
                    until = until.min(r.props.loop_to);
                }
                let mut offset = 0u32;
                // (muted notes and notes past the region's end don't play; regparams: the
                // region parameters as played)
                let groove = lg_core::groove::groove_index(r.props.quantize).and_then(|k| lg_core::groove::resolve(p, k));
                let played = lg_core::regparams::play_notes_groove(&r.props, &r.notes, r.start, r.start + r.length, &|tk| lg_core::regparams::key_shift(p, tk), groove.as_ref());
                loop {
                    for n in &played {
                        let mut n = *n;
                        let tp = self.track_props.get(&i).copied().unwrap_or_default();
                        if !tp.no_transpose {
                            n.pitch = (n.pitch as i16 + tp.transpose as i16).clamp(0, 127) as u8;
                        }
                        n.velocity = (n.velocity as i16 + tp.velocity as i16).clamp(1, 127) as u8;
                        n.pos = (n.pos as i64 + tp.delay as i64).max(0) as u32;
                        if n.pitch < tp.key_limit.0 || n.pitch > tp.key_limit.1 || n.velocity < tp.vel_limit.0 || n.velocity > tp.vel_limit.1 {
                            continue;
                        }
                        let at = n.pos + offset;
                        if offset > 0 && at >= until {
                            continue;
                        }
                        // the Transposition track
                        n.pitch = (n.pitch as i16 + crate::trackarea::transposition(self, i, at)).clamp(0, 127) as u8;
                        // the region's Pitch Source (globals.rs; Key Signature is applied by
                        // lg_core::regparams::play_notes above)
                        if r.props.pitch_source > 1 {
                            n.pitch = (n.pitch as i16 + crate::globals::pitch_source_shift(self, &r.props, at)).clamp(0, 127) as u8;
                        }
                        notes.push((at, at + n.len.max(1), n.pitch, n.velocity));
                    }
                    offset += r.length.max(1);
                    if !r.props.looped || r.start + offset >= until {
                        break;
                    }
                }
            }
            notes.sort_by_key(|n| n.0);
            crate::arp::for_playback(self, i, &mut notes);
            crate::artic::for_playback(self, i, &mut notes);
            tracks.push(PlayTrack { channel, program: self.programs.get(i).copied().unwrap_or(0), notes, audio: Vec::new(), volume: db_to_gain(t.volume_db), pan: t.pan, mute: t.mute, solo: t.solo, eq: self.eqs.get(&i).copied(), instrument: self.instrument_for(i), instrument_floats: t.plugins.iter().find(|u| u.slot == 2 && !self.instrument_choice.contains_key(&i)).map(|u| u.params.clone()).unwrap_or_default(), inserts: t.plugins.iter().filter(|u| u.slot >= 3 && u.name != "Channel EQ" && !u.name.is_empty()).map(|u| (u.name.clone(), u.params.clone())).collect(), sends: self.sends.get(&i).map(|v| v.iter().map(|s| audio::Send { bus: s.bus, gain: db_to_gain(s.level_db), pre: s.pre }).collect()).unwrap_or_default(), auto_volume: crate::automation::for_engine(self, i).0, auto_pan: crate::automation::for_engine(self, i).1 });
        }
        crate::trackarea::stack_mix(self, &mut tracks);
        crate::tm::engine_tracks(self, &mut tracks);
        let param_auto: Vec<Vec<crate::automation::ParamLane>> = (0..tracks.len()).map(|i| crate::automation::params_for_engine(self, i)).collect();
        let audio_files = crate::audioedit::playback(self);
        {
            let mut e = self.engine.lock().unwrap_or_else(|e| e.into_inner());
            e.all_off();
            e.tempo = p.tempo;
            e.tempo_map = p.tempo_map.clone();
            e.beats_per_bar = p.signature.0.max(1) as u32;
            e.tracks = tracks;
            e.param_auto = param_auto;
            crate::mixstate::to_engine(self, &mut e);
            crate::mixengine::to_engine(self, &mut e);
            crate::arp::to_engine(self, &mut e);
            e.timeline = lg_core::timeline::Map::of(p);
            crate::midifx::to_engine(self, &mut e);
            e.cycle = Some(self.cycle);
            e.program_changes();
        }
        if !audio_files.is_empty() {
            let engine = self.engine.clone();
            std::thread::spawn(move || {
                for (track, start, plan) in audio_files {
                    if let Some(samples) = plan.render() {
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

    /// What a track's instrument is: the Library's choice, else the project's instrument
    /// plug-in, else (a drummer) Logic's drum kit, else the patch named as the track.
    pub fn instrument_for(&self, i: usize) -> Option<String> {
        if let Some(c) = self.instrument_choice.get(&i) {
            return Some(c.clone());
        }
        let t = self.project.as_ref()?.tracks.get(i)?;
        if t.kind == TrackKind::Audio {
            return None;
        }
        if let Some(u) = t.plugins.iter().find(|u| u.slot == 2 && !u.name.is_empty()) {
            return Some(u.name.clone());
        }
        Some(if t.kind == TrackKind::Drummer { "Drum Kit Designer".into() } else { t.name.clone() })
    }

    /// Plays a note on the track's sound (the List Editor and Piano Roll's click).
    pub fn preview_note(&mut self, track: usize, pitch: u8, _velocity: u8) {
        crate::pianoroll::preview(self, track, pitch);
    }

    /// Plays an audio file once (the Loop Browser's and All Files' click), over the song.
    pub fn preview_file(&mut self, path: &Path) {
        self.output.preview(path);
    }

    /// Imports a file at the playhead (Loop Browser / All Files double-click, File > Import):
    /// a MIDI file's tracks become new software instrument tracks; an audio file a region on
    /// the selected audio track, or on a new one.
    pub fn import_file(&mut self, path: &Path) {
        let bar = self.bar_ticks();
        let at = ((self.position() as u32) / bar) * bar;
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if ["mid", "midi", "smf", "kar"].contains(&ext.as_str()) {
            let Some(m) = std::fs::read(path).ok().and_then(|d| lg_core::midi::read(&d)) else {
                self.tell("Couldn't read that MIDI file");
                return;
            };
            self.checkpoint("Import MIDI File");
            let Some(p) = self.project.as_mut() else { return };
            let n = m.tracks.len();
            for (i, (tname, ch, notes)) in m.tracks.into_iter().enumerate() {
                let end = notes.iter().map(|n| n.pos + n.len).max().unwrap_or(bar);
                let length = end.div_ceil(bar).max(1) * bar;
                let notes: Vec<_> = notes.into_iter().map(|mut n| {
                    n.pos += at;
                    n
                }).collect();
                let title = if tname.is_empty() { if n > 1 { format!("{name} {}", i + 1) } else { name.clone() } } else { tname };
                let drums = ch == 9;
                let row = p.tracks.len() as u32 + 1;
                let region = lg_core::Region { name: title.clone(), start: at, length, notes, audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
                p.tracks.push(lg_core::Track { name: title.clone(), kind: if drums { TrackKind::Drummer } else { TrackKind::Instrument }, channel: 0, row, regions: vec![region], volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new(), automation: Vec::new(), extra: Default::default() });
                self.programs.push(if drums { 0 } else { program_for(&title, TrackKind::Instrument) });
            }
            if let Some(t) = m.tempo.filter(|_| p.tracks.len() == n) {
                p.tempo = t;
            }
            self.selected_track = self.project.as_ref().map(|p| p.tracks.len() - 1);
            self.after_edit();
            self.tell(format!("Imported {n} track{}", if n == 1 { "" } else { "s" }));
            return;
        }
        // (Project Settings ▸ Assets ▸ Convert audio file sample rate when importing)
        let converted = crate::recsettings::import_converted(self, path);
        let path = converted.as_path();
        let Some(seconds) = lg_core::project::wav_seconds(path).or_else(|| audio::load_audio(path).map(|s| s.len() as f64 / 2.0 / audio::RATE as f64)) else {
            self.tell("Couldn't read that audio file");
            return;
        };
        self.checkpoint("Add Audio File");
        let tempo = self.tempo();
        let length = ((seconds * tempo / 60.0 * lg_core::events::PPQ as f64) as u32).max(1);
        let Some(p) = self.project.as_mut() else { return };
        let target = self.selected_track.filter(|t| p.tracks.get(*t).is_some_and(|tr| tr.kind == TrackKind::Audio));
        let t = match target {
            Some(t) => t,
            None => {
                let row = p.tracks.len() as u32 + 1;
                p.tracks.push(lg_core::Track { name: "Audio".into(), kind: TrackKind::Audio, channel: 0, row, regions: Vec::new(), volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new(), automation: Vec::new(), extra: Default::default() });
                self.programs.push(0);
                p.tracks.len() - 1
            }
        };
        p.tracks[t].regions.push(lg_core::Region { name, start: at, length, notes: Vec::new(), audio: Some(lg_core::project::AudioRef { file: path.to_path_buf(), seconds }), muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
        self.selected_track = Some(t);
        self.after_edit();
        // (Project Settings ▸ Smart Tempo ▸ Set Imported Files To)
        let r = self.project.as_ref().map_or(0, |p| p.tracks[t].regions.len() - 1);
        crate::stdefaults::imported(self, t, r);
    }

    /// Ends recording: the notes played become a region on the selected track (Logic: from the
    /// bar recording began to the bar after the last note).
    pub fn finish_recording(&mut self) {
        crate::recording::finish(self);
    }

    /// Capture as Recording: the notes just played while not recording, as a region.
    pub fn capture_recording(&mut self) {
        let notes = std::mem::take(&mut self.played);
        let from = notes.iter().map(|n| n.pos).min().unwrap_or(0);
        self.make_take(from, notes, "Capture as Recording");
    }

    fn make_take(&mut self, from: u32, notes: Vec<lg_core::events::Note>, what: &str) {
        // (placed as the Record menu's overlap and Replace choices say)
        crate::recording::midi_take(self, from, notes, what);
    }

    /// Sends a track to `bus`: Logic makes an aux strip for a bus that has none, with a reverb on
    /// it, and the send starts at −∞ … here at −10 dB so it's heard.
    pub fn add_send(&mut self, track: usize, bus: usize) {
        if !self.auxes.iter().any(|a| a.bus == bus) {
            let n = self.auxes.len() + 1;
            self.auxes.push(AuxStrip { name: format!("Aux {n}"), bus, volume_db: 0.0, pan: 0.0, mute: false, solo: false, inserts: vec![crate::mixstate::new_plugin("ChromaVerb")], output: Default::default() });
        }
        let list = self.sends.entry(track).or_default();
        if !list.iter().any(|s| s.bus == bus) {
            list.push(SendSlot { bus, level_db: -10.0, pre: false });
        }
        self.dirty = true;
        self.load_engine();
    }

    pub fn recording(&self) -> bool {
        self.record_from.is_some()
    }

    /// Seconds from bar 1 to `tick`, through the tempo changes.
    pub fn seconds_at(&self, tick: f64) -> f64 {
        let Some(p) = &self.project else { return tick / PPQ_F / 120.0 * 60.0 };
        let mut secs = 0.0;
        let mut at = 0.0;
        let mut bpm = p.tempo;
        for &(t, b) in p.tempo_map.iter() {
            let t = t as f64;
            if t >= tick {
                break;
            }
            if t > at {
                secs += (t - at) / PPQ_F * 60.0 / bpm;
                at = t;
            }
            bpm = b;
        }
        secs + (tick - at).max(0.0) / PPQ_F * 60.0 / bpm
    }

    pub fn position(&self) -> f64 {
        let e = self.engine();
        // (Live Loops cells running on past the Project End: the playhead stays there)
        match e.end {
            Some(end) if e.past_end => e.pos.min(end as f64),
            _ => e.pos,
        }
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
            // (Logic's last locate position: where playback started)
            let at = e.pos;
            // Settings ▸ Audio ▸ General ▸ Playback pre-roll: start early by the compensation
            let early = crate::prefs::playback_preroll(&self.logic_settings, e.latency(), self.project.as_ref().map_or(120.0, |p| p.tempo_at(at as u32)));
            if early > 0.0 {
                e.locate(at - early);
            }
            e.playing = true;
            drop(e);
            self.stopbtn.last_pos = at;
        }
    }

    /// The Stop button and command, as Stop Button Options say (stopbutton.rs).
    pub fn stop(&mut self) {
        crate::stopbutton::press(self);
    }

    /// Stops playback where it is.
    pub fn halt(&mut self) {
        let mut e = self.engine();
        e.playing = false;
        e.all_off();
    }

    pub fn locate(&mut self, ticks: f64) {
        // (Settings ▸ Catch when moving playhead)
        if crate::prefs::catch_on_move(self) {
            self.catch_off = false;
        }
        self.engine().locate(ticks);
        self.reveal_playhead = true;
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

    /// Sets the song's tempo (the LCD): one tempo for the whole song.
    pub fn set_tempo(&mut self, bpm: f64) {
        let bpm = (bpm * 100.0).round() / 100.0;
        // (Project Settings ▸ Recording ▸ Allow tempo change recording)
        if crate::recsettings::record_tempo(self, bpm) {
            return;
        }
        let Some(p) = self.project.as_mut() else { return };
        if (p.tempo - bpm).abs() < 1e-6 {
            return;
        }
        p.tempo = bpm.clamp(5.0, 990.0);
        p.tempo_map = vec![(0, p.tempo)];
        self.dirty = true;
        let (t, m) = (p.tempo, p.tempo_map.clone());
        let mut e = self.engine();
        e.tempo = t;
        e.tempo_map = m;
    }

    /// The tempo at the playhead (the LCD).
    pub fn tempo(&self) -> f64 {
        let pos = self.position() as u32;
        self.project.as_ref().map_or(120.0, |p| p.tempo_at(pos))
    }

    /// Sets a track's sound (the Browser's instruments: the SoundFont's presets).
    /// The Browser: the track plays one of Logic's patches (our stand-in for it; until there is
    /// one, the SoundFont's nearest General MIDI sound), and takes its name, as Logic's tracks do.
    pub fn choose_patch(&mut self, track: usize, category: &str, name: &str) {
        self.checkpoint("Change Patch");
        self.instrument_choice.insert(track, name.to_string());
        let guess = patch_program(category, name);
        self.set_program(track, guess, name);
        self.load_engine();
    }

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
        crate::mixstate::apply_vcas(self, &mut e);
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
        // (the Bounce window; a surround or Spatial Audio project's has Surround Bounce)
        if only.is_none() {
            crate::share::command(self, "Project or Section…");
            return;
        }
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
            crate::stepseq::region_removed(&self.ctx, t, r);
            self.selected_region = None;
        }
        self.after_edit();
    }

    /// File > Save (⌘S): the edits written into the project's own ProjectData (Logic's file,
    /// everything else unchanged); the first save keeps the original beside it.
    pub fn save(&mut self) {
        crate::projfile::save(self)
    }

    /// File > Save As… / Save A Copy As…: the whole package copied to a new place, then saved there.
    pub fn save_as(&mut self) {
        crate::projfile::save_as(self, false)
    }

    /// Track > New Session Player SI Track…: a drummer track with an 8-bar region from the
    /// playhead's bar, played by Bramble's drummer.
    pub fn new_drummer(&mut self) {
        self.checkpoint("New Session Player Track");
        let bar = self.bar_ticks();
        let at = (self.position() as u32 / bar) * bar;
        let Some(p) = self.project.as_mut() else { return };
        let row = p.tracks.len() as u32 + 1;
        let region = lg_core::Region { name: "Drummer - Pop Rock".into(), start: at, length: 8 * bar, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
        p.tracks.push(lg_core::Track { name: "SoCal".into(), kind: TrackKind::Drummer, channel: 0, row, regions: vec![region], volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new(), automation: Vec::new(), extra: Default::default() });
        self.programs.push(0);
        let t = p.tracks.len() - 1;
        self.selected_track = Some(t);
        self.selected_region = Some((t, 0));
        self.bottom = Bottom::PianoRoll;
        self.drum_tab = 0;
        crate::drummer::regenerate(self, t, 0);
    }

    /// Before an edit: the song as it is goes on the undo list.
    pub fn checkpoint(&mut self, what: &str) {
        self.dirty = true;
        // (the markers are the app's: kept with the song for undo)
        if let Some(p) = self.project.as_mut() {
            p.markers = self.markers.clone();
            p.arrangement = self.arrangement.clone();
        }
        let Some(p) = &self.project else { return };
        self.undo.push((what.to_string(), p.clone(), self.programs.clone()));
        let steps = self.logic_settings.get("General / Editing / Number of Undo Steps:").parse::<usize>().unwrap_or(100).max(1);
        while self.undo.len() > steps {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.live_loops.cell_redo.clear();
    }

    pub fn undo(&mut self) {
        let Some((what, p, progs)) = self.undo.pop() else { return };
        if let Some(mut cur) = self.project.take() {
            cur.markers = self.markers.clone();
            cur.arrangement = self.arrangement.clone();
            self.redo.push((what.clone(), cur, self.programs.clone()));
        }
        self.markers = p.markers.clone();
        self.arrangement = p.arrangement.clone();
        self.project = Some(p);
        self.programs = progs;
        crate::cellinsp::undone(self);
        self.after_edit();
        self.tell(format!("Undo {what}"));
    }

    pub fn redo(&mut self) {
        let Some((what, p, progs)) = self.redo.pop() else { return };
        if let Some(mut cur) = self.project.take() {
            cur.markers = self.markers.clone();
            cur.arrangement = self.arrangement.clone();
            self.undo.push((what.clone(), cur, self.programs.clone()));
        }
        self.markers = p.markers.clone();
        self.arrangement = p.arrangement.clone();
        self.project = Some(p);
        self.programs = progs;
        crate::cellinsp::redone(self);
        self.after_edit();
        self.tell(format!("Redo {what}"));
    }

    /// After the song changed: selections still valid, the engine follows.
    pub fn after_edit(&mut self) {
        crate::trackarea::sync(self);
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

    /// A menu item's text as shown: the live label, with the app's working name.
    pub fn menu_label(&self, label: &str) -> String {
        crate::shown_name(&self.menu_label_live(label))
    }

    fn menu_label_live(&self, label: &str) -> String {
        if let Some(l) = crate::nav::menu_label(self, label) {
            return l;
        }
        if let Some(l) = crate::stmulti::menu_label(self, label) {
            return l;
        }
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
            l if l.starts_with("Mixer Undo") => match self.mix.undo.last() {
                Some((w, _)) => format!("Mixer Undo {w}"),
                None => "Mixer Undo".into(),
            },
            _ => label.to_string(),
        }
    }

    pub fn command(&mut self, ctx: &egui::Context, cmd: &str) {
        if crate::audioedit::command(self, cmd) || crate::barfit::command(self, cmd) || crate::listwin::command(self, ctx, cmd) {
            return;
        }
        match cmd {
            "About Bramble Studio" => self.show_about = true,
            "General…" | "Audio…" | "Recording…" | "MIDI…" | "Score…" | "Movie…" | "Automation…" | "Control Surfaces…" | "View…" | "My Info…" => {
                self.settings_tab = cmd.trim_end_matches('…').to_string();
                self.settings_sub.clear();
                self.show_settings = true;
            }
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
            "New" | "New from Template…" => crate::projfile::new_project(self, cmd),
            "Open…" => self.open_dialog(),
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
            "Save" => self.save(),
            "Save As…" | "Save A Copy As…" => crate::projfile::save_as(self, cmd == "Save A Copy As…"),
            "New Session Player SI Track…" => {
                self.new_track_kind = crate::newtrack::Kind::Drummer;
                self.new_track_open = true;
            }
            "New Software Instrument Track" | "New Audio Track" => {
                let _ = self.key_command(ctx, cmd);
            }
            c if c.starts_with("Undo ") && c != "Undo History…" => self.undo(),
            c if c.starts_with("Can’t Redo") || c.starts_with("Redo") => self.redo(),
            "1 Track as Audio File…" => crate::freeze::open_export_selected(self),
            "All MIDI Tracks as MIDI File…" => self.export_midi(),
            "Show Musical Typing" => {
                self.key_command(ctx, "Show/Hide Musical Typing");
            }
            _ => {
                if !self.key_command(ctx, cmd) {
                    self.tell(format!("“{cmd}” isn't available yet"));
                }
            }
        }
    }

    /// Key presses: Logic's key commands for the focused area (keymap.rs); ⌘Z / ⇧⌘Z undo.
    fn keys(&mut self, ctx: &egui::Context) {
        crate::focus::press(self, ctx);
        crate::trackimport::keys(self, ctx);
        if ctx.egui_wants_keyboard_input() {
            crate::live::frame(self, ctx);
            return;
        }
        let presses: Vec<(egui::Key, egui::Modifiers, Option<egui::Key>)> = ctx.input(|i| i.events.iter().filter_map(|e| match e {
            egui::Event::Key { key, pressed: true, modifiers, physical_key, .. } => Some((*key, *modifiers, *physical_key)),
            // (egui-winit turns ⌘C / ⌘X / ⌘V into these and sends no key: Edit's Copy, Cut and
            // Paste for the area with key focus)
            egui::Event::Copy => Some((egui::Key::C, egui::Modifiers::COMMAND, None)),
            egui::Event::Cut => Some((egui::Key::X, egui::Modifiers::COMMAND, None)),
            egui::Event::Paste(_) | egui::Event::PasteImage(_) => Some((egui::Key::V, egui::Modifiers::COMMAND, None)),
            _ => None,
        }).collect());
        // the area clicked last, not the one under the pointer (focus.rs)
        let focus = crate::focus::keymap_focus(self, ctx);
        let mut used = crate::live::frame(self, ctx);
        used.extend(crate::playsurface::keys(self, ctx));
        for (key, m, physical) in presses {
            if used.contains(&key) {
                continue;
            }
            // the Key Commands window open: keys learn or list there instead of running commands
            if crate::keycmds::key_press(self, key, m, physical) {
                continue;
            }
            // the Environment and Controller Assignments windows' own keys
            if crate::envhub::key_press(self, ctx, key, m) {
                continue;
            }
            // the T menu open: a letter picks the tool (T again: the Pointer), Esc closes it
            if self.tool_menu_open {
                self.tool_menu_open = false;
                if key == egui::Key::T && !m.shift {
                    self.edit_command(ctx, "Pointer Tool");
                    continue;
                }
                if let Some(t) = crate::keymap::tool_for(key, m.shift) {
                    self.edit_command(ctx, t);
                    continue;
                }
                if key == egui::Key::Escape {
                    continue;
                }
            }
            if crate::stepinput::key_press(self, ctx, key, m) {
                continue;
            }
            if focus == crate::keymap::Focus::PianoRoll && crate::audioedit::key_press(self, key, m) {
                continue;
            }
            // over an instrument editor: its undo and its key commands
            if crate::instwin::key_press(self, ctx, key, m) || crate::pluginwin::fx_key(self, ctx, key, m) {
                continue;
            }
            if m.command && key == egui::Key::Z {
                // over the Mixer, its own history first (Include Mixer Undo Steps in Project Undo)
                if focus == crate::keymap::Focus::Mixer && self.mix.include_in_project_undo && (if m.shift { crate::mixstate::redo(self) } else { crate::mixstate::undo(self) }) {
                    continue;
                }
                if m.shift { self.redo() } else { self.undo() }
                continue;
            }
            if m.command && key == egui::Key::O {
                self.open_dialog();
                continue;
            }
            if m.command && key == egui::Key::S {
                if m.shift { self.save_as() } else { self.save() }
                continue;
            }
            if crate::liveloops::key_press(self, key, m) {
                continue;
            }
            // the Browser, Project Audio and Smart Controls under the pointer: their groups
            if crate::browser::key_press(self, key, m) || crate::projaudio::key_press(self, key, m) || crate::smartcontrols::key_press(self, key, m) {
                continue;
            }
            let cmds = crate::keymap::commands_for(key, m, focus);
            if cmds.is_empty() && matches!(key, egui::Key::Delete | egui::Key::Backspace) && m.is_none() {
                crate::focus::delete_key(self, ctx);
                continue;
            }
            for c in cmds {
                if self.key_command(ctx, c) {
                    break;
                }
            }
        }
    }

    /// Runs a Logic key command by its name; false when Studio doesn't have it yet.
    pub fn key_command(&mut self, ctx: &egui::Context, name: &str) -> bool {
        if crate::listwin::command(self, ctx, name) || crate::evlist::command(self, ctx, name) || crate::groovecmd::command(self, name) || crate::joinaudio::command(self, name) || crate::demix::command(self, name) || crate::regsolo::command(self, name) || crate::varispeed::command(self, name) {
            return true;
        }
        if crate::focus::command(self, name) || crate::share::command(self, name) || crate::keyextra::command(self, ctx, name) {
            return true;
        }
        if crate::arrmarks::command(self, name) || crate::globalclip::command(self, name) || crate::beatmapping::command(self, name) || crate::stmulti::command(self, name) || crate::tempointerp::command(self, name) || crate::markers::command(self, ctx, name) || crate::globals::command(self, name) || crate::tempomode::command(self, name) {
            return true;
        }
        if crate::plughead::command(self, ctx, name) || crate::autoext::command(self, ctx, name) || crate::spx::command(self, name) || crate::artic::command(self, name) || crate::sbp::command(self, name) {
            return true;
        }
        if crate::envhub::command(self, ctx, name) || crate::convsampler::command(self, ctx, name) {
            return true;
        }
        if crate::fcpxml::command(self, name) {
            return true;
        }
        if crate::trackimport::command(self, name) {
            return true;
        }
        if crate::transportx::command(self, ctx, name) {
            return true;
        }
        if crate::playsurface::command(self, name) {
            return true;
        }
        if crate::tm::command(self, ctx, name) {
            return true;
        }
        if crate::instwin::command(self, ctx, name) || crate::browser::command(self, name) || crate::projaudio::global(self, name) || crate::smartcontrols::command(self, name) {
            return true;
        }
        if crate::flex::command(self, ctx, name) || crate::hosting::command(self, name) || crate::arranging::command(self, name) {
            return true;
        }
        if crate::nav::command(self, ctx, name) || crate::screensets::command(self, ctx, name) || crate::toolbar::command(self, ctx, name) || crate::windows::command(self, name) {
            return true;
        }
        // the MIDI editors besides the Piano Roll: Score, Step Sequencer, Step Input, Step and
        // Event Editors
        if crate::scorecmd::command(self, ctx, name) || crate::scorepdf::command(self, ctx, name) || crate::scoreattr::command(self, ctx, name) || crate::scorestyle::command(self, ctx, name) || crate::scoregrid::command(self, ctx, name) || crate::scoreinsp::command(self, ctx, name) || crate::scorexml::command(self, name) || crate::stepseq::command(self, ctx, name) || crate::noterepeat::command(self, ctx, name) || crate::stepinput::command(self, ctx, name) || crate::stepedit::command(self, ctx, name) || crate::eventedit::command(self, ctx, name) {
            return true;
        }
        if crate::surround_ui::command(self, ctx, name) {
            return true;
        }
        if crate::projfile::command(self, name) || crate::keycmds::command(self, name) || crate::prefs::command(self, name) || crate::projsettings::command(self, name) {
            return true;
        }
        if crate::mixercmd::run(self, ctx, name) {
            return true;
        }
        if crate::midifunc::command(self, ctx, name) {
            return true;
        }
        if crate::stopbutton::command(self, name) {
            return true;
        }
        if crate::recording::command(self, name) {
            return true;
        }
        if crate::stemsplit::command(self, name) || crate::regionparams::command(self, ctx, name) {
            return true;
        }
        // (the Live Loops view commands from the menus and key commands; the grid's own buttons
        // and keys already reach liveloops.rs)
        if crate::live::command(self, name) {
            return true;
        }
        if matches!(name, "Show/Hide Live Loops Grid" | "Switch between Live Loops Grid and Tracks View" | "Show Both, Live Loops Grid and Tracks View" | "Show Live Loops Grid only" | "Show Tracks View only" | "Toggle Grid Horizontal Zoom" | "Stop All Cells") && crate::liveloops::command(self, name) {
            return true;
        }
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
            "Show/Hide Musical Typing" | "Show Musical Typing" | "Open Musical Typing" => {
                self.live.typing_open = !self.live.typing_open;
                // (opened by this command: the computer keys, not the onscreen keyboard)
                self.live.keyboard_mode = false;
                if self.live.typing_open && self.live.input_names.is_empty() {
                    self.live.connect_midi(ctx);
                }
            }
            "Create Marker" | "Create Marker without Rounding" => {
                let pos = self.position() as u32;
                let at = if name == "Create Marker" { ((pos as f64 / bar).round() * bar) as u32 } else { pos };
                if !self.markers.iter().any(|m| m.0 == at) {
                    let n = self.markers.len() + 1;
                    self.markers.push(lg_core::markers::Marker::new(at, format!("Marker {n}")));
                    self.markers.sort_by_key(|m| m.0);
                    self.dirty = true;
                }
            }
            "Delete Marker" => {
                let pos = self.position() as u32;
                if let Some(i) = self.markers.iter().rposition(|m| m.0 <= pos) {
                    self.markers.remove(i);
                    self.dirty = true;
                }
            }
            "Go to Next Marker" | "Go to Previous Marker" => {
                let pos = self.position() as u32;
                let to = if name == "Go to Next Marker" { self.markers.iter().map(|m| m.0).find(|&m| m > pos) } else { self.markers.iter().map(|m| m.0).filter(|&m| m < pos).last() };
                if let Some(t) = to {
                    self.locate(t as f64);
                }
            }
            "Record" | "Record / Record Toggle" => crate::recording::record_pressed(self, name),
            "Capture as Recording" => self.capture_recording(),
            "Discard Recording and Return to Last Play Position" => crate::recording::discard(self),
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
            "New Session Player SI Track…" => self.new_drummer(),
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
                        copy.origin = copy.source.or(copy.origin);
                        copy.source = None;
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
            // (the Tracks area's regions; the editors' notes are midifunc.rs's, steps stepseq.rs's)
            "Copy" | "Cut" if crate::focus::tracks(self) => {
                if !crate::regionclip::copy(self) {
                    return false;
                }
                if name == "Cut" {
                    self.edit_command(ctx, "Delete");
                }
            }
            // at the playhead, on the selected track (Logic)
            "Paste" if crate::focus::tracks(self) => return crate::regionclip::paste(self),
            "Quantize Selected Regions/Cells/Events" => {
                // Logic's default quantize: 1/16 notes; the selected notes in the Piano Roll, else
                // the whole selected region
                let Some((t, r)) = self.selected_region else { return false };
                self.checkpoint("Quantize");
                let q = lg_core::events::PPQ / 4;
                let sel = self.selected_notes.clone();
                let only = self.bottom == Bottom::PianoRoll && !sel.is_empty();
                if let Some(rg) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|tr| tr.regions.get_mut(r)) {
                    for (i, n) in rg.notes.iter_mut().enumerate() {
                        if !only || sel.contains(&i) {
                            n.pos = (n.pos + q / 2) / q * q;
                        }
                    }
                }
                self.after_edit();
            }
            "Zoom Horizontal In" => self.bar_w = (self.bar_w * 1.25).min(1200.0),
            "Zoom Horizontal Out" => self.bar_w = (self.bar_w / 1.25).max(8.0),
            "Export Tracks as Audio Files…" => {
                let t = self.selected_track;
                self.bounce(t);
            }
            "Save" => self.save(),
            "Save Project as…" => self.save_as(),
            other => return self.edit_command(ctx, other),
        }
        true
    }


    fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if let Some(pp) = ctx.input(|i| i.pointer.hover_pos()) {
            self.pointer = pp;
        }
        // the focused region is always part of the selection
        match self.selected_region {
            None => self.selected_regions.clear(),
            Some(x) if !self.selected_regions.contains(&x) => self.selected_regions = vec![x],
            _ => {}
        }
        if let Some(p) = &self.project {
            self.selected_regions.retain(|(t, r)| p.tracks.get(*t).is_some_and(|tr| *r < tr.regions.len()));
        }
        if let Some((_, t)) = &mut self.message {
            *t += ctx.input(|i| i.stable_dt) as f64;
            if *t > 3.5 {
                self.message = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        let window = ui.max_rect();
        // (the Settings switch takes effect right away)
        kn_ui::winframe::follow(&ctx, self.look.system_title_bar);
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
            crate::windows::area(self, bar, "control-bar");
            top = bar.max.y;
        }
        if self.show_toolbar {
            let bar = Rect::from_min_size(pos2(window.min.x, top), vec2(window.width(), crate::toolbar::H));
            crate::toolbar::show(self, ui, bar);
            crate::windows::area(self, bar, "toolbar");
            top = bar.max.y;
        }
        let body = Rect::from_min_max(pos2(window.min.x, top + 1.0), window.max);
        let mut left = body.min.x;
        if self.show_browser {
            let w = self.browser_w.min(body.width() - 360.0 - if self.show_inspector { INSPECTOR_W } else { 0.0 }).max(200.0);
            let r = Rect::from_min_max(pos2(left, body.min.y), pos2(left + w, body.max.y));
            crate::browser::show(self, ui, r);
            crate::windows::area(self, r, "browser");
            self.nav.browser_rect = r;
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
        // (as Logic: when a right pane leaves the Tracks area under its ~360 pt minimum, the
        // Inspector hides itself, and stays hidden; RESIZING.txt)
        if self.show_inspector && self.right_pane.is_some() && body.max.x - RIGHT_W.min(body.width() * 0.5) - (left + INSPECTOR_W) < 360.0 {
            self.show_inspector = false;
        }
        if self.show_inspector {
            let r = Rect::from_min_max(pos2(left, body.min.y), pos2(left + INSPECTOR_W, body.max.y));
            crate::inspector::show(self, ui, r);
            crate::windows::area(self, r, "inspector");
            self.nav.inspector_rect = r;
            left = r.max.x + 1.0;
        }
        let mut right = body.max.x;
        if let Some(pane) = self.right_pane {
            let w = RIGHT_W.min(body.width() - left - 360.0).max(220.0);
            let r = Rect::from_min_max(pos2(body.max.x - w, body.min.y), body.max);
            crate::rightpane::show(self, ui, r, pane);
            crate::windows::area(self, r, "right-pane");
            right = r.min.x - 1.0;
        }
        let main = Rect::from_min_max(pos2(left, body.min.y), pos2(right, body.max.y));
        // (the Mixer takes more of the window than an editor, as Logic's)
        let editor_h = match self.bottom {
            Bottom::None => 0.0,
            Bottom::PianoRoll => self.editor_h.clamp(120.0, main.height() - 90.0),
            Bottom::Mixer => self.mixer_h.clamp(260.0, main.height() - 90.0),
            Bottom::SmartControls => 190.0f32.min(main.height() - 90.0),
        };
        let tracks = Rect::from_min_max(main.min, pos2(main.max.x, main.max.y - editor_h - if editor_h > 0.0 { 1.0 } else { 0.0 }));
        self.tracks_w = (tracks.width() - self.header_w).max(100.0);
        self.kf.tracks_rect = Some(tracks);
        crate::tracks::show(self, ui, tracks);
        if editor_h > 0.0 {
            let ed = Rect::from_min_max(pos2(main.min.x, main.max.y - editor_h), main.max);
            self.editor_rect = ed;
            crate::windows::area(self, ed, if self.bottom == Bottom::Mixer { "mixer" } else { "editor" });
            // (no region selected: the pane shows the selected track's, as Logic; edpane.rs)
            let picked = self.selected_region.is_none() && self.bottom == Bottom::PianoRoll;
            let auto = if picked { crate::edpane::shown_region(self) } else { None };
            if picked {
                self.selected_region = auto;
            }
            self.fit.picked = picked;
            let drummer = self.selected_region.filter(|(t, _)| self.project.as_ref().and_then(|p| p.tracks.get(*t)).is_some_and(|tr| tr.kind == TrackKind::Drummer));
            let created = picked && self.bottom == Bottom::PianoRoll && crate::celledit::current(self).is_none() && crate::edpane::create_view(self, ui, ed);
            match self.bottom {
                Bottom::PianoRoll if created => {}
                Bottom::PianoRoll if crate::celledit::current(self).is_some() => crate::celledit::show(self, ui, ed),
                Bottom::PianoRoll if self.selected_region.is_some_and(|(t, r)| crate::stepseq::is_pattern(ui.ctx(), t, r)) => crate::stepseq::show(self, ui, ed),
                // (no region selected on an instrument track: Logic's four tabs; Step Sequencer
                // and Session Player offer to make that kind of region; edpane.rs)
                Bottom::PianoRoll if self.selected_region.is_some_and(|x| self.players.contains_key(&x)) => {
                    let (t, r) = self.selected_region.unwrap();
                    crate::players::show(self, ui, ed, t, r);
                }
                Bottom::PianoRoll if drummer.is_some() => {
                    let (t, r) = drummer.unwrap();
                    crate::drummer::show(self, ui, ed, t, r);
                }
                Bottom::PianoRoll if crate::audioedit::is_audio_region(self) => crate::audioedit::show(self, ui, ed),
                Bottom::PianoRoll => match crate::score::editor_tab(self, ui.ctx()) {
                    crate::score::EditorTab::Score => crate::score::show(self, ui, ed),
                    crate::score::EditorTab::StepSequencer => crate::stepseq::show(self, ui, ed),
                    crate::score::EditorTab::PianoRoll => crate::pianoroll::show(self, ui, ed),
                },
                Bottom::Mixer => crate::mixer::show(self, ui, ed),
                Bottom::SmartControls => crate::smartcontrols::show(self, ui, ed),
                Bottom::None => {}
            }
            // (the picked region stays unselected unless the editor selected it)
            if picked && self.selected_region == auto {
                self.selected_region = None;
            }
            self.fit.picked = false;
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
        crate::focus::paint(self, &ctx, &ui.painter().with_clip_rect(body));
        MIDDLE_C4.store(self.logic_settings.get("View / General / Display Middle C As:").starts_with("C4"), std::sync::atomic::Ordering::Relaxed);
        let tags_on = self.logic_settings.get("View / General / Show help tags") != "0";
        if let Some(tag) = self.help_tag.take().filter(|_| tags_on) {
            help_tag(&ctx, &tag);
        }
        crate::dialogs::show(self, &ctx);
        crate::midiwin::windows(self, &ctx);
        crate::noterepeat::window(self, &ctx);
        crate::stepinput::window(self, &ctx);
        crate::evlist::float_window(self, &ctx);
        crate::listwin::windows(self, &ctx);
        crate::joinaudio::window(self, &ctx);
        crate::regsolo::frame(self, &ctx);
        crate::varispeed::frame(self, &ctx);
        crate::stepedit::window(self, &ctx);
        crate::live::window(self, &ctx);
        crate::playsurface::windows(self, &ctx);
        crate::transportx::frame(self, &ctx);
        crate::fcpxml::window(self, &ctx);
        crate::autosave::frame(self, &ctx);
        crate::pluginwin::window(self, &ctx);
        crate::midifx::window(self, &ctx);
        crate::artic::window(self, &ctx);
        crate::sbp::window(self, &ctx);
        crate::keyextra::history_window(self, &ctx);
        crate::arranging::windows(self, &ctx);
        crate::markers::frame(self, &ctx);
        crate::globals::window(self, &ctx);
        crate::tempomode::frame(self, &ctx);
        crate::tempointerp::frame(self, &ctx);
        crate::stmulti::frame(self, &ctx);
        crate::globalclip::frame(self, &ctx);
        crate::regionparams::windows(self, &ctx);
        crate::hosting::manager(self, &ctx);
        crate::stemsplit::window(self, &ctx);
        crate::newtrack::sheet(self, &ctx);
        crate::tm::windows(self, &ctx);
        crate::projaudio::window(self, &ctx);
        crate::smartcontrols::window(self, &ctx);
        crate::autoext::frame(self, &ctx);
        crate::projend::frame(self);
        crate::browser::frame(self, &ctx);
        crate::projfile::windows(self, &ctx);
        crate::surround_ui::windows(self, &ctx);
        crate::share::windows(self, &ctx);
        if let Some(t) = self.eq_window {
            let name = self.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
            let mut open = true;
            let mut p = self.eqs.get(&t).copied().unwrap_or_default();
            let analyzer = self.engine().eq_analyzer(t);
            if crate::eq::window(&ctx, &mut open, t, &name, &mut p, &analyzer) {
                self.eqs.insert(t, p);
                if let Some(tr) = self.engine().tracks.get_mut(t) {
                    tr.eq = Some(p);
                }
            }
            if !open {
                self.eq_window = None;
            }
        }
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
    /// Quitting: the window layout is kept for the next start (Logic keeps it in the project).
    fn on_exit(&mut self) {
        crate::screensets::remember(self);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        kn_ui::begin_frame(&ctx);
        // a .logicx dropped on the window opens
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if let Some(p) = dropped.first()
            && !crate::instwin::takes_drop(self, &ctx, &dropped)
            && !crate::share::takes_drop(self, &dropped)
        {
            self.open_path(p);
        }
        self.keys(&ctx);
        crate::nav::frame(self);
        crate::liveloops::update(self);
        crate::noterepeat::frame(self, &ctx);
        crate::recording::update(self);
        crate::flex::update(self);
        crate::hosting::idle(self, &ctx);
        crate::tm::frame(self, &ctx);
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
/// A Library patch's General MIDI program (the SoundFont's stand-in) by its category.
pub fn patch_program(category: &str, name: &str) -> u8 {
    match category {
        "Bass" => 33,
        "Guitar" => 25,
        "Strings" => 48,
        "Brass & Woodwind" => 61,
        "Mallet" => 11,
        "Synthesizer" => {
            let p = program_for(name, TrackKind::Instrument);
            if p == 0 { 81 } else { p }
        }
        _ => program_for(name, TrackKind::Instrument),
    }
}

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
/// Settings ▸ View ▸ Display Middle C As: C3 (Yamaha, Logic's default) or C4 (Roland).
pub static MIDDLE_C4: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn note_name(p: u8) -> String {
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let base = if MIDDLE_C4.load(std::sync::atomic::Ordering::Relaxed) { 1 } else { 2 };
    format!("{}{}", N[(p % 12) as usize], p as i32 / 12 - base)
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
