//! Studio's MIDI studio around the song: the MIDI Environment (envwin.rs), control surfaces and
//! controller assignments (surfaces.rs), synchronization (syncmidi.rs), the movie (movie.rs) and
//! the MIDI outputs they share (midiout.rs). The app calls in here through a few hooks.

use crate::app::App;

#[derive(Default)]
pub struct Hub {
    pub env: crate::envwin::State,
    pub surf: crate::surfaces::State,
    pub sync: crate::syncmidi::State,
    pub movie: crate::movie::State,
    pub outs: crate::midiout::Outs,
}

/// A whole MIDI message from input `port`, before anything else sees it: control surfaces and
/// controller assignments (remote control events stop here) and sync (MTC, MMC). True when
/// taken.
pub fn midi_raw(app: &mut App, ctx: &egui::Context, port: usize, bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    if crate::syncmidi::midi_in(app, ctx, port, bytes) {
        return true;
    }
    crate::surfaces::midi_in(app, ctx, port, bytes)
}

/// Each frame (before the windows): sync out, surface feedback, the movie.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    crate::syncmidi::frame(app, ctx);
    crate::surfaces::frame(app, ctx);
    crate::movie::frame(app, ctx);
    crate::viewprefs::frame(app);
}

/// The windows: Environment, its object windows, Control Surfaces Setup, Controller
/// Assignments, the Movie window.
pub fn windows(app: &mut App, ctx: &egui::Context) {
    crate::envwin::window(app, ctx);
    crate::surfaces::windows(app, ctx);
    crate::movie::window(app, ctx);
}

/// Key commands and menu items of these areas.
pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    crate::envwin::command(app, ctx, name) || crate::surfaces::command(app, ctx, name) || crate::syncmidi::command(app, ctx, name) || crate::movie::command(app, ctx, name)
}

/// Keys while the pointer is over one of these windows.
pub fn key_press(app: &mut App, ctx: &egui::Context, key: egui::Key, m: egui::Modifiers) -> bool {
    crate::envwin::key_press(app, ctx, key, m) || crate::surfaces::key_press(app, ctx, key, m)
}

/// Menu ticks.
pub fn checked(app: &App, label: &str) -> Option<bool> {
    crate::surfaces::checked(app, label).or_else(|| crate::syncmidi::checked(app, label)).or_else(|| crate::movie::checked(app, label))
}

pub fn after_open(app: &mut App) {
    crate::envwin::after_open(app);
    crate::movie::after_open(app);
}

pub fn save(app: &mut App) {
    crate::envwin::save(app);
    crate::movie::save(app);
}
