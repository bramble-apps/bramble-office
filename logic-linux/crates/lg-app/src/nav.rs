//! Navigation as Logic's (Global Commands, Views Showing Time Ruler, Various Windows): the
//! playhead's go-to, rewind / forward steps and play-from commands, the locators, markers by
//! number, the Play and Stop button options, Auto Set Locators, the division and nudge values,
//! Tap Tempo; the Tracks area's zoom and scroll (fit, saved zooms, navigation snapshots with
//! Back / Forward, pages, Scroll in Play); the window commands (Window and View menus, the
//! Settings tabs). The position math is pure and tested below.

use crate::app::{App, Bottom, RightPane};
use lg_core::events::PPQ;

/// Ticks in a whole note.
const WHOLE: u32 = PPQ * 4;

/// What the Tracks area shows: its zoom and scroll (Save as Zoom, navigation snapshots).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub bar_w: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub v_zoom: f32,
}

/// Navigation snapshots: Back steps to the one before, Forward again (Logic's ⌥Z / ⌥⇧Z).
#[derive(Clone, Debug, Default)]
pub struct History {
    back: Vec<View>,
    forward: Vec<View>,
}

impl History {
    /// Remembers `now` (the view before a zoom-to-fit or Store Navigation Snapshot).
    pub fn store(&mut self, now: View) {
        if self.back.last() != Some(&now) {
            self.back.push(now);
            if self.back.len() > 64 {
                self.back.remove(0);
            }
        }
        self.forward.clear();
    }

    pub fn back(&mut self, now: View) -> Option<View> {
        let v = self.back.pop()?;
        self.forward.push(now);
        Some(v)
    }

    pub fn forward(&mut self, now: View) -> Option<View> {
        let v = self.forward.pop()?;
        self.back.push(now);
        Some(v)
    }
}

/// A modal sheet asking for a position or a number.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    GoToPosition(String),
    LeftLocator(String),
    RightLocator(String),
    MarkerNumber(String),
}

/// Navigation's state (App::nav).
pub struct State {
    /// The division (LCD, Rewind / Forward by Division Value), ticks.
    pub division: u32,
    /// Play Button Options (Navigate menu), in Logic's priority order.
    pub play_marquee: bool,
    pub play_cycle: bool,
    pub play_region: bool,
    pub play_last: bool,
    /// Auto Set Locators: on, and by what.
    pub auto_locators: bool,
    pub auto_marquee: bool,
    pub auto_region: bool,
    pub auto_marker: bool,
    pub auto_note: bool,
    auto_seen: (Vec<(usize, usize)>, Option<(u32, u32)>, usize),
    /// Scroll in Play (⌃`): the playhead stays put and the lanes scroll under it.
    pub scroll_in_play: bool,
    /// Recall Zoom 1-3.
    pub zooms: [Option<View>; 3],
    pub history: History,
    /// The lanes' height (the Tracks area sets it each frame), for pages and vertical fits.
    pub lanes_h: f32,
    /// Tap Tempo: when the last taps were (seconds).
    taps: Vec<f64>,
    pub ask: Option<Ask>,
    /// The window's place before Move & Resize (Return to Previous Size).
    prev_window: Option<egui::Rect>,
    /// Quick Help: 0 Inspector pane, 1 Floating window, 2 Bubbles (Help menu).
    pub quick_help_as: u8,
    /// The window's parts this frame (Quick Help), and the Inspector's place.
    pub help_areas: Vec<(egui::Rect, &'static str)>,
    pub inspector_rect: egui::Rect,
    /// The Tracks area (headers and lanes) and its lanes' left edge, for drops from the Browser.
    pub tracks_rect: egui::Rect,
    pub lanes_x: f32,
    pub browser_rect: egui::Rect,
    /// The Browser: the item picked last (its kind, category, name), favourites, the search
    /// field to focus, a drag from it, the filter before Toggle All Filters.
    pub browser_pick: Option<(u8, String, String)>,
    pub favorites: Vec<String>,
    pub browser_focus: bool,
    pub browser_drag: Option<(u8, String, String)>,
    pub last_filter: Option<String>,
}

impl Default for State {
    fn default() -> State {
        State {
            division: PPQ / 4,
            play_marquee: true,
            play_cycle: true,
            play_region: false,
            play_last: false,
            auto_locators: false,
            auto_marquee: true,
            auto_region: true,
            auto_marker: false,
            auto_note: false,
            auto_seen: Default::default(),
            scroll_in_play: false,
            zooms: [None; 3],
            history: History::default(),
            lanes_h: 400.0,
            taps: Vec::new(),
            ask: None,
            prev_window: None,
            quick_help_as: 1,
            help_areas: Vec::new(),
            inspector_rect: egui::Rect::NOTHING,
            tracks_rect: egui::Rect::NOTHING,
            browser_rect: egui::Rect::NOTHING,
            lanes_x: 0.0,
            browser_pick: None,
            favorites: crate::browser::load_favorites(),
            browser_focus: false,
            browser_drag: None,
            last_filter: None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// pure math

/// A 1/`denom` note in ticks; a triplet is two thirds of it.
pub fn note_ticks(denom: u32, triplet: bool) -> u32 {
    let t = WHOLE / denom.max(1);
    if triplet { t * 2 / 3 } else { t }
}

/// The divisions Logic offers, coarse to fine (1/4, 1/8, 1/12 = 1/8 triplet, … 1/192).
pub const DIVISIONS: [u32; 12] = [960, 640, 480, 320, 240, 160, 120, 80, 60, 40, 30, 20];

/// The next finer (`higher`) or coarser division.
pub fn division_step(cur: u32, higher: bool) -> u32 {
    if higher {
        DIVISIONS.iter().copied().find(|&d| d < cur).unwrap_or(DIVISIONS[DIVISIONS.len() - 1])
    } else {
        DIVISIONS.iter().rev().copied().find(|&d| d > cur).unwrap_or(DIVISIONS[0])
    }
}

/// A position typed as Logic writes it: "bar beat division tick", any of them left out (from
/// the end), separated by spaces, dots or commas; 1-based. `div` = the division's ticks.
pub fn parse_position(s: &str, sig: (u8, u8), div: u32) -> Option<u32> {
    let parts: Vec<i64> = s.split(|c: char| c == ' ' || c == '.' || c == ',').filter(|x| !x.is_empty()).map(|x| x.parse::<i64>()).collect::<Result<_, _>>().ok()?;
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let beat_t = (WHOLE / sig.1.max(1) as u32) as i64;
    let bar_t = beat_t * sig.0.max(1) as i64;
    let units = [bar_t, beat_t, div.max(1) as i64, 1];
    let mut t = 0i64;
    for (i, v) in parts.iter().enumerate() {
        t += (v - 1) * units[i];
    }
    Some(t.max(0) as u32)
}

/// A time typed as "minutes:seconds" or "hours:minutes:seconds" (decimals allowed), in seconds.
pub fn parse_time(s: &str) -> Option<f64> {
    if !s.contains(':') {
        return None;
    }
    let mut secs = 0.0;
    for p in s.split(':') {
        secs = secs * 60.0 + p.trim().parse::<f64>().ok()?;
    }
    Some(secs)
}

/// Zoom to fit ticks `a`..`b` into `width` points: the bar width and the scroll (bars), with a
/// little room after the end as Logic leaves.
pub fn fit(width: f32, a: u32, b: u32, bar: u32) -> (f32, f32) {
    let bars = (b.saturating_sub(a) as f32 / bar.max(1) as f32).max(0.25);
    ((width / (bars * 1.04)).clamp(8.0, 1200.0), a as f32 / bar.max(1) as f32)
}

/// The tempo from Tap Tempo's taps (seconds): the mean of the last intervals, None until two
/// taps less than two seconds apart.
pub fn tap_tempo(taps: &[f64]) -> Option<f64> {
    let gaps: Vec<f64> = taps.windows(2).map(|w| w[1] - w[0]).filter(|g| *g > 0.05 && *g < 2.0).collect();
    if gaps.is_empty() {
        return None;
    }
    let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
    Some((60.0 / mean).clamp(5.0, 990.0))
}

/// Where the Tracks area scrolls while playing (bars): Catch turns a page when the playhead
/// nears the right edge or is left of it; Scroll in Play keeps it in the middle.
pub fn follow(scroll_x: f32, playhead: f32, visible: f32, scroll_in_play: bool) -> f32 {
    if scroll_in_play {
        return (playhead - visible / 2.0).max(0.0);
    }
    if playhead > scroll_x + visible * 0.92 || playhead < scroll_x {
        return playhead.floor().max(0.0);
    }
    scroll_x
}

/// Play from the Play button: where Play starts given the options (marquee, cycle start, the
/// selected region, the last locate position), first that applies; None: from the playhead.
pub fn play_start(opts: (bool, bool, bool, bool), marquee: Option<u32>, cycle: Option<u32>, region: Option<u32>, last: f64) -> Option<f64> {
    let (m, c, r, l) = opts;
    if m && let Some(x) = marquee {
        return Some(x as f64);
    }
    if c && let Some(x) = cycle {
        return Some(x as f64);
    }
    if r && let Some(x) = region {
        return Some(x as f64);
    }
    l.then_some(last)
}

// ---------------------------------------------------------------------------------------------
// helpers on the app

fn view(app: &App) -> View {
    View { bar_w: app.bar_w, scroll_x: app.scroll_x, scroll_y: app.scroll_y, v_zoom: app.ta.v_zoom }
}

fn set_view(app: &mut App, v: View) {
    app.bar_w = v.bar_w;
    app.scroll_x = v.scroll_x;
    app.scroll_y = v.scroll_y;
    app.ta.v_zoom = v.v_zoom;
    if app.playing() {
        app.catch_off = true;
    }
}

fn snapshot(app: &mut App) {
    let v = view(app);
    app.nav.history.store(v);
}

fn marquee(app: &App) -> Option<(u32, u32)> {
    app.ta.regions.marquee.as_ref().map(|m| (m.from.min(m.to), m.from.max(m.to)))
}

/// The selected regions' (or, in the Piano Roll, notes') span.
fn selection_span(app: &App) -> Option<(u32, u32)> {
    let p = app.project.as_ref()?;
    if app.bottom == Bottom::PianoRoll && !app.selected_notes.is_empty() {
        let (t, r) = app.selected_region?;
        let rg = p.tracks.get(t)?.regions.get(r)?;
        let ns: Vec<_> = app.selected_notes.iter().filter_map(|&i| rg.notes.get(i)).collect();
        return Some((ns.iter().map(|n| n.pos).min()?, ns.iter().map(|n| n.pos + n.len).max()?));
    }
    let sel = app.region_selection();
    let a = sel.iter().filter_map(|(t, r)| p.tracks.get(*t)?.regions.get(*r).map(|x| x.start)).min()?;
    let b = sel.iter().filter_map(|(t, r)| p.tracks.get(*t)?.regions.get(*r).map(|x| x.start + x.length)).max()?;
    Some((a, b))
}

fn contents_span(app: &App) -> (u32, u32) {
    let bar = app.bar_ticks();
    let p = app.project.as_ref();
    let a = p.and_then(|p| p.tracks.iter().flat_map(|t| t.regions.iter().map(|r| r.start)).min()).unwrap_or(0);
    let b = p.and_then(|p| p.tracks.iter().flat_map(|t| t.regions.iter().map(|r| r.start + r.length)).max()).unwrap_or(16 * bar);
    (a.min(b), b)
}

/// The project's end: past the last region, a whole bar (Logic's end marker, never before 8 bars).
pub fn project_end(app: &App) -> u32 {
    let bar = app.bar_ticks();
    contents_span(app).1.div_ceil(bar).max(8) * bar
}

fn rounded(app: &App, (a, b): (u32, u32)) -> (u32, u32) {
    let bar = app.bar_ticks();
    (a / bar * bar, b.div_ceil(bar).max(a / bar + 1) * bar)
}

fn play(app: &mut App) {
    if !app.playing() {
        app.play_stop();
    }
}

fn halt(app: &mut App) {
    app.halt();
}

fn locators_by_selection(app: &mut App, round: bool) -> bool {
    let Some(span) = marquee(app).or_else(|| selection_span(app)) else { return false };
    let c = if round { rounded(app, span) } else { span };
    app.set_cycle(c);
    true
}

fn fit_horizontal(app: &mut App, (a, b): (u32, u32)) {
    let (w, s) = fit(app.tracks_w, a, b, app.bar_ticks());
    app.bar_w = w;
    app.scroll_x = s;
}

fn fit_vertical(app: &mut App) {
    let mut tracks: Vec<usize> = app.region_selection().into_iter().map(|x| x.0).collect();
    if tracks.is_empty() {
        tracks.extend(app.ta.selected.iter().copied());
    }
    tracks.extend(app.selected_track);
    tracks.sort_unstable();
    tracks.dedup();
    let rows = crate::trackarea::rows(app);
    let shown: Vec<_> = rows.iter().filter(|r| tracks.contains(&r.t)).collect();
    let Some(first) = shown.first() else { return };
    let need = shown.len() as f32 * crate::tracks::ROW_H;
    let z = (app.nav.lanes_h / need.max(1.0)).clamp(crate::trackarea::ZOOM_STEPS[0], *crate::trackarea::ZOOM_STEPS.last().unwrap_or(&4.0));
    app.ta.v_zoom = z;
    let y = first.y / if first.h > 0.0 { first.h / (crate::tracks::ROW_H * z).max(1.0) } else { 1.0 };
    app.scroll_y = y.max(0.0);
}

fn open_list(app: &mut App, tab: usize, toggle: bool) {
    if toggle && app.right_pane == Some(RightPane::Lists) && app.list_tab == tab {
        app.right_pane = None;
    } else {
        app.right_pane = Some(RightPane::Lists);
        app.list_tab = tab;
    }
}

fn settings_tab(app: &mut App, tab: &str) {
    app.settings_tab = tab.to_string();
    app.settings_sub.clear();
    app.show_settings = true;
}

/// The window's place on its monitor (Move & Resize, Fill, Center).
fn place(app: &mut App, ctx: &egui::Context, f: impl Fn(egui::Rect) -> egui::Rect) {
    let (outer, monitor) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().monitor_size));
    let Some(m) = monitor else {
        app.tell("The window's place isn't known on this desktop");
        return;
    };
    if let Some(o) = outer {
        app.nav.prev_window = Some(o);
    }
    let r = f(egui::Rect::from_min_size(egui::Pos2::ZERO, m));
    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(r.min));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(r.size()));
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

// ---------------------------------------------------------------------------------------------
// every frame

/// The project's screensets once it opens, and Auto Set Locators.
pub fn frame(app: &mut App) {
    crate::screensets::frame(app);
    crate::screensets::autosave(app);
    if app.nav.auto_locators {
        let sel = app.region_selection();
        let mq = marquee(app);
        let notes = app.selected_notes.len();
        let seen = (sel.clone(), mq, notes);
        if seen != app.nav.auto_seen {
            app.nav.auto_seen = seen;
            let span = if app.nav.auto_marquee && mq.is_some() {
                mq
            } else if app.nav.auto_note && notes > 0 && app.bottom == Bottom::PianoRoll {
                selection_span(app)
            } else if app.nav.auto_region && !sel.is_empty() {
                selection_span(app).map(|s| rounded(app, s))
            } else {
                None
            };
            if let Some(c) = span.filter(|c| c.1 > c.0) {
                app.set_cycle(c);
            }
        }
    }
}

/// Where the Tracks area scrolls to while playing (called by tracks.rs with the visible bars).
pub fn follow_playhead(app: &mut App, visible_bars: f32) {
    if !app.playing() {
        // (Settings ▸ Catch when starting playback)
        if crate::prefs::catch_on_start(app) {
            app.catch_off = false;
        }
        // stopped: a locate off screen (Go to Beginning, Go to End…) scrolls to the playhead
        if std::mem::take(&mut app.reveal_playhead) {
            let bar_pos = app.position() as f32 / app.bar_ticks() as f32;
            app.scroll_x = follow(app.scroll_x, bar_pos, visible_bars, false);
        }
        return;
    }
    let moved = std::mem::take(&mut app.reveal_playhead);
    // (Catch off: the Tracks area's Catch button, barfit.rs)
    if (app.catch_off || !app.fit.catch) && !app.nav.scroll_in_play && !moved {
        return;
    }
    let bar_pos = app.position() as f32 / app.bar_ticks() as f32;
    app.scroll_x = follow(app.scroll_x, bar_pos, visible_bars, app.nav.scroll_in_play);
}

// ---------------------------------------------------------------------------------------------
// the commands

/// The command names run here (build.rs and tool/checklist.py read this list).
#[allow(dead_code)]
pub const COMMANDS: &[&str] = &[
    // transport
    "Play or Stop", "Play",
    "Stop or Play From Last Position", "Play or Stop and Go to Last Locate Position",
    "Rewind one Frame", "Forward one Frame", "Rewind by Division Value", "Forward by Division Value",
    "Rewind one 1/8 Note", "Forward one 1/8 Note", "Rewind one 1/16 Note", "Forward one 1/16 Note",
    "Rewind by Nudge Value", "Forward by Nudge Value",
    "Play from Beginning", "Play from Previous Bar", "Play from Left Locator", "Play from Right Locator", "Play from Left Window Edge",
    "Go to Last Locate Position", "Stop and Go to Beginning",
    "Go to Position…", "Position…", "Go to Beginning of First Region", "Go to End",
    "Left Locator", "Right Locator", "Selection Start", "Selection End", "Next Marker", "Previous Marker",
    // locators
    "Set Left Locator numerically…", "Set Right Locator numerically…",
    "Set Left Locator by Playhead", "Set Left Locator by Rounded Playhead", "Set Right Locator by Playhead", "Set Right Locator by Rounded Playhead",
    "Set Locators/Loop by Regions/Events/Marquee", "Set Rounded Locators/Loop by Regions/Events/Marquee",
    "Set Locators and Play", "Set Rounded Locators and Play", "Set Rounded Locators and Cycle Play",
    "Move Locators/Loop Forward by 1 Bar", "Move Locators/Loop Backwards by 1 Bar", "Move Locators Backwards by Cycle Length",
    "Toggle Auto Set Locators", "Enable Auto Set Locators", "By Marquee Selection", "By Region Selection", "By Marker/Chord Selection", "By Note Selection",
    // play / stop button options
    "Play from Marquee Selection", "Play from Cycle Start", "Play From Selected Region", "Play From Last Locate Position",
    // markers
    "Go to Marker Number…", "Marker Number…",
    "Go to Marker Number 1", "Go to Marker Number 2", "Go to Marker Number 3", "Go to Marker Number 4", "Go to Marker Number 5", "Go to Marker Number 6", "Go to Marker Number 7", "Go to Marker Number 8", "Go to Marker Number 9", "Go to Marker Number 10",
    "Go to Marker Number 11", "Go to Marker Number 12", "Go to Marker Number 13", "Go to Marker Number 14", "Go to Marker Number 15", "Go to Marker Number 16", "Go to Marker Number 17", "Go to Marker Number 18", "Go to Marker Number 19", "Go to Marker Number 20",
    "Create Marker by Cycle Area", "Open Marker List", "Show/Hide Marker List", "Create Arrangement Marker",
    "Create Marker for Region Selection", "Create Without Rounding", "Delete Marker at Playhead Position",
    // division, nudge, tempo
    "Set Next Higher Division", "Set Next Lower Division",
    "Set Division Value to 1/4 Note", "Set Division Value to 1/8 Note", "Set Division Value to 1/16 Note", "Set Division Value to 1/32 Note", "Set Division Value to 1/64 Note", "Set Division Value to 1/128 Note",
    "Set Division Value to 1/4 Triplet (1/6)", "Set Division Value to 1/8 Triplet (1/12)", "Set Division Value to 1/16 Triplet (1/24)", "Set Division Value to 1/32 Triplet (1/48)", "Set Division Value to 1/64 Triplet (1/96)", "Set Division Value to 1/128 Triplet (1/192)",
    "Set Nudge Value to 10 Ticks", "Set Nudge Value to 1/1 Note", "Set Nudge Value to 1/2 Note", "Set Nudge Value to 1/4 Note", "Set Nudge Value to 1/8 Note", "Set Nudge Value to 1/16 Note", "Set Nudge Value to 1/32 Note", "Set Nudge Value to 1/64 Note",
    "Set Nudge Value to 1/2 Triplet (1/3)", "Set Nudge Value to 1/4 Triplet (1/6)", "Set Nudge Value to 1/8 Triplet (1/12)", "Set Nudge Value to 1/16 Triplet (1/24)", "Set Nudge Value to 1/32 Triplet (1/48)", "Set Nudge Value to 1/64 Triplet (1/96)",
    "Set Nudge Value to 5 SMPTE Frame", "Set Nudge Value to SMPTE Bit",
    "Tap Tempo",
    // zoom and scroll
    "Zoom Horizontal In", "Zoom Horizontal Out",
    "Recall Zoom 1", "Recall Zoom 2", "Recall Zoom 3", "Save as Zoom 1", "Save as Zoom 2", "Save as Zoom 3",
    "Zoom to fit Selection vertically and horizontally, store Navigation Snapshot", "Zoom to fit Selection horizontally, store Navigation Snapshot", "Zoom to fit Selection vertically, store Navigation Snapshot",
    "Zoom to fit All Contents, store Navigation Snapshot", "Zoom to fit Locators, store Navigation Snapshot", "Toggle Zoom to fit Selection or All Contents",
    "Store Navigation Snapshot", "Navigation: Back", "Navigation: Forward",
    "Page Up", "Page Down", "1/3 Page Left", "1/3 Page Right", "1/8 Page Left", "1/8 Page Right", "Page Top", "Page Bottom", "Page Left-Most", "Page Right-Most",
    "Scroll in Play", "Show/Hide Control Bar",
    // windows and views
    "Show Smart Controls", "Show List Editors", "Show Note Pads", "Show Project Files",
    "Open Smart Controls", "Open Smart Controls…", "Open Score Editor", "Open Step Sequencer", "Open Event List", "Show/Hide Event List", "Open Signature List", "Open Signature List…", "Show/Hide Signature List", "Show/Hide Tempo List",
    "Show/Hide All Files", "Show Play Surface", "Open MIDI Transform", "Open Audio Track Editor", "Show/Hide Audio Track Editor", "Open Audio File Editor", "Open Audio File Editor…", "Show/Hide Audio File Editor", "Open Smart Tempo Editor", "Open Smart Tempo Editor…",
    "Minimize All", "Minimize All Windows", "Zoom All", "Fill", "Center", "Left", "Top", "Bottom", "Top Left", "Top Right", "Bottom Left", "Bottom Right", "Return to Previous Size", "Bring All to Front", "Arrange in Front", "Left of Screen", "Right of Screen",
    "Hide Others", "Close Project without Saving",
    // settings tabs
    "Open General Settings…", "Open Audio Settings…", "Open MIDI Settings…", "Open View Settings…", "Open Score Settings…", "Open Movie Settings…", "Open Automation Settings…", "Open Control Surfaces Settings…", "Open My Info Settings…",
    // help
    "Quick Help", "Inspector Pane", "Floating Window", "Bubbles at Pointer Location", "Switch Quick Help Appearance between Bubbles and Inspector Pane/Float Window", "Show/Hide Help Tags",
    "Bramble Studio Help", "Show Detailed Help", "Bramble Studio Instruments", "Bramble Studio Effects", "What’s New in Bramble Studio", "Release Notes", "Bramble Studio Support", "Bramble Studio on the Web", "Bramble Studio Discussions",
];

const WEB: &str = "https://bramble-apps.github.io";
const REPO: &str = "https://github.com/bramble-apps/bramble-office";

/// Runs a navigation command; false when it isn't one (or can't run now).
pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    let bar = app.bar_ticks();
    let pos = app.position();
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let beat = WHOLE / sig.1.max(1) as u32;
    match name {
        // ---- transport
        "Play or Stop" | "Play" => {
            if app.playing() {
                // (Space stops where it is; the Stop button follows its options: stopbutton.rs)
                if name == "Play or Stop" {
                    halt(app);
                }
                return true;
            }
            let reg = app.region_selection().first().and_then(|(t, r)| app.project.as_ref()?.tracks.get(*t)?.regions.get(*r).map(|x| x.start));
            let n = &app.nav;
            // (cycle on: Logic starts at the cycle start anyway)
            let cyc = (app.cycle_on() && n.play_cycle).then_some(app.cycle.0);
            if let Some(at) = play_start((n.play_marquee, false, n.play_region, n.play_last), marquee(app).map(|m| m.0), None, reg, app.stopbtn.last_pos).or(cyc.map(|c| c as f64)) {
                if !app.cycle_on() || (at >= app.cycle.0 as f64 && at < app.cycle.1 as f64) {
                    app.locate(at);
                }
            }
            app.play_stop();
        }
        "Stop or Play From Last Position" => {
            if app.playing() {
                halt(app);
            } else {
                let at = app.stopbtn.last_pos;
                app.locate(at);
                play(app);
            }
        }
        "Play or Stop and Go to Last Locate Position" => {
            if app.playing() {
                halt(app);
                let at = app.stopbtn.last_pos;
                app.locate(at);
            } else {
                play(app);
            }
        }
        "Rewind one Frame" | "Forward one Frame" | "Rewind by Division Value" | "Forward by Division Value" | "Rewind one 1/8 Note" | "Forward one 1/8 Note" | "Rewind one 1/16 Note" | "Forward one 1/16 Note" | "Rewind by Nudge Value" | "Forward by Nudge Value" => {
            let step = if name.contains("Frame") {
                // a 25 fps frame at the tempo here
                (app.tempo() / 60.0 * PPQ as f64 / 25.0).max(1.0)
            } else if name.contains("Division") {
                app.nav.division as f64
            } else if name.contains("1/8") {
                note_ticks(8, false) as f64
            } else if name.contains("1/16") {
                note_ticks(16, false) as f64
            } else {
                app.nudge.max(1) as f64
            };
            let to = if name.starts_with("Rewind") { (pos - step).max(crate::projend::start(app) as f64) } else { pos + step };
            app.locate(to);
        }
        "Play from Beginning" => {
            halt(app);
            app.locate(crate::projend::start(app) as f64);
            play(app);
        }
        "Play from Previous Bar" => {
            halt(app);
            let b = bar as f64;
            app.locate(((pos / b).ceil() - 1.0).max(0.0) * b);
            play(app);
        }
        "Play from Left Locator" | "Play from Right Locator" => {
            halt(app);
            let at = if name.ends_with("Left Locator") { app.cycle.0 } else { app.cycle.1 };
            app.locate(at as f64);
            play(app);
        }
        "Play from Left Window Edge" => {
            halt(app);
            app.locate(app.scroll_x as f64 * bar as f64);
            play(app);
        }
        "Go to Last Locate Position" => {
            let at = app.stopbtn.last_pos;
            app.locate(at);
        }
        "Stop and Go to Beginning" => {
            halt(app);
            app.locate(crate::projend::start(app) as f64);
        }
        "Go to Position…" | "Position…" => app.nav.ask = Some(Ask::GoToPosition(String::new())),
        "Go to Beginning of First Region" => {
            let a = app.project.as_ref().and_then(|p| p.tracks.iter().flat_map(|t| t.regions.iter().map(|r| r.start)).min()).unwrap_or(0);
            app.locate(a as f64);
        }
        "Go to End" => {
            let e = project_end(app);
            app.locate(e as f64);
        }
        "Left Locator" => return app.key_command(ctx, "Go to Left Locator"),
        "Right Locator" => return app.key_command(ctx, "Go to Right Locator"),
        "Selection Start" => return app.key_command(ctx, "Go to Selection Start"),
        "Selection End" => return app.key_command(ctx, "Go to Selection End"),
        "Next Marker" => return app.key_command(ctx, "Go to Next Marker"),
        "Previous Marker" => return app.key_command(ctx, "Go to Previous Marker"),
        // ---- locators
        "Set Left Locator numerically…" => app.nav.ask = Some(Ask::LeftLocator(String::new())),
        "Set Right Locator numerically…" => app.nav.ask = Some(Ask::RightLocator(String::new())),
        "Set Left Locator by Playhead" | "Set Left Locator by Rounded Playhead" | "Set Right Locator by Playhead" | "Set Right Locator by Rounded Playhead" => {
            let mut at = pos as u32;
            if name.contains("Rounded") {
                at = ((pos / bar as f64).round() as u32) * bar;
            }
            let (a, b) = app.cycle;
            let c = if name.contains("Left") { (at, b.max(at + 1)) } else { (a.min(at.saturating_sub(1)), at.max(1)) };
            app.set_cycle(c);
        }
        "Set Locators/Loop by Regions/Events/Marquee" | "Set Rounded Locators/Loop by Regions/Events/Marquee" => return locators_by_selection(app, name.contains("Rounded")),
        "Set Locators and Play" | "Set Rounded Locators and Play" | "Set Rounded Locators and Cycle Play" => {
            if !locators_by_selection(app, name.contains("Rounded")) {
                return false;
            }
            if name.contains("Cycle") && !app.cycle_on() {
                app.toggle_cycle();
            }
            halt(app);
            app.locate(app.cycle.0 as f64);
            play(app);
        }
        "Move Locators/Loop Forward by 1 Bar" => {
            let (a, b) = app.cycle;
            app.set_cycle((a + bar, b + bar));
        }
        "Move Locators/Loop Backwards by 1 Bar" => {
            let (a, b) = app.cycle;
            if a >= bar {
                app.set_cycle((a - bar, b - bar));
            }
        }
        "Move Locators Backwards by Cycle Length" => return app.key_command(ctx, "Move Locators Backwards by Cycle Length / Loop Backwards by Loop Length"),
        "Toggle Auto Set Locators" | "Enable Auto Set Locators" => {
            app.nav.auto_locators = !app.nav.auto_locators;
            app.nav.auto_seen = (app.region_selection(), marquee(app), app.selected_notes.len());
            app.tell(if app.nav.auto_locators { "Auto Set Locators on" } else { "Auto Set Locators off" });
        }
        "By Marquee Selection" => app.nav.auto_marquee = !app.nav.auto_marquee,
        "By Region Selection" => app.nav.auto_region = !app.nav.auto_region,
        "By Marker/Chord Selection" => app.nav.auto_marker = !app.nav.auto_marker,
        "By Note Selection" => app.nav.auto_note = !app.nav.auto_note,
        "Play from Marquee Selection" => app.nav.play_marquee = !app.nav.play_marquee,
        "Play from Cycle Start" => app.nav.play_cycle = !app.nav.play_cycle,
        "Play From Selected Region" => app.nav.play_region = !app.nav.play_region,
        "Play From Last Locate Position" => app.nav.play_last = !app.nav.play_last,
        // ---- markers
        "Go to Marker Number…" | "Marker Number…" => app.nav.ask = Some(Ask::MarkerNumber(String::new())),
        n if n.starts_with("Go to Marker Number ") => {
            let k: usize = n["Go to Marker Number ".len()..].trim().parse().unwrap_or(0);
            // the bare digit keys 1-9 are Logic's screenset keys too (Recall Screenset n): they
            // go to the screensets; no such marker, likewise
            if k <= 9 && (app.screens.tens.is_some() || ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { key, pressed: true, modifiers, .. } if modifiers.is_none() && key.name().len() == 1 && key.name().chars().all(|c| c.is_ascii_digit()))))) {
                return false;
            }
            let Some(at) = k.checked_sub(1).and_then(|i| app.markers.get(i)).map(|m| m.0) else { return false };
            app.locate(at as f64);
        }
        "Create Marker by Cycle Area" => {
            let at = app.cycle.0;
            if !app.markers.iter().any(|m| m.0 == at) {
                let n = app.markers.len() + 1;
                app.markers.push(lg_core::markers::Marker::new(at, format!("Marker {n}")));
                app.markers.sort_by_key(|m| m.0);
                app.dirty = true;
            }
        }
        "Open Marker List" => open_list(app, 1, false),
        "Show/Hide Marker List" => open_list(app, 1, true),
        "Create Arrangement Marker" => {
            // after the last one, 8 bars, the next section name (as the Arrangement track's +)
            let start = app.arrangement.last().map_or(0, |a| a.0 + a.1);
            let name = crate::tracks::SECTIONS[app.arrangement.len() % crate::tracks::SECTIONS.len()].to_string();
            app.arrangement.push((start, 8 * bar, name));
            app.global_tracks = true;
            app.dirty = true;
        }
        "Create Marker for Region Selection" => return app.key_command(ctx, "Create Marker for Region/Marquee Selection"),
        "Create Without Rounding" => return app.key_command(ctx, "Create Marker without Rounding"),
        "Delete Marker at Playhead Position" => return app.key_command(ctx, "Delete Marker"),
        // ---- division, nudge, tempo
        "Set Next Higher Division" => app.nav.division = division_step(app.nav.division, true),
        "Set Next Lower Division" => app.nav.division = division_step(app.nav.division, false),
        n if n.starts_with("Set Division Value to 1/") => {
            let Some(d) = note_value(n) else { return false };
            app.nav.division = d;
            app.tell(format!("Division: {}", &n["Set Division Value to ".len()..]));
        }
        "Set Nudge Value to 10 Ticks" => app.nudge = 10,
        "Set Nudge Value to 5 SMPTE Frame" | "Set Nudge Value to SMPTE Bit" => {
            let secs = if name.contains('5') { 5.0 / 25.0 } else { 1.0 / 25.0 / 80.0 };
            app.nudge = ((secs * app.tempo() / 60.0 * PPQ as f64).round() as u32).max(1);
        }
        n if n.starts_with("Set Nudge Value to 1/") => {
            let Some(d) = note_value(n) else { return false };
            app.nudge = d;
        }
        "Tap Tempo" => {
            let t = now();
            let n = &mut app.nav;
            if n.taps.last().is_some_and(|&l| t - l > 2.0) {
                n.taps.clear();
            }
            n.taps.push(t);
            if n.taps.len() > 8 {
                n.taps.remove(0);
            }
            if let Some(bpm) = tap_tempo(&app.nav.taps) {
                if app.nav.taps.len() == 2 {
                    app.checkpoint("Tap Tempo");
                }
                app.set_tempo(bpm.round());
                app.tell(format!("Tempo {:.0}", bpm.round()));
            }
        }
        // ---- zoom and scroll
        "Zoom Horizontal In" | "Zoom Horizontal Out" => {
            // ≈1.3× a press, the left edge kept (GESTURES-SCROLL.txt: 17 → 13 → 10 bars)
            app.bar_w = if name.ends_with("In") { (app.bar_w * 1.3).min(1200.0) } else { (app.bar_w / 1.3).max(8.0) };
        }
        "Recall Zoom 1" | "Recall Zoom 2" | "Recall Zoom 3" => {
            let i = name.as_bytes()[name.len() - 1] - b'1';
            let Some(v) = app.nav.zooms[i as usize] else { return false };
            snapshot(app);
            set_view(app, v);
        }
        "Save as Zoom 1" | "Save as Zoom 2" | "Save as Zoom 3" => {
            let i = name.as_bytes()[name.len() - 1] - b'1';
            app.nav.zooms[i as usize] = Some(view(app));
            app.tell(format!("Zoom {} saved", i + 1));
        }
        "Zoom to fit Selection vertically and horizontally, store Navigation Snapshot" | "Zoom to fit Selection horizontally, store Navigation Snapshot" | "Zoom to fit Selection vertically, store Navigation Snapshot" => {
            let Some(span) = marquee(app).or_else(|| selection_span(app)) else { return false };
            snapshot(app);
            if name.contains("horizontally") {
                fit_horizontal(app, span);
            }
            if name.contains("vertically") {
                fit_vertical(app);
            }
        }
        "Zoom to fit All Contents, store Navigation Snapshot" => {
            snapshot(app);
            let s = contents_span(app);
            fit_horizontal(app, s);
            app.ta.v_zoom = 1.0;
            app.scroll_y = 0.0;
        }
        "Zoom to fit Locators, store Navigation Snapshot" => {
            snapshot(app);
            let c = app.cycle;
            fit_horizontal(app, c);
        }
        "Toggle Zoom to fit Selection or All Contents" => {
            // Logic: fit the selection; pressed again (nothing changed since), back where it was
            let span = marquee(app).or_else(|| selection_span(app)).unwrap_or_else(|| contents_span(app));
            let (w, s) = fit(app.tracks_w, span.0, span.1, bar);
            if (app.bar_w - w).abs() < 0.01 && (app.scroll_x - s).abs() < 0.001 {
                let now = view(app);
                if let Some(v) = app.nav.history.back(now) {
                    set_view(app, v);
                }
            } else {
                snapshot(app);
                app.bar_w = w;
                app.scroll_x = s;
            }
        }
        "Store Navigation Snapshot" => {
            snapshot(app);
            app.tell("Navigation snapshot stored");
        }
        "Navigation: Back" | "Navigation: Forward" => {
            let now = view(app);
            let v = if name.ends_with("Back") { app.nav.history.back(now) } else { app.nav.history.forward(now) };
            let Some(v) = v else { return false };
            set_view(app, v);
        }
        "Page Up" | "Page Down" | "Page Top" | "Page Bottom" => {
            let total = crate::trackarea::total_h(&crate::trackarea::rows(app));
            let max = (total - app.nav.lanes_h + 40.0).max(0.0);
            let h = app.nav.lanes_h.max(40.0);
            app.scroll_y = match name {
                "Page Up" => app.scroll_y - h,
                "Page Down" => app.scroll_y + h,
                "Page Top" => 0.0,
                _ => max,
            }
            .clamp(0.0, max);
        }
        "1/3 Page Left" | "1/3 Page Right" | "1/8 Page Left" | "1/8 Page Right" | "Page Left-Most" | "Page Right-Most" => {
            let page = app.tracks_w / app.bar_w;
            app.scroll_x = match name {
                "Page Left-Most" => 0.0,
                "Page Right-Most" => (project_end(app) as f32 / bar as f32 - page * 0.9).max(0.0),
                n => {
                    let f = if n.starts_with("1/3") { page / 3.0 } else { page / 8.0 };
                    app.scroll_x + if n.ends_with("Right") { f } else { -f }
                }
            }
            .max(0.0);
            if app.playing() {
                app.catch_off = true;
            }
        }
        "Scroll in Play" => {
            app.nav.scroll_in_play = !app.nav.scroll_in_play;
            app.tell(if app.nav.scroll_in_play { "Scroll in Play on" } else { "Scroll in Play off" });
        }
        "Show/Hide Control Bar" => app.show_control_bar = !app.show_control_bar,
        // ---- windows and views
        "Show Smart Controls" | "Open Smart Controls" | "Open Smart Controls…" => app.bottom = if app.bottom == Bottom::SmartControls && name.starts_with("Show") { Bottom::None } else { Bottom::SmartControls },
        "Show List Editors" => return app.key_command(ctx, "Show/Hide List Editors"),
        "Show Note Pads" => return app.key_command(ctx, "Show/Hide Note Pad"),
        "Show Project Files" => return app.key_command(ctx, "Show/Hide Project Files"),
        "Show/Hide All Files" => return app.key_command(ctx, "Show/Hide All Files Browser"),
        "Open Score Editor" => return app.key_command(ctx, "Open Score Editor…"),
        "Open Step Sequencer" => return app.key_command(ctx, "Open Step Sequencer…"),
        "Open Event List" => open_list(app, 0, false),
        "Show/Hide Event List" => open_list(app, 0, true),
        "Show/Hide Tempo List" => open_list(app, 2, true),
        "Open Signature List" | "Open Signature List…" => open_list(app, 3, false),
        "Show/Hide Signature List" => open_list(app, 3, true),
        "Show Play Surface" => return crate::playsurface::command(app, "Show Play Surface"),
        "Open MIDI Transform" => return app.key_command(ctx, "Open Transform…"),
        "Open Audio Track Editor" | "Show/Hide Audio Track Editor" | "Open Audio File Editor" | "Open Audio File Editor…" | "Show/Hide Audio File Editor" | "Open Smart Tempo Editor" | "Open Smart Tempo Editor…" => {
            if !crate::audioedit::is_audio_region(app) {
                app.tell("Select an audio region first");
                return true;
            }
            let tab = if name.contains("File") { crate::audioedit::Tab::File } else if name.contains("Smart Tempo") { crate::audioedit::Tab::SmartTempo } else { crate::audioedit::Tab::Track };
            if name.starts_with("Show/Hide") && app.bottom == Bottom::PianoRoll && app.audio_edit.tab == tab {
                app.bottom = Bottom::None;
            } else {
                app.audio_edit.tab = tab;
                app.bottom = Bottom::PianoRoll;
            }
        }
        "Minimize All" | "Minimize All Windows" => crate::chrome::minimize(ctx),
        "Zoom All" => return app.key_command(ctx, "Zoom Window"),
        "Fill" => place(app, ctx, |m| m),
        "Center" => {
            let size = ctx.input(|i| i.viewport().outer_rect.map(|r| r.size()));
            place(app, ctx, |m| egui::Rect::from_center_size(m.center(), size.unwrap_or(m.size() * 0.8).min(m.size())));
        }
        "Left" | "Right" | "Top" | "Bottom" | "Top Left" | "Top Right" | "Bottom Left" | "Bottom Right" | "Left of Screen" | "Right of Screen" => {
            let n = name.trim_end_matches(" of Screen").to_string();
            place(app, ctx, move |m| {
                let (w, h) = (m.width() / 2.0, m.height() / 2.0);
                let x = if n.contains("Right") { m.min.x + w } else { m.min.x };
                let y = if n.contains("Bottom") { m.min.y + h } else { m.min.y };
                let full_w = n == "Top" || n == "Bottom";
                let full_h = n == "Left" || n == "Right";
                egui::Rect::from_min_size(egui::pos2(if full_w { m.min.x } else { x }, if full_h { m.min.y } else { y }), egui::vec2(if full_w { m.width() } else { w }, if full_h { m.height() } else { h }))
            });
        }
        "Return to Previous Size" => {
            let Some(r) = app.nav.prev_window.take() else { return false };
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(r.min));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(r.size()));
        }
        "Bring All to Front" | "Arrange in Front" => ctx.send_viewport_cmd(egui::ViewportCommand::Focus),
        // (one window: nothing else of Studio's to hide)
        "Hide Others" => {}
        "Close Project without Saving" => {
            app.stop();
            app.dirty = false;
            app.set_project(crate::demo::song(), true);
        }
        // ---- settings
        "Open General Settings…" => settings_tab(app, "General"),
        "Open Audio Settings…" => settings_tab(app, "Audio"),
        "Open MIDI Settings…" => settings_tab(app, "MIDI"),
        "Open View Settings…" => settings_tab(app, "View"),
        "Open Score Settings…" => settings_tab(app, "Score"),
        "Open Movie Settings…" => settings_tab(app, "Movie"),
        "Open Automation Settings…" => settings_tab(app, "Automation"),
        "Open Control Surfaces Settings…" => settings_tab(app, "Control Surfaces"),
        "Open My Info Settings…" => settings_tab(app, "My Info"),
        // ---- help
        "Quick Help" => app.quick_help = !app.quick_help,
        "Inspector Pane" => app.nav.quick_help_as = 0,
        "Floating Window" => app.nav.quick_help_as = 1,
        "Bubbles at Pointer Location" => app.nav.quick_help_as = 2,
        "Switch Quick Help Appearance between Bubbles and Inspector Pane/Float Window" => app.nav.quick_help_as = if app.nav.quick_help_as == 2 { 1 } else { 2 },
        "Show/Hide Help Tags" => {
            let on = app.logic_settings.get("View / General / Show help tags") != "0";
            app.logic_settings.0.insert("View / General / Show help tags".into(), if on { "0" } else { "1" }.into());
            app.tell(if on { "Help tags off" } else { "Help tags on" });
        }
        "Bramble Studio Help" | "Show Detailed Help" => kn_ui::open_url(&format!("{REPO}/tree/main/logic-linux#readme")),
        "Bramble Studio Instruments" | "Bramble Studio Effects" => kn_ui::open_url(&format!("{REPO}/blob/main/logic-linux/CHECKLIST.md")),
        "What’s New in Bramble Studio" | "Release Notes" => kn_ui::open_url(&format!("{REPO}/releases")),
        "Bramble Studio Support" => kn_ui::open_url(&format!("{REPO}/issues")),
        "Bramble Studio Discussions" => kn_ui::open_url(&format!("{REPO}/discussions")),
        "Bramble Studio on the Web" => kn_ui::open_url(WEB),
        _ => return crate::browser::command(app, name),
    }
    let _ = beat;
    true
}

/// Navigate's option submenus (Play Button Options, Auto Set Locators) and Help's Quick
/// Help appears as: their items are choices (they share names with key commands), drawn here.
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    if !matches!(it.label.as_str(), "Play Button Options" | "Auto Set Locators" | "Quick Help appears as") {
        return false;
    }
    ui.menu_button(&it.label, |ui| {
        ui.set_min_width(260.0);
        for c in &it.children {
            if c.separator {
                ui.separator();
                continue;
            }
            let on = checked(app, &c.label).unwrap_or(false);
            // (Auto Set Locators' sources apply once it's enabled, as Logic greys them)
            let can = it.label != "Auto Set Locators" || c.label == "Enable Auto Set Locators" || app.nav.auto_locators;
            let b = egui::Button::new(if on { format!("✓ {}", c.label) } else { format!("   {}", c.label) });
            if ui.add_enabled(can, b).clicked() {
                let ctx = ui.ctx().clone();
                command(app, &ctx, &c.label);
                ui.close();
            }
        }
    });
    true
}

/// "…to 1/16 Note", "…to 1/8 Triplet (1/12)": the value in ticks.
fn note_value(name: &str) -> Option<u32> {
    let i = name.find("1/")?;
    let rest = &name[i + 2..];
    let denom: u32 = rest.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()?;
    Some(note_ticks(denom, name.contains("Triplet")))
}

/// A menu item's check mark, for the items here.
pub fn checked(app: &App, label: &str) -> Option<bool> {
    let n = &app.nav;
    Some(match label {
        "Play from Marquee Selection" => n.play_marquee,
        "Play from Cycle Start" => n.play_cycle,
        "Play From Selected Region" => n.play_region,
        "Play From Last Locate Position" => n.play_last,
        "By Marquee Selection" => n.auto_marquee,
        "By Region Selection" => n.auto_region,
        "By Marker/Chord Selection" => n.auto_marker,
        "By Note Selection" => n.auto_note,
        "Enable Auto Set Locators" => n.auto_locators,
        "Quick Help" => app.quick_help,
        "Inspector Pane" => n.quick_help_as == 0,
        "Floating Window" => n.quick_help_as == 1,
        "Bubbles at Pointer Location" => n.quick_help_as == 2,
        "Scroll in Play" => n.scroll_in_play,
        _ => return None,
    })
}

/// The View menu's Show / Hide labels as the panes are.
pub fn menu_label(app: &App, label: &str) -> Option<String> {
    let flip = |on: bool, show: &str| if on { show.replacen("Show", "Hide", 1) } else { show.to_string() };
    Some(match label {
        "Show Smart Controls" => flip(app.bottom == Bottom::SmartControls, label),
        "Show List Editors" => flip(app.right_pane == Some(RightPane::Lists), label),
        "Show Note Pads" => flip(app.right_pane == Some(RightPane::Notes), label),
        "Show Project Files" => flip(app.right_pane == Some(RightPane::Loops), label),
        "Show Toolbar" => flip(app.show_toolbar, label),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_values_in_ticks() {
        assert_eq!(note_ticks(4, false), 960);
        assert_eq!(note_ticks(16, false), 240);
        assert_eq!(note_ticks(8, true), 320);
        assert_eq!(note_ticks(1, false), 3840);
        assert_eq!(note_value("Set Division Value to 1/16 Triplet (1/24)"), Some(160));
        assert_eq!(note_value("Set Nudge Value to 1/2 Triplet (1/3)"), Some(1280));
        assert_eq!(note_value("Set Division Value to 1/128 Note"), Some(30));
    }

    #[test]
    fn divisions_step_finer_and_coarser() {
        assert_eq!(division_step(240, true), 160);
        assert_eq!(division_step(240, false), 320);
        assert_eq!(division_step(960, false), 960);
        assert_eq!(division_step(20, true), 20);
        // (not one of the list: to the nearest one each way)
        assert_eq!(division_step(200, false), 240);
        assert_eq!(division_step(200, true), 160);
    }

    #[test]
    fn positions_typed_as_logic_writes_them() {
        assert_eq!(parse_position("1 1 1 1", (4, 4), 240), Some(0));
        assert_eq!(parse_position("9", (4, 4), 240), Some(8 * 3840));
        assert_eq!(parse_position("2.2.4.1", (4, 4), 240), Some(3840 + 960 + 720));
        assert_eq!(parse_position("3 2", (3, 4), 240), Some(2 * 2880 + 960));
        assert_eq!(parse_position("2 1", (6, 8), 240), Some(6 * 480));
        assert_eq!(parse_position("x", (4, 4), 240), None);
        assert_eq!(parse_position("", (4, 4), 240), None);
        // typed back as written
        let t = 5 * 3840 + 2 * 960 + 3 * 240 + 17;
        assert_eq!(parse_position(&crate::app::bbdt(t, (4, 4)), (4, 4), 240), Some(t));
        assert_eq!(parse_time("1:30"), Some(90.0));
        assert_eq!(parse_time("1:00:02.5"), Some(3602.5));
        assert_eq!(parse_time("12"), None);
    }

    #[test]
    fn fit_and_follow() {
        // 8 bars into 832 points: 100 points a bar, from bar 3
        let (w, s) = fit(832.0, 2 * 3840, 10 * 3840, 3840);
        assert!((w - 100.0).abs() < 0.01, "{w}");
        assert_eq!(s, 2.0);
        // Catch: a page when the playhead nears the right edge
        assert_eq!(follow(0.0, 5.0, 16.0, false), 0.0);
        assert_eq!(follow(0.0, 15.0, 16.0, false), 15.0);
        assert_eq!(follow(10.0, 4.5, 16.0, false), 4.0);
        // Scroll in Play: the playhead in the middle
        assert_eq!(follow(0.0, 20.0, 16.0, true), 12.0);
        assert_eq!(follow(0.0, 3.0, 16.0, true), 0.0);
    }

    #[test]
    fn taps_make_a_tempo() {
        assert_eq!(tap_tempo(&[0.0]), None);
        let t = tap_tempo(&[0.0, 0.5, 1.0, 1.5]).unwrap();
        assert!((t - 120.0).abs() < 1e-9);
        // a pause longer than two seconds isn't an interval
        assert_eq!(tap_tempo(&[0.0, 5.0]), None);
    }

    #[test]
    fn navigation_back_and_forward() {
        let v = |x: f32| View { bar_w: 70.0, scroll_x: x, scroll_y: 0.0, v_zoom: 1.0 };
        let mut h = History::default();
        assert_eq!(h.back(v(0.0)), None);
        h.store(v(1.0));
        h.store(v(2.0));
        assert_eq!(h.back(v(3.0)), Some(v(2.0)));
        assert_eq!(h.back(v(2.0)), Some(v(1.0)));
        assert_eq!(h.forward(v(1.0)), Some(v(2.0)));
        assert_eq!(h.forward(v(2.0)), Some(v(3.0)));
        assert_eq!(h.forward(v(3.0)), None);
        // a new snapshot drops what Forward had
        h.back(v(3.0));
        h.store(v(9.0));
        assert_eq!(h.forward(v(9.0)), None);
    }

    #[test]
    fn play_button_options() {
        // marquee first, then cycle, region, last locate
        assert_eq!(play_start((true, true, true, true), Some(100), Some(200), Some(300), 400.0), Some(100.0));
        assert_eq!(play_start((true, true, true, true), None, Some(200), Some(300), 400.0), Some(200.0));
        assert_eq!(play_start((false, false, true, false), Some(100), Some(200), Some(300), 400.0), Some(300.0));
        assert_eq!(play_start((false, false, false, true), None, None, None, 400.0), Some(400.0));
        assert_eq!(play_start((false, false, false, false), Some(1), Some(2), Some(3), 4.0), None);
    }
}

#[cfg(test)]
mod names {
    /// Every command here is spelled as Logic's (key commands, menus) or is one of Studio's own.
    #[test]
    fn commands_are_logics() {
        let data = [include_str!("../data/commands.txt"), include_str!("../data/menus.txt"), include_str!("../data/local-menus.txt"), include_str!("../data/context-menus.txt")].concat();
        let own = ["Tuner…", "Show Toolbar"];
        for list in [super::COMMANDS, crate::screensets::COMMANDS, crate::toolbar::COMMANDS, crate::windows::COMMANDS] {
            for c in list {
                let found = own.contains(c) || data.lines().any(|l| l.split('\t').nth(1).is_some_and(|x| x == *c) || l.trim().trim_start_matches("✓ ").split("  ").next().is_some_and(|x| x.trim_end_matches(" ▸") == *c));
                assert!(found, "{c:?} isn't one of Logic's names");
            }
        }
    }
}
