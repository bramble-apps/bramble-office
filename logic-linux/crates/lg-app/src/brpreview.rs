//! Browser previews as Logic's results list does them (guide: Work with the results list):
//! a click on a sound's icon starts its preview and a second click stops it; Option-click
//! retriggers it; a click on the already-selected sound starts or stops it; Right Arrow starts
//! or retriggers the selected one; dragging across icons, or moving over them with Option held,
//! previews each in turn; while a sound previews its icon shows a speaker. Play Preview on
//! Selection (More menu, per view, on by default) previews on the first click that selects.
//! Loops preview looped, one-shots once; instrument patches play a short chord with the patch's
//! sound, rendered by Studio's engine apart from the song (the selected track keeps its patch).
//! User patches can't be previewed.
//!
//! The chord: nothing in the research records which chord Logic plays, so Studio plays a
//! C major triad, C3–E3–G3, held 1.5 seconds (Logic numbers middle C as C3: MIDI 60 is C3,
//! so C3–E3–G3 is MIDI 60, 64, 67).

use crate::app::App;
use crate::soundlib::{Item, Kind, LoopType};

pub const COMMANDS: &[&str] = &["Play Preview on Selection"];

/// The preview chord (MIDI notes) and how long it is held.
pub const CHORD: [u8; 3] = [60, 64, 67];
pub const CHORD_SECONDS: f64 = 1.5;
const CHORD_VELOCITY: u8 = 96;

/// What a click on a result does to its preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Start,
    Stop,
    Retrigger,
}

/// Where the click landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    /// The sound's icon (the type mark); `option` held: retrigger.
    Icon { option: bool },
    /// The row; `was_selected`: it was the selected sound before this click.
    Row { was_selected: bool },
    /// Right Arrow on the selected sound.
    RightArrow,
}

/// Logic's rules for a click on a previewable sound.
pub fn decide(click: Click, playing: bool, preview_on_select: bool) -> Action {
    match click {
        Click::Icon { option: true } | Click::RightArrow if playing => Action::Retrigger,
        Click::Icon { option: true } | Click::RightArrow => Action::Start,
        Click::Icon { .. } | Click::Row { was_selected: true } => {
            if playing {
                Action::Stop
            } else {
                Action::Start
            }
        }
        // (Play Preview on Selection only affects the first click on a sound)
        Click::Row { was_selected: false } => {
            if preview_on_select {
                Action::Start
            } else {
                Action::None
            }
        }
    }
}

/// Instrument patches, loops and one-shots preview; user patches, effect patches and plug-in
/// presets don't.
pub fn previewable(it: &Item) -> bool {
    match it.kind {
        Kind::Loop | Kind::OneShot => it.path.is_some(),
        Kind::Patch => it.pack != "Your Patches" && it.pack != crate::plugbrowser::PACK && !(it.loop_type == LoopType::Audio && it.pack == "Bramble Effects"),
    }
}

/// The per-view key of Play Preview on Selection.
pub fn select_key(view: &str) -> String {
    let v = view.split('(').next().unwrap_or(view);
    format!("preview_on_select_{v}")
}

pub fn preview_on_select(app: &App) -> bool {
    app.br.saved.flag(&select_key(&format!("{:?}", app.br.view)), true)
}

/// Carries out `a` for `it`.
pub fn apply(app: &mut App, it: &Item, a: Action) {
    if !previewable(it) {
        return;
    }
    match a {
        Action::None => {}
        Action::Stop => app.preview.stop(),
        Action::Start | Action::Retrigger => {
            app.preview.stop();
            start(app, it);
        }
    }
}

/// A click: decides and acts.
pub fn click(app: &mut App, it: &Item, c: Click) {
    let now = app.ctx.input(|i| i.time);
    let playing = app.preview.is_playing(&it.id, now);
    let a = decide(c, playing, preview_on_select(app));
    apply(app, it, a);
}

/// Dragging across icons or Option-hovering them: each newly reached sound previews.
pub fn sweep(app: &mut App, it: &Item) {
    if app.br.swept.as_deref() == Some(it.id.as_str()) {
        return;
    }
    app.br.swept = Some(it.id.clone());
    apply(app, it, Action::Start);
}

fn start(app: &mut App, it: &Item) {
    match it.kind {
        Kind::Loop | Kind::OneShot => crate::browser::preview_item(app, it, it.kind == Kind::Loop),
        Kind::Patch => chord(app, it),
    }
}

/// The patch's chord, rendered offline by the engine with the patch's instrument and program
/// (the song and the selected track untouched), then played at the preview volume.
fn chord(app: &mut App, it: &Item) {
    let cat = crate::browser::patch_category(&it.name);
    let program = crate::app::patch_program(&cat, &it.name);
    let sf = app.engine.lock().unwrap_or_else(|e| e.into_inner()).soundfont.clone();
    let name = it.name.clone();
    let render = move || render_chord(sf.as_deref(), &name, program);
    let gain = app.br.volume;
    crate::preview::play_rendered(app, &it.id, gain, CHORD_SECONDS + 0.5, render);
}

/// The chord as stereo samples (with a short release tail).
pub fn render_chord(soundfont: Option<&std::path::Path>, instrument: &str, program: u8) -> Vec<f32> {
    let mut e = crate::audio::Engine::new();
    e.tempo = 120.0;
    e.load_soundfont(soundfont);
    let ppq = lg_core::events::PPQ as f64;
    let len = (CHORD_SECONDS * 2.0 * ppq) as u32; // 120 BPM: two beats a second
    let notes = chord_notes(len);
    e.tracks = vec![crate::audio::PlayTrack {
        channel: 0,
        program,
        notes,
        audio: Vec::new(),
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        eq: None,
        instrument: Some(instrument.to_string()),
        instrument_floats: Vec::new(),
        inserts: Vec::new(),
        sends: Vec::new(),
        auto_volume: Vec::new(),
        auto_pan: Vec::new(),
    }];
    e.program_changes();
    crate::audio::render_offline(e, 0, len, 0.5, true)
}

/// The chord's notes (start, end, pitch, velocity), all `len` ticks long.
pub fn chord_notes(len: u32) -> Vec<(u32, u32, u8, u8)> {
    CHORD.iter().map(|&p| (0, len, p, CHORD_VELOCITY)).collect()
}

/// The More menu's Play Preview on Selection (this view's).
pub fn menu_item(app: &mut App, ui: &mut egui::Ui) {
    let k = select_key(&format!("{:?}", app.br.view));
    let on = app.br.saved.flag(&k, true);
    if ui.add(egui::Button::selectable(on, "Play Preview on Selection")).clicked() {
        app.br.saved.set_flag(&k, !on);
        crate::browser::save_state(app);
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_click_starts_then_stops() {
        assert_eq!(decide(Click::Icon { option: false }, false, false), Action::Start);
        assert_eq!(decide(Click::Icon { option: false }, true, true), Action::Stop);
    }

    #[test]
    fn option_click_and_right_arrow_retrigger() {
        assert_eq!(decide(Click::Icon { option: true }, true, true), Action::Retrigger);
        assert_eq!(decide(Click::Icon { option: true }, false, true), Action::Start);
        assert_eq!(decide(Click::RightArrow, true, false), Action::Retrigger);
        assert_eq!(decide(Click::RightArrow, false, false), Action::Start);
    }

    #[test]
    fn selected_sound_toggles_and_play_on_selection_only_first_click() {
        assert_eq!(decide(Click::Row { was_selected: true }, true, true), Action::Stop);
        assert_eq!(decide(Click::Row { was_selected: true }, false, false), Action::Start);
        assert_eq!(decide(Click::Row { was_selected: false }, false, true), Action::Start);
        assert_eq!(decide(Click::Row { was_selected: false }, false, false), Action::None);
    }

    #[test]
    fn per_view_key() {
        assert_eq!(select_key("Instruments"), "preview_on_select_Instruments");
        assert_eq!(select_key("Location(3)"), "preview_on_select_Location");
    }

    #[test]
    fn chord_is_a_c_major_triad_held_together() {
        let n = chord_notes(2880);
        assert_eq!(n.iter().map(|x| x.2).collect::<Vec<_>>(), vec![60, 64, 67]);
        assert!(n.iter().all(|x| x.0 == 0 && x.1 == 2880));
    }

    #[test]
    fn chord_renders_sound_about_one_and_a_half_seconds() {
        let s = render_chord(None, "Steinway Grand Piano", 0);
        let secs = s.len() as f64 / 2.0 / crate::audio::RATE as f64;
        assert!((1.5..=2.1).contains(&secs), "{secs}");
        assert!(s.iter().any(|x| x.abs() > 1e-3), "the chord is audible");
    }
}
