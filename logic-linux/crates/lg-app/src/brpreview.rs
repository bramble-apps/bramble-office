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
    let drums = is_kit(&cat, &it.category) || kit_named(&it.name);
    let (sf, data) = {
        let e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
        (e.soundfont.clone(), e.soundfont_data())
    };
    let name = it.name.clone();
    // (LG_PREVIEW_WAV=<dir>, for checks without a sound device: each chord also goes to a WAV)
    let dump = std::env::var_os("LG_PREVIEW_WAV").map(std::path::PathBuf::from);
    let render: crate::preview::Render = Box::new(move |go: &dyn Fn() -> bool| {
        let _one = Rendering::start();
        let s = render_preview_with(sf.as_deref(), data, &name, program, drums, go);
        if let Some(d) = dump.filter(|_| go()) {
            let (l, r): (Vec<f32>, Vec<f32>) = s.chunks(2).map(|f| (f[0], f.get(1).copied().unwrap_or(0.0))).unzip();
            let rate = crate::audio::RATE;
            let _ = crate::projaudio::write_wav(&d.join(format!("{name}.wav")), &[l, r], rate, rate, 16);
            eprintln!("LG_PREVIEW_WAV {name} program {program}");
        }
        s
    });
    let gain = app.br.volume;
    crate::preview::play_rendered(app, &it.id, gain, CHORD_SECONDS + 0.5, render);
}

/// A drum kit patch (its preview plays the kit, not a chord).
pub fn is_kit(category: &str, item_category: &str) -> bool {
    [category, item_category].iter().any(|c| c.starts_with("Drum"))
}

/// A drum kit by its name: a Drummer / Drum Kit Designer kit, or a patch voiced on a kit.
pub fn kit_named(name: &str) -> bool {
    lg_sounds::for_kit(name).is_some() || lg_synth::patch(name).is_some_and(|s| s.engine == lg_synth::Engine::DrumKit)
}

/// The chord as stereo samples (with a short release tail).
pub fn render_chord(soundfont: Option<&std::path::Path>, instrument: &str, program: u8) -> Vec<f32> {
    render_preview(soundfont, instrument, program, false)
}

/// A patch's preview through the engine, apart from the song: the chord (all three notes even
/// on a patch set to mono, as a preview of its sound), or for a drum kit a bar of the kit
/// (kick, snare, hi-hat) on the drum channel.
pub fn render_preview(soundfont: Option<&std::path::Path>, instrument: &str, program: u8, drums: bool) -> Vec<f32> {
    render_preview_with(soundfont, None, instrument, program, drums, &|| true)
}

/// Renders running now and the most at once (the preview thread renders one at a time).
static RENDERING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static MOST_AT_ONCE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Rendering;

impl Rendering {
    fn start() -> Rendering {
        use std::sync::atomic::Ordering::SeqCst;
        let n = RENDERING.fetch_add(1, SeqCst) + 1;
        MOST_AT_ONCE.fetch_max(n, SeqCst);
        Rendering
    }
}

impl Drop for Rendering {
    fn drop(&mut self) {
        RENDERING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// The most preview renders that ran at once.
pub fn most_at_once() -> usize {
    MOST_AT_ONCE.load(std::sync::atomic::Ordering::SeqCst)
}

/// `render_preview` with the engine's SoundFont already read (`data`, so each preview doesn't
/// read the file again) and a check `go` that stops it early (a newer preview replaced it).
pub fn render_preview_with(soundfont: Option<&std::path::Path>, data: Option<std::sync::Arc<rustysynth::SoundFont>>, instrument: &str, program: u8, drums: bool, go: &dyn Fn() -> bool) -> Vec<f32> {
    let mut e = crate::audio::Engine::new();
    e.tempo = 120.0;
    match data {
        Some(sf) => e.use_soundfont(soundfont.map(std::path::Path::to_path_buf), sf),
        None => e.load_soundfont(soundfont),
    }
    let ppq = lg_core::events::PPQ as f64;
    let len = (CHORD_SECONDS * 2.0 * ppq) as u32; // 120 BPM: two beats a second
    let notes = if drums { kit_notes(len) } else { chord_notes(len) };
    e.tracks = vec![crate::audio::PlayTrack {
        channel: if drums { 9 } else { 0 },
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
    if let Some(ins) = e.units.get_mut(0).and_then(|u| u.instrument.as_mut()) {
        poly(ins.as_mut());
    }
    crate::audio::render_offline_while(e, 0, len, 0.5, true, go)
}

/// A mono-set instrument made polyphonic (three voices at least) for the chord: Polyphony /
/// Voices raised, Keyboard / Voice / MIDI Mono Mode set to Poly.
pub fn poly(ins: &mut dyn lg_dsp::Instrument) {
    let ps: Vec<(u32, &'static str, f32, &'static [&'static str])> = ins.params().iter().map(|p| (p.id, p.name, p.max, p.choices)).collect();
    for (id, name, max, choices) in ps {
        let v = ins.get(id);
        match name {
            "Polyphony" | "Voices" if choices.is_empty() => {
                if v < 3.0 {
                    ins.set(id, 3f32.min(max));
                }
            }
            "Voices" => {
                // (a menu: Mono, Legato, 2, 3 …)
                if let Some(k) = choices.iter().position(|c| *c == "3") {
                    if (v as usize) < k {
                        ins.set(id, k as f32);
                    }
                }
            }
            n if (n.contains("Mono Mode") || n == "Keyboard Mode" || n == "Voice Mode") && !n.contains("Bend") => {
                if let Some(k) = choices.iter().position(|c| *c == "Poly") {
                    ins.set(id, k as f32);
                }
            }
            _ => {}
        }
    }
}

/// A bar of the kit for its preview: kick on 1 and 3, snare on 2 (and 4 when it fits), closed
/// hi-hat on the eighths, ending at `len`.
pub fn kit_notes(len: u32) -> Vec<(u32, u32, u8, u8)> {
    let ppq = lg_core::events::PPQ;
    let eighth = ppq / 2;
    let mut v = Vec::new();
    let mut t = 0;
    let mut k = 0;
    while t < len {
        v.push((t, t + eighth / 2, 42, 80));
        match k % 4 {
            0 => v.push((t, t + eighth, 36, 110)),
            2 => v.push((t, t + eighth, 38, 105)),
            _ => {}
        }
        t += eighth;
        k += 1;
    }
    v
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

    /// Energy of `hz` in mono `x` (Goertzel).
    fn tone(x: &[f32], hz: f32) -> f32 {
        let w = 2.0 * std::f32::consts::PI * hz / crate::audio::RATE as f32;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0f32, 0f32);
        for &v in x {
            let s0 = v + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / x.len().max(1) as f32
    }

    fn mono(s: &[f32]) -> Vec<f32> {
        // (0.1 s to 0.9 s: the held chord)
        let r = crate::audio::RATE as usize;
        s.chunks(2).skip(r / 10).take(r * 8 / 10).map(|f| (f[0] + f.get(1).copied().unwrap_or(0.0)) / 2.0).collect()
    }

    fn hz(midi: u8) -> f32 {
        440.0 * 2f32.powf((midi as f32 - 69.0) / 12.0)
    }

    #[test]
    fn chord_has_its_three_pitches() {
        for patch in ["Steinway Grand Piano", "Deluxe Classic"] {
            let cat = crate::browser::patch_category(patch);
            let s = render_chord(None, patch, crate::app::patch_program(&cat, patch));
            let m = mono(&s);
            let on: Vec<f32> = CHORD.iter().map(|&p| tone(&m, hz(p))).collect();
            // (between the chord's notes: D and F, not played)
            let off = tone(&m, hz(62)).max(tone(&m, hz(65)));
            eprintln!("{patch}: chord {on:?} off {off}");
            for (k, e) in on.iter().enumerate() {
                assert!(*e > 2.0 * off, "{patch}: pitch {} missing ({on:?} vs {off})", CHORD[k]);
            }
        }
    }

    #[test]
    fn two_patches_sound_different() {
        let a = render_chord(None, "Steinway Grand Piano", crate::app::patch_program(&crate::browser::patch_category("Steinway Grand Piano"), "Steinway Grand Piano"));
        let b = render_chord(None, "Classic Electric Piano", crate::app::patch_program(&crate::browser::patch_category("Classic Electric Piano"), "Classic Electric Piano"));
        // harmonic profiles (C3's first eight partials), normalised
        let prof = |s: &[f32]| {
            let m = mono(s);
            let v: Vec<f32> = (1..=8).map(|k| tone(&m, hz(60) * k as f32)).collect();
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
            v.into_iter().map(|x| x / n).collect::<Vec<f32>>()
        };
        let (pa, pb) = (prof(&a), prof(&b));
        let d: f32 = pa.iter().zip(&pb).map(|(x, y)| (x - y).abs()).sum();
        eprintln!("profiles {pa:?} {pb:?} diff {d}");
        assert!(d > 0.2, "the two patches sound alike: {d}");
    }

    fn patch(name: &str) -> Vec<f32> {
        let cat = crate::browser::patch_category(name);
        render_preview(None, name, crate::app::patch_program(&cat, name), is_kit(&cat, "") || kit_named(name))
    }

    #[test]
    fn a_mono_bass_patch_still_previews_the_whole_chord() {
        let m = mono(&patch("Big Stadium Bass"));
        let on: Vec<f32> = CHORD.iter().map(|&p| tone(&m, hz(p))).collect();
        let off = tone(&m, hz(62)).max(tone(&m, hz(65)));
        eprintln!("Big Stadium Bass: {on:?} off {off}");
        assert!(on.iter().all(|e| *e > 2.0 * off), "{on:?} vs {off}");
    }

    #[test]
    fn kits_preview_a_bar_of_drums_with_their_own_kit() {
        assert!(kit_named("SoCal") && kit_named("Bluebird") && !kit_named("Deluxe Classic"));
        let a = patch("SoCal");
        let b = patch("Bluebird");
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt();
        assert!(rms(&a) > 1e-3 && rms(&b) > 1e-3);
        // (the kick on beat 1 and the snare on beat 2: loud at both)
        let r = crate::audio::RATE as usize;
        let at = |x: &[f32], sec: f32| rms(&x[(sec * r as f32) as usize * 2..((sec + 0.1) * r as f32) as usize * 2]);
        assert!(at(&a, 0.0) > 1e-3 && at(&a, 0.5) > 1e-3);
        let n = a.len().min(b.len());
        let d: f32 = a[..n].iter().zip(&b[..n]).map(|(x, y)| (x - y).abs()).sum::<f32>() / n as f32;
        assert!(d > 1e-4, "two kits of different character sound the same");
        assert_eq!(kit_notes(2880).iter().filter(|n| n.2 == 36).count(), 2);
    }
}
