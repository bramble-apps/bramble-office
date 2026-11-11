//! Stem Splitter (the audio region's Processing ▸ Stem Splitter…, key commands for its eight
//! presets): the window with Logic's presets and a checkbox per stem (Vocals, Drums, Bass,
//! Guitar, Piano, Other; Individual Kit Pieces: Kick, Snare, Tom, Hi-Hat, Cymbal, Other) and the
//! submix of the unchecked stems; Apply renders the selected audio region, separates it
//! (lg-audio stems: signal processing, no trained model, so the separation is approximate),
//! writes each stem to the project's audio folder and puts them as subtracks of a summing stack
//! below the track, muting the original region. One undo step.

use crate::app::App;
use lg_audio::stems::Stem;

pub const COMMANDS: &[&str] = &[
    "Stem Splitter…",
    "Stem Splitter Preset 1 (Acapella)",
    "Stem Splitter Preset 2 (Instrumental)",
    "Stem Splitter Preset 3 (Vocals + Instrumental)",
    "Stem Splitter Preset 4 (Vocals + Instruments + Drums)",
    "Stem Splitter Preset 5 (Vocals + Instruments + Drums + Bass)",
    "Stem Splitter Preset 6 (Drums)",
    "Stem Splitter Preset 7 (Separate all Stems)",
    "Stem Splitter Preset 8 (Drums, Individual Kit Pieces)",
];

const PRESETS: &[&str] = &["Acapella", "Instrumental", "Vocals + Instrumental", "Vocals + Instruments + Drums", "Vocals + Instruments + Drums + Bass", "Drums", "Separate all Stems", "Drums, Individual Kit Pieces"];

/// What the window shows: the region, the checked stems (main 6), kit pieces, submix.
#[derive(Clone)]
struct State {
    region: (usize, usize),
    checked: [bool; 6],
    kit: bool,
    submix: bool,
    preset: usize,
}

static OPEN: std::sync::Mutex<Option<State>> = std::sync::Mutex::new(None);

/// A preset's checked stems (Vocals, Drums, Bass, Guitar, Piano, Other), kit pieces, submix.
fn preset(k: usize) -> ([bool; 6], bool, bool) {
    match k {
        0 => ([true, false, false, false, false, false], false, false),
        1 => ([false; 6], false, true),
        2 => ([true, false, false, false, false, false], false, true),
        3 => ([true, true, false, false, false, false], false, true),
        4 => ([true, true, true, false, false, false], false, true),
        5 => ([false, true, false, false, false, false], false, false),
        6 => ([true; 6], false, false),
        _ => ([false, true, false, false, false, false], true, false),
    }
}

fn audio_region(app: &App) -> Option<(usize, usize)> {
    let sel = app.region_selection();
    if sel.len() != 1 {
        return None;
    }
    let (t, r) = sel[0];
    app.project.as_ref()?.tracks.get(t)?.regions.get(r)?.audio.as_ref()?;
    Some((t, r))
}

pub fn command(app: &mut App, name: &str) -> bool {
    let Some(i) = COMMANDS.iter().position(|c| *c == name) else { return false };
    let Some(region) = audio_region(app) else {
        app.tell("Stem Splitter works on one selected audio region");
        return true;
    };
    let p = if i == 0 { 3 } else { i - 1 };
    let (checked, kit, submix) = preset(p);
    let st = State { region, checked, kit, submix, preset: p };
    if i == 0 {
        *OPEN.lock().unwrap() = Some(st);
    } else {
        apply(app, &st);
    }
    true
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut st) = OPEN.lock().unwrap().clone() else { return };
    let mut open = true;
    let mut go = false;
    egui::Window::new("Stem Splitter").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        egui::ComboBox::from_label("Preset").selected_text(PRESETS[st.preset]).show_ui(ui, |ui| {
            for (k, n) in PRESETS.iter().enumerate() {
                if ui.selectable_label(st.preset == k, *n).clicked() {
                    st.preset = k;
                    (st.checked, st.kit, st.submix) = preset(k);
                }
            }
        });
        for (k, s) in Stem::MAIN.iter().enumerate() {
            ui.checkbox(&mut st.checked[k], s.name());
        }
        ui.checkbox(&mut st.kit, "Individual Kit Pieces (Kick, Snare, Tom, Hi-Hat, Cymbal, Other)");
        let rest: Vec<&str> = Stem::MAIN.iter().enumerate().filter(|(k, _)| !st.checked[*k]).map(|(_, s)| s.name()).collect();
        ui.add_enabled_ui(rest.len() >= 2, |ui| {
            ui.checkbox(&mut st.submix, format!("Submix: {}", rest.join(" + ")));
        });
        ui.label(egui::RichText::new("Separation by signal processing (no trained model): expect some bleed between stems.").size(11.0).weak());
        if ui.button("Apply").clicked() {
            go = true;
        }
    });
    if go {
        apply(app, &st);
        open = false;
    }
    *OPEN.lock().unwrap() = if open { Some(st) } else { None };
}

fn apply(app: &mut App, st: &State) {
    let (t, r) = st.region;
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.get(r)).cloned() else { return };
    let Some(a) = rg.audio.clone() else { return };
    let x = crate::audioedit::extras(app, t, r);
    let secs = crate::regionops::seconds_pub(app, rg.start, rg.start + rg.length);
    let (mode, flex, notes) = crate::flex::plan_for(app, t, &rg, &x);
    let plan = crate::audioedit::Plan { file: a.file.clone(), offset: x.offset, seconds: secs, gain_db: x.gain_db, fades: Default::default(), flex, reverse: x.reverse, mode, notes };
    let Some(samples) = plan.render() else {
        app.tell("Couldn't read the region's audio");
        return;
    };
    let kit = st.kit && st.checked[1];
    let stems = lg_audio::stems::split(&samples, crate::audio::RATE as f32, kit, &mut |_| {});
    // which outputs: the checked stems (Drums as its kit pieces), and the submix of the rest
    let mut outs: Vec<(String, Vec<f32>)> = Vec::new();
    let mut sub: Option<Vec<f32>> = None;
    let mut sub_names = Vec::new();
    for (s, data) in stems {
        let main = match s {
            Stem::Kick | Stem::Snare | Stem::Tom | Stem::HiHat | Stem::Cymbal | Stem::DrumOther => 1,
            _ => Stem::MAIN.iter().position(|m| *m == s).unwrap_or(5),
        };
        if st.checked[main] {
            outs.push((s.name().to_string(), data));
        } else if st.submix {
            sub_names.push(s.name());
            match &mut sub {
                Some(v) => v.iter_mut().zip(&data).for_each(|(a, b)| *a += b),
                None => sub = Some(data),
            }
        }
    }
    if let Some(v) = sub {
        outs.push((sub_names.join(" + "), v));
    }
    if outs.is_empty() {
        app.tell("No stems chosen");
        return;
    }
    let dir = match (&app.file, app.project.as_ref().and_then(|p| p.audio_dir.clone())) {
        (_, Some(d)) => d,
        (Some((alt, _)), None) => lg_core::project::package_of(alt).join("Media/Audio Files"),
        _ => std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join(".local/share/bramble-studio/Stems"),
    };
    let _ = std::fs::create_dir_all(&dir);
    app.checkpoint("Stem Splitter");
    let depth = app.project.as_ref().map_or(0, |p| p.tracks[t].extra.depth);
    let mut new_tracks = Vec::new();
    for (n, data) in &outs {
        let base = format!("{} {}", rg.name.trim(), n);
        let mut path = dir.join(format!("{base}.wav"));
        let mut k = 1;
        while path.exists() {
            path = dir.join(format!("{base} {k}.wav"));
            k += 1;
        }
        if std::fs::write(&path, crate::audio::wav_bytes(data)).is_err() {
            app.tell("Couldn't write a stem");
            return;
        }
        let mut nt = lg_core::Track::new(n, lg_core::TrackKind::Audio);
        nt.extra.depth = depth;
        let mut reg = rg.clone();
        reg.name = base;
        reg.audio = Some(lg_core::project::AudioRef { file: path, seconds: data.len() as f64 / 2.0 / crate::audio::RATE as f64 });
        reg.muted = false;
        nt.regions.push(reg);
        new_tracks.push(nt);
    }
    let Some(p) = app.project.as_mut() else { return };
    p.tracks[t].regions[r].muted = true;
    let at = lg_core::arrange::group(&p.tracks, t).end;
    let n = new_tracks.len();
    for (i, nt) in new_tracks.into_iter().enumerate() {
        p.tracks.insert(at + i, nt);
        app.programs.insert((at + i).min(app.programs.len()), 0);
    }
    crate::trackarea::create_stack(app, lg_core::arrange::StackKind::Summing, (at..at + n).collect());
    app.after_edit();
    app.tell(format!("Stem Splitter: {n} stems"));
}
