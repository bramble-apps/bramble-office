//! The File menu's project side: Save (into the project's own .logicx or GarageBand .band),
//! Save As… / Save A Copy As…, Save as Template…, New from Template… (the template chooser),
//! Revert to ▸ (Saved, and Logic's Project File Backups made on every save), Project
//! Alternatives ▸ (New Alternative…, Edit Alternatives…: rename, open, delete; Export
//! Alternative as Project…; one item per alternative), Project Management ▸ (Clean Up…,
//! Consolidate…, Rename…, Show in Finder…) and the Project Settings window.
//!
//! Before writing, the app's own state that a project saves goes into the project (markers,
//! arrangement, auxes, groups, Channel EQ, audio regions' fades and file windows); lg-core's
//! save.rs and writeback.rs write it and say what they leave out, and Save says so.

use std::path::{Path, PathBuf};

use lg_core::package;
use lg_core::TrackKind;

use crate::app::App;

pub const COMMANDS: &[&str] = &[
    "Save",
    "Save As…",
    "Save A Copy As…",
    "Save Project as…",
    "Save as Template…",
    "New from Template…",
    "Revert to Saved",
    "Revert to Saved…",
    "Revert to",
    "New Alternative…",
    "New Project Alternative…",
    "Edit Alternatives…",
    "Edit Project Alternatives",
    "Export Alternative as Project…",
    "Project Alternatives",
    "Project Management",
    "Clean Up…",
    "Clean Up Project…",
    "Consolidate…",
    "Consolidate Project…",
    "Save and Clean Up Project",
    "Rename…",
    "Show in Finder…",
    "Project Settings",
    "Project Settings…",
    "Open Synchronization Project Settings…",
    "Open Metronome Project Settings…",
    "Open Recording Project Settings…",
    "Open Tuning Project Settings…",
    "Open Audio Project Settings…",
    "Open MIDI Project Settings…",
    "Open Score Project Settings…",
    "Open Movie Project Settings…",
    "Open Assets Project Settings…",
];



/// Something asked in a small window before it's done.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// A name for: a new alternative, a renamed alternative (its key), the renamed project.
    NewAlternative(String),
    RenameAlternative(u32, String),
    RenameProject(String),
    /// Delete an alternative (key, name).
    DeleteAlternative(u32, String),
    /// Clean Up: the unused files found.
    CleanUp(Vec<PathBuf>),
    /// Save first? before switching to an alternative (key) or reverting to a file.
    Switch(u32),
}

#[derive(Default)]
pub struct State {
    /// The template chooser is open; its selected entry.
    pub chooser: bool,
    pub pick: usize,
    pub ask: Option<Ask>,
    pub edit_alternatives: bool,
    /// The Project Settings window's pane (None: closed).
    pub settings: Option<String>,
    /// Assets ▸ Copy audio files into project (Save consolidates first).
    pub copy_assets: bool,
    /// A GarageBand .band open: its package (Save writes its projectData).
    pub band: Option<PathBuf>,
    /// The app's mixer as opened (sends, outputs): Save tells when they changed.
    opened_sends: String,
    opened_outputs: String,
    /// What the last save couldn't write.
    pub last_left_out: Vec<String>,
}

// --- open ----------------------------------------------------------------------------------------

/// After a project opens (App::open_path): the alternative it opened with, GarageBand's
/// package, and what the app keeps beside the project taken from it.
pub fn after_open(app: &mut App, path: &Path) {
    app.pf.band = None;
    app.pf.copy_assets = true;
    let is_band = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("band")) || path.join("projectData").is_file();
    if is_band {
        let data = lg_core::bandsave::model_bytes(path);
        app.file = data.map(|d| (path.to_path_buf(), d));
        app.pf.band = Some(path.to_path_buf());
    } else {
        let pkg = lg_core::project::package_of(path);
        if let Some(a) = package::active(&pkg) {
            app.file = std::fs::read(a.dir.join("ProjectData")).ok().map(|d| (a.dir, d));
        }
    }
    take_from_project(app);
    crate::hosting::after_open(app);
    crate::projsettings::after_open(app);
    crate::arranging::after_open(app);
    crate::envhub::after_open(app);
}

/// The app's side of what the project holds: markers, arrangement, audio regions' extras.
fn take_from_project(app: &mut App) {
    let Some(p) = app.project.as_ref() else { return };
    app.markers = p.markers.clone();
    app.arrangement = p.arrangement.clone();
    let mut extras = Vec::new();
    for (t, tr) in p.tracks.iter().enumerate() {
        for (r, rg) in tr.regions.iter().enumerate() {
            let Some(a) = &rg.audio else { continue };
            let pr = rg.props;
            if pr.fade_in != (0, 0) || pr.fade_out != (0, 0) || pr.audio_offset != 0 {
                let fades = lg_audio::fade::RegionFades { in_ms: pr.fade_in.0, in_curve: pr.fade_in.1, out_ms: pr.fade_out.0, out_curve: pr.fade_out.1, ..Default::default() };
                extras.push(((t, r), crate::audioedit::RegionAudio { file: a.file.clone(), offset: pr.audio_offset as usize, fades, ..Default::default() }));
            }
        }
    }
    for (k, v) in extras {
        app.audio_edit.regions.insert(k, v);
    }
    app.pf.opened_sends = format!("{:?}", sorted(&app.sends));
    app.pf.opened_outputs = format!("{:?}", sorted(&app.mix.outputs));
}

fn sorted<V: std::fmt::Debug>(m: &std::collections::HashMap<usize, V>) -> Vec<(usize, String)> {
    let mut v: Vec<(usize, String)> = m.iter().map(|(k, x)| (*k, format!("{x:?}"))).collect();
    v.sort();
    v
}

// --- save ----------------------------------------------------------------------------------------

/// The app's state a project saves, into the project; returns what the app holds that Save
/// can't write.
pub fn give_to_project(app: &mut App) -> Vec<String> {
    let mut left = Vec::new();
    if format!("{:?}", sorted(&app.sends)) != app.pf.opened_sends {
        left.push("sends".to_string());
    }
    if format!("{:?}", sorted(&app.mix.outputs)) != app.pf.opened_outputs {
        left.push("track outputs".to_string());
    }
    if !app.mix.vcas.is_empty() {
        left.push("VCAs".to_string());
    }
    let markers = app.markers.clone();
    let arrangement = app.arrangement.clone();
    let auxes = app.auxes.clone();
    let groups = app.mix.groups.clone();
    let group_of = app.mix.group_of.clone();
    let eqs = app.eqs.clone();
    let extras = app.audio_edit.regions.clone();
    let Some(p) = app.project.as_mut() else { return left };
    p.markers = markers;
    p.arrangement = arrangement;
    // auxes: the ones read from the file, in their order
    let read = p.mixer.auxes();
    if auxes.len() > read.len() {
        left.push(format!("{} aux(es) made here", auxes.len() - read.len()));
    }
    for (k, s) in read.into_iter().enumerate() {
        let Some(a) = auxes.get(k) else { continue };
        let st = &mut p.mixer.strips[s];
        st.volume_db = a.volume_db;
        st.pan = a.pan;
        st.mute = a.mute;
        st.solo = a.solo;
        let mut list: Vec<lg_core::plugin::Plugin> = st.plugins.iter().filter(|u| u.slot < 3 || u.name.is_empty()).cloned().collect();
        list.extend(a.inserts.iter().cloned());
        st.plugins = list;
    }
    // groups: membership by track, settings by group
    p.mixer.track_group = (0..p.tracks.len()).map(|t| group_of.get(&t).map_or(0, |g| *g as u8 + 1)).collect();
    for (k, g) in groups.iter().enumerate() {
        match p.mixer.groups.get_mut(k) {
            Some(x) => {
                x.mask = g.mask;
                x.name = g.name.clone();
            }
            None => p.mixer.groups.push(lg_core::mixer::Group { number: k as u8 + 1, name: g.name.clone(), mask: g.mask }),
        }
    }
    // Channel EQ: Logic's block is 8 × [on, frequency, gain, Q] (eq.rs reads it so)
    for (t, eq) in eqs {
        let Some(u) = p.tracks.get_mut(t).and_then(|tr| tr.plugins.iter_mut().find(|u| u.name == "Channel EQ")) else { continue };
        if u.params.len() < 32 || crate::eq::Params::from_logic(&u.params) == Some(eq) {
            continue;
        }
        for (i, b) in eq.bands.iter().enumerate() {
            u.params[4 * i..4 * i + 4].copy_from_slice(&[if b.on { 1.0 } else { 0.0 }, b.freq, b.gain, b.q]);
        }
    }
    // audio regions: fades and where they start in their files
    for ((t, r), x) in extras {
        let Some(rg) = p.tracks.get_mut(t).and_then(|tr| tr.regions.get_mut(r)) else { continue };
        if rg.audio.as_ref().is_none_or(|a| a.file != x.file) {
            continue;
        }
        rg.props.fade_in = (x.fades.in_ms, x.fades.in_curve);
        rg.props.fade_out = (x.fades.out_ms, x.fades.out_curve);
        rg.props.audio_offset = x.offset as u32;
        if x.gain_db.abs() > 0.01 {
            left.push(format!("gain of region “{}”", rg.name));
        }
        if x.fades.out_type != Default::default() {
            left.push(format!("fade type of region “{}”", rg.name));
        }
        if !x.flex.is_empty() || x.reverse {
            left.push(format!("flex / reverse of region “{}”", rg.name));
        }
    }
    left
}

/// File ▸ Save (⌘S): the edits written into the project's own file; the alternative's
/// ProjectData goes into its Project File Backups first.
pub fn save(app: &mut App) {
    crate::projsettings::save(app);
    crate::envhub::save(app);
    if app.project.is_none() || app.file.is_none() || app.demo {
        app.tell(if app.demo { "The demo song can't be saved: open a Logic project (⌘O)" } else { "A project made here can't be saved as a Logic file yet: start from one of My Templates (Save as Template…) or open a .logicx" });
        return;
    }
    // (checks: LG_SAVE=1 lets a scripted run save its scratch copy)
    if app.automated && std::env::var_os("LG_SAVE").is_none() {
        return;
    }
    if app.pf.copy_assets && app.pf.band.is_none() {
        consolidate(app, true);
    }
    let mut left = give_to_project(app);
    if let Some(pkg) = app.pf.band.clone() {
        let p = app.project.clone().unwrap_or_default();
        match lg_core::bandsave::save(&p, &pkg) {
            Ok(s) => {
                left.extend(s.unsaved.iter().map(|n| format!("region “{n}”")));
                left.extend(s.unsaved_automation.clone());
                left.extend(s.left_out.clone());
                app.file = Some((pkg, s.data));
                app.dirty = false;
                report(app, left);
            }
            Err(e) => app.tell(format!("Couldn't save: {e}")),
        }
        return;
    }
    let (Some(p), Some((alt, original))) = (app.project.as_ref(), app.file.as_ref()) else { return };
    let alt = alt.clone();
    match lg_core::save::save(p, original) {
        Ok(mut saved) => {
            // the Live Loops grid's cells written into the same bytes
            let (data, unsaved_cells) = crate::liveloops::save_into(app, &saved.data);
            saved.data = data;
            saved.unsaved.extend(unsaved_cells);
            let target = alt.join("ProjectData");
            let first = alt.join("ProjectData.before-bramble");
            if !first.exists() {
                let _ = std::fs::copy(&target, &first);
            }
            if saved.data != *original || !target.exists() {
                // (Settings ▸ Project Handling ▸ Auto Backup)
                if let Some(keep) = crate::prefs::backups(&app.logic_settings) {
                    let _ = package::make_backup(&alt, keep);
                }
            }
            match std::fs::write(&target, &saved.data) {
                Ok(()) => {
                    if let Some(p) = app.project.as_ref() {
                        lg_core::save::update_metadata(&alt, p);
                    }
                    left.extend(saved.unsaved.iter().map(|n| format!("region “{n}”")));
                    left.extend(saved.unsaved_automation.clone());
                    left.extend(saved.left_out.clone());
                    app.file = Some((alt, saved.data));
                    app.dirty = false;
                    report(app, left);
                }
                Err(e) => app.tell(format!("Couldn't save: {e}")),
            }
        }
        Err(e) => app.tell(format!("Couldn't save: {e}")),
    }
}

/// "Saved", or what the Logic file doesn't have yet.
fn report(app: &mut App, mut left: Vec<String>) {
    // (hosted CLAP / VST3 / LV2 plug-ins are kept in Studio's own settings)
    crate::hosting::saved_elsewhere(&mut left);
    crate::hosting::save_sidecar(app);
    crate::arranging::saved_elsewhere(&mut left);
    crate::arranging::save_sidecar(app);
    left.dedup();
    app.pf.last_left_out = left.clone();
    if left.is_empty() {
        app.tell("Saved");
        return;
    }
    let shown: Vec<String> = left.iter().take(4).cloned().collect();
    let more = if left.len() > 4 { format!(" and {} more (File ▸ Project Settings ▸ General)", left.len() - 4) } else { String::new() };
    app.tell(format!("Saved — not in the Logic file yet: {}{more}", shown.join(", ")));
}

/// The package the open project lives in (None: GarageBand, the demo, a new project).
fn package(app: &App) -> Option<PathBuf> {
    let (alt, _) = app.file.as_ref()?;
    if app.pf.band.is_some() {
        return None;
    }
    alt.parent().and_then(|a| a.parent()).map(Path::to_path_buf)
}

/// Save As… (the project moves to the copy) / Save A Copy As… (the copy is saved, the project
/// stays where it is): the whole package copied, then saved there.
pub fn save_as(app: &mut App, copy: bool) {
    let Some(pkg) = package(app) else {
        save(app);
        return;
    };
    if app.automated && std::env::var_os("LG_SAVE_AS").is_none() {
        return;
    }
    let name = app.project.as_ref().map(|p| p.name.clone()).unwrap_or_default();
    let dest = match std::env::var_os("LG_SAVE_AS") {
        Some(d) => PathBuf::from(d),
        None => {
            let Some(d) = kn_ui::files::FileDialog::new().set_title(if copy { "Save A Copy As" } else { "Save As" }).set_file_name(format!("{name}.logicx")).save_file() else { return };
            d
        }
    };
    let dest = if dest.extension().is_some_and(|e| e == "logicx") { dest } else { dest.with_extension("logicx") };
    save_package_to(app, &pkg, &dest, copy);
}

fn save_package_to(app: &mut App, pkg: &Path, dest: &Path, copy: bool) {
    if dest.exists() {
        app.tell(format!("{} exists: pick another name", dest.display()));
        return;
    }
    if let Err(e) = package::copy_dir(pkg, dest) {
        app.tell(format!("Couldn't save: {e}"));
        return;
    }
    let Some((alt, data)) = app.file.clone() else { return };
    let rel = alt.strip_prefix(pkg).map(Path::to_path_buf).unwrap_or_default();
    let (keep_file, keep_project) = (app.file.clone(), app.project.as_ref().map(|p| (p.name.clone(), p.path.clone())));
    app.file = Some((dest.join(&rel), data));
    if let Some(pr) = app.project.as_mut() {
        pr.name = dest.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        pr.path = dest.to_path_buf();
    }
    // (the grid stays as it is, not re-read from the new path)
    crate::liveloops::keep_after_save_as(app);
    let dirty = app.dirty;
    save(app);
    if copy {
        // the project stays where it was, as edited as it was
        app.file = keep_file;
        if let (Some(pr), Some((n, p))) = (app.project.as_mut(), keep_project) {
            pr.name = n;
            pr.path = p;
        }
        app.dirty = dirty;
        app.tell(format!("Saved a copy as {}", dest.file_name().unwrap_or_default().to_string_lossy()));
    }
}

// --- templates -------------------------------------------------------------------------------

/// Studio's own templates folder (Save as Template… puts projects there).
fn templates_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
    base.join("bramble-studio/Project Templates")
}

/// The user's own templates: Studio's folder and Logic's (when its templates were copied over).
fn my_templates() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let mut out = Vec::new();
    for d in [templates_dir(), home.join("Music/Audio Music Apps/Project Templates")] {
        if let Ok(rd) = std::fs::read_dir(&d) {
            out.extend(rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "logicx")));
        }
    }
    out.sort();
    out
}

/// Studio's starting layouts (track kinds and names), after the kinds of layout Logic's New
/// Project templates set up.
pub const LAYOUTS: &[(&str, &[(&str, TrackKind)])] = &[
    ("Empty Project", &[("Deluxe Classic", TrackKind::Instrument)]),
    ("Songwriter", &[("SoCal", TrackKind::Drummer), ("Fingerstyle Bass", TrackKind::Instrument), ("Steinway Grand Piano", TrackKind::Instrument), ("Acoustic Guitar", TrackKind::Audio), ("Vocal", TrackKind::Audio)]),
    ("Electronic", &[("Drum Machine", TrackKind::Drummer), ("Synth Bass", TrackKind::Instrument), ("Lead Synth", TrackKind::Instrument), ("Pad", TrackKind::Instrument)]),
    ("Hip Hop", &[("Beat", TrackKind::Drummer), ("808 Bass", TrackKind::Instrument), ("Keys", TrackKind::Instrument), ("Vocal", TrackKind::Audio)]),
    ("Orchestral", &[("Strings", TrackKind::Instrument), ("Brass", TrackKind::Instrument), ("Woodwinds", TrackKind::Instrument), ("Timpani", TrackKind::Instrument), ("Harp", TrackKind::Instrument)]),
    ("Voice Memo", &[("Audio 1", TrackKind::Audio)]),
];

/// A new project laid out as `LAYOUTS[k]`.
pub fn layout_project(k: usize) -> lg_core::Project {
    let (name, tracks) = LAYOUTS.get(k).copied().unwrap_or(LAYOUTS[0]);
    let mut p = crate::demo::empty();
    p.name = if k == 0 { "Untitled".into() } else { format!("Untitled {name}") };
    p.tracks = tracks
        .iter()
        .enumerate()
        .map(|(i, (n, kind))| {
            let mut t = lg_core::Track::new(n, *kind);
            t.row = i as u32 + 1;
            t
        })
        .collect();
    p
}

/// New (⇧⌘N) / New from Template… (⌘N).
pub fn new_project(app: &mut App, cmd: &str) {
    app.stop();
    if cmd == "New" {
        app.set_project(crate::demo::empty(), true);
        app.file = None;
        app.pf.band = None;
    } else {
        app.pf.chooser = true;
    }
}

/// Opens a copy of a template package as a new, untitled project (so the template stays as
/// it is): in ~/Music/Bramble Studio.
fn from_template(app: &mut App, tpl: &Path) {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let dir = std::env::var_os("LG_PROJECTS").map(PathBuf::from).unwrap_or_else(|| home.join("Music/Bramble Studio"));
    let _ = std::fs::create_dir_all(&dir);
    let dest = (1..1000).map(|n| dir.join(if n == 1 { "Untitled.logicx".to_string() } else { format!("Untitled {n}.logicx") })).find(|d| !d.exists()).unwrap_or_else(|| dir.join("Untitled.logicx"));
    if let Err(e) = package::copy_dir(tpl, &dest) {
        app.tell(format!("Couldn't open the template: {e}"));
        return;
    }
    let _ = package::rename(&dest, 0, &dest.file_stem().unwrap_or_default().to_string_lossy());
    app.open_path(&dest);
}

/// Save as Template…: a copy of the project in Studio's templates folder.
fn save_as_template(app: &mut App) {
    let Some(pkg) = package(app) else {
        app.tell("Only a Logic project (.logicx) can be saved as a template");
        return;
    };
    let name = app.project.as_ref().map(|p| p.name.clone()).unwrap_or_default();
    let dest = templates_dir().join(format!("{name}.logicx"));
    let _ = std::fs::create_dir_all(templates_dir());
    if dest.exists() {
        let _ = std::fs::remove_dir_all(&dest);
    }
    save_package_to(app, &pkg, &dest, true);
    app.tell(format!("Saved as template “{name}”"));
}

// --- revert ----------------------------------------------------------------------------------

/// Reopens the alternative from `data` (the saved file, or a backup); the project's files stay
/// as they are until the next Save.
fn revert_to(app: &mut App, data: Vec<u8>, what: &str) {
    let Some((alt, _)) = app.file.clone() else { return };
    let Some(path) = app.project.as_ref().map(|p| p.path.clone()) else { return };
    if app.pf.band.is_some() {
        app.open_path(&path);
        return;
    }
    match lg_core::project::open_alternative(&path, &alt, &data) {
        Ok(p) => {
            app.stop();
            app.set_project(p, false);
            // (Save writes onto the file as it is on disk)
            app.file = std::fs::read(alt.join("ProjectData")).ok().map(|d| (alt, d));
            take_from_project(app);
            app.dirty = what != "Saved";
            app.tell(format!("Reverted to {what}"));
        }
        Err(e) => app.tell(format!("Couldn't revert: {e}")),
    }
}

fn when(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    // (local time without a time zone database: UTC offset from the TZ environment isn't read)
    let days = secs.div_euclid(86400);
    let s = secs.rem_euclid(86400);
    let (y, m, d) = civil(days);
    format!("{m}/{d}/{:02}, {}:{:02}:{:02}", y % 100, s / 3600, (s / 60) % 60, s % 60)
}

/// Days since 1970-01-01 as (year, month, day).
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

// --- alternatives ----------------------------------------------------------------------------

fn open_key(app: &App) -> Option<u32> {
    app.file.as_ref().and_then(|(alt, _)| alt.file_name()?.to_str()?.parse().ok())
}

fn switch_alternative(app: &mut App, key: u32) {
    let Some(pkg) = package(app) else { return };
    let Some(a) = package::list(&pkg).into_iter().find(|a| a.key == key) else { return };
    let _ = package::set_active(&pkg, key);
    let path = app.project.as_ref().map(|p| p.path.clone()).unwrap_or(pkg);
    app.stop();
    app.open_path(&path);
    app.tell(format!("Alternative “{}”", a.name));
}

fn new_alternative(app: &mut App, name: &str) {
    let Some(pkg) = package(app) else { return };
    let Some((alt, _)) = app.file.clone() else { return };
    match package::new_alternative(&pkg, &alt, name) {
        Ok(a) => {
            // the edits so far are saved into the new alternative; the old one stays as saved
            if let Some((_, d)) = app.file.as_ref().map(|(_, d)| ((), d.clone())) {
                app.file = Some((a.dir.clone(), d));
            }
            save(app);
            app.tell(format!("Saved as new alternative “{name}”"));
        }
        Err(e) => app.tell(format!("Couldn't make the alternative: {e}")),
    }
}

// --- assets ----------------------------------------------------------------------------------

/// Consolidate…: audio files the regions use from outside the project are copied into its
/// audio folder (and the regions use the copies).
fn consolidate(app: &mut App, quiet: bool) {
    let Some(path) = app.project.as_ref().map(|p| p.path.clone()) else { return };
    if package(app).is_none() {
        return;
    }
    let dir = package::audio_dir(&path);
    let mut copied = 0;
    let mut failed = Vec::new();
    let Some(p) = app.project.as_mut() else { return };
    for r in p.tracks.iter_mut().flat_map(|t| t.regions.iter_mut()) {
        let Some(a) = r.audio.as_mut() else { continue };
        if a.file.starts_with(&dir) || !a.file.is_file() {
            continue;
        }
        let name = a.file.file_name().unwrap_or_default().to_os_string();
        let to = dir.join(&name);
        if !to.exists() {
            let _ = std::fs::create_dir_all(&dir);
            if let Err(e) = std::fs::copy(&a.file, &to) {
                failed.push(format!("{}: {e}", name.to_string_lossy()));
                continue;
            }
            copied += 1;
        }
        a.file = to;
    }
    if !quiet || !failed.is_empty() {
        let msg = if failed.is_empty() { format!("Consolidated: {copied} file(s) copied into the project") } else { format!("Couldn't copy {}", failed.join(", ")) };
        app.tell(msg);
    }
}

/// Clean Up…: the audio files no alternative and no region uses.
fn clean_up_candidates(app: &App) -> Vec<PathBuf> {
    let Some(p) = app.project.as_ref() else { return Vec::new() };
    let using: Vec<String> = p.tracks.iter().flat_map(|t| t.regions.iter()).filter_map(|r| r.audio.as_ref()?.file.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
    package::unused_audio(&p.path, &using)
}

/// Moves files to the desktop's Trash (freedesktop: files + a .trashinfo), so Clean Up can be
/// undone by hand.
fn to_trash(files: &[PathBuf]) -> usize {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let trash = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")).join("Trash");
    let _ = std::fs::create_dir_all(trash.join("files"));
    let _ = std::fs::create_dir_all(trash.join("info"));
    let mut n = 0;
    for f in files {
        let name = f.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let dest = (0..1000).map(|k| if k == 0 { name.clone() } else { format!("{name}.{k}") }).find(|d| !trash.join("files").join(d).exists()).unwrap_or(name);
        let info = format!("[Trash Info]\nPath={}\nDeletionDate={}\n", f.display(), when(std::time::SystemTime::now()));
        if std::fs::rename(f, trash.join("files").join(&dest)).is_ok() {
            let _ = std::fs::write(trash.join("info").join(format!("{dest}.trashinfo")), info);
            n += 1;
        }
    }
    n
}

// --- commands --------------------------------------------------------------------------------

/// The commands above (App::key_command asks here first).
pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Save" => save(app),
        "Save As…" | "Save Project as…" => save_as(app, false),
        "Save A Copy As…" => save_as(app, true),
        "Save as Template…" => save_as_template(app),
        "New from Template…" => new_project(app, name),
        "Revert to Saved" | "Revert to Saved…" | "Revert to" => {
            if let Some((alt, _)) = app.file.clone() {
                match std::fs::read(alt.join("ProjectData")) {
                    Ok(d) => revert_to(app, d, "Saved"),
                    Err(e) => app.tell(format!("Couldn't revert: {e}")),
                }
            }
        }
        "New Alternative…" | "New Project Alternative…" => {
            if package(app).is_some() {
                let n = app.project.as_ref().map(|p| p.name.clone()).unwrap_or_default();
                app.pf.ask = Some(Ask::NewAlternative(format!("{n} #1")));
            }
        }
        "Edit Alternatives…" | "Edit Project Alternatives" | "Project Alternatives" => app.pf.edit_alternatives = package(app).is_some(),
        "Export Alternative as Project…" => {
            let (Some(pkg), Some(key)) = (package(app), open_key(app)) else { return true };
            if app.automated && std::env::var_os("LG_SAVE_AS").is_none() {
                return true;
            }
            let name = package::list(&pkg).into_iter().find(|a| a.key == key).map(|a| a.name).unwrap_or_default();
            let dest = std::env::var_os("LG_SAVE_AS").map(PathBuf::from).or_else(|| kn_ui::files::FileDialog::new().set_title("Export Alternative as Project").set_file_name(format!("{name}.logicx")).save_file());
            if let Some(d) = dest {
                let d = if d.extension().is_some_and(|e| e == "logicx") { d } else { d.with_extension("logicx") };
                match package::export(&pkg, key, &d) {
                    Ok(()) => app.tell(format!("Exported “{name}” as {}", d.file_name().unwrap_or_default().to_string_lossy())),
                    Err(e) => app.tell(format!("Couldn't export: {e}")),
                }
            }
        }
        "Clean Up…" | "Clean Up Project…" => {
            let c = clean_up_candidates(app);
            if c.is_empty() {
                app.tell("Clean Up: no unused audio files");
            } else {
                app.pf.ask = Some(Ask::CleanUp(c));
            }
        }
        "Save and Clean Up Project" => {
            save(app);
            let c = clean_up_candidates(app);
            if !c.is_empty() {
                app.pf.ask = Some(Ask::CleanUp(c));
            }
        }
        "Consolidate…" | "Consolidate Project…" => {
            consolidate(app, false);
            app.dirty = true;
        }
        "Rename…" => {
            if package(app).is_some() {
                let n = app.project.as_ref().map(|p| p.name.clone()).unwrap_or_default();
                app.pf.ask = Some(Ask::RenameProject(n));
            }
        }
        "Show in Finder…" => {
            if let Some(pkg) = package(app).or_else(|| app.pf.band.clone()) {
                if !app.automated {
                    let _ = std::process::Command::new("xdg-open").arg(pkg.parent().unwrap_or(&pkg)).spawn();
                }
            }
        }
        "Project Settings" | "Project Settings…" => app.pf.settings = Some("General".into()),
        n if n.starts_with("Open ") && n.ends_with(" Project Settings…") => {
            app.pf.settings = Some(n.trim_start_matches("Open ").trim_end_matches(" Project Settings…").to_string());
        }
        _ => return false,
    }
    true
}

/// The submenus that list what's on disk: Revert to ▸ (Saved, the backups), Project
/// Alternatives ▸ (one item per alternative), Project Settings ▸ (its panes). True: drawn here.
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    match it.label.as_str() {
        "Revert to" => {
            let alt = app.file.as_ref().map(|f| f.0.clone());
            ui.add_enabled_ui(alt.is_some(), |ui| {
                ui.menu_button("Revert to", |ui| {
                    ui.set_min_width(230.0);
                    let Some(alt) = alt else { return };
                    let saved = std::fs::metadata(alt.join("ProjectData")).and_then(|m| m.modified()).ok();
                    if ui.button(format!("Saved\t\t{}", saved.map(when).unwrap_or_default())).clicked() {
                        command(app, "Revert to Saved");
                        ui.close();
                    }
                    let b = package::backups(&alt);
                    if !b.is_empty() {
                        ui.separator();
                    }
                    for (slot, t) in b {
                        if ui.button(format!("Backup\t\t{}", when(t))).clicked() {
                            if let Ok(d) = std::fs::read(slot.join("ProjectData")) {
                                revert_to(app, d, &format!("the backup of {}", when(t)));
                            }
                            ui.close();
                        }
                    }
                });
            });
            true
        }
        "Project Alternatives" => {
            let pkg = package(app);
            ui.add_enabled_ui(pkg.is_some(), |ui| {
                ui.menu_button("Project Alternatives", |ui| {
                    ui.set_min_width(230.0);
                    for c in it.children.iter().filter(|c| !c.separator) {
                        if ui.button(&c.label).clicked() {
                            command(app, &c.label);
                            ui.close();
                        }
                    }
                    let Some(pkg) = pkg else { return };
                    ui.separator();
                    let open = open_key(app);
                    for a in package::list(&pkg) {
                        let on = Some(a.key) == open;
                        if ui.button(format!("{} {}", if on { "✓" } else { "  " }, a.name)).clicked() && !on {
                            if app.dirty {
                                app.pf.ask = Some(Ask::Switch(a.key));
                            } else {
                                switch_alternative(app, a.key);
                            }
                            ui.close();
                        }
                    }
                });
            });
            true
        }
        "Project Settings" => {
            ui.menu_button("Project Settings", |ui| {
                ui.set_min_width(200.0);
                for c in &it.children {
                    if c.separator {
                        ui.separator();
                        continue;
                    }
                    let pane = c.label.trim_end_matches('…').to_string();
                    if ui.button(&c.label).clicked() {
                        if crate::projsettings::PANES.contains(&pane.as_str()) {
                            app.pf.settings = Some(pane);
                        } else {
                            crate::projsettings::command(app, &c.label);
                        }
                        ui.close();
                    }
                }
            });
            true
        }
        _ => false,
    }
}

// --- windows ---------------------------------------------------------------------------------

/// The chooser, the asks, Edit Alternatives, Project Settings (App's frame calls this).
pub fn windows(app: &mut App, ctx: &egui::Context) {
    chooser(app, ctx);
    ask(app, ctx);
    edit_alternatives(app, ctx);
    project_settings(app, ctx);
}

fn chooser(app: &mut App, ctx: &egui::Context) {
    if !app.pf.chooser {
        return;
    }
    let mut open = true;
    let mine = my_templates();
    let mut chosen: Option<usize> = None;
    egui::Window::new("Choose a Project Template").open(&mut open).collapsible(false).resizable(false).default_width(520.0).show(ctx, |ui| {
        ui.label(egui::RichText::new("New Project").font(crate::look::bold(13.0)));
        let total = LAYOUTS.len() + mine.len();
        ui.horizontal_wrapped(|ui| {
            for (k, (name, tracks)) in LAYOUTS.iter().enumerate() {
                let r = ui.add(egui::Button::selectable(app.pf.pick == k, format!("{name}\n{} track{}", tracks.len(), if tracks.len() == 1 { "" } else { "s" })).min_size(egui::vec2(110.0, 54.0)));
                if r.clicked() {
                    app.pf.pick = k;
                }
                if r.double_clicked() {
                    chosen = Some(k);
                }
            }
        });
        ui.add_space(8.0);
        ui.label(egui::RichText::new("My Templates").font(crate::look::bold(13.0)));
        if mine.is_empty() {
            ui.label(egui::RichText::new("None yet: File ▸ Save as Template… keeps a Logic project here.").color(crate::look::text_dim()));
        }
        ui.horizontal_wrapped(|ui| {
            for (j, m) in mine.iter().enumerate() {
                let k = LAYOUTS.len() + j;
                let r = ui.add(egui::Button::selectable(app.pf.pick == k, m.file_stem().unwrap_or_default().to_string_lossy()).min_size(egui::vec2(110.0, 54.0)));
                if r.clicked() {
                    app.pf.pick = k;
                }
                if r.double_clicked() {
                    chosen = Some(k);
                }
            }
        });
        app.pf.pick = app.pf.pick.min(total.saturating_sub(1));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Open an existing project…").clicked() {
                app.pf.chooser = false;
                app.open_dialog();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Choose").clicked() {
                    chosen = Some(app.pf.pick);
                }
                if ui.button("Cancel").clicked() {
                    app.pf.chooser = false;
                }
            });
        });
    });
    if let Some(k) = chosen {
        app.pf.chooser = false;
        if k < LAYOUTS.len() {
            app.stop();
            app.set_project(layout_project(k), false);
            app.file = None;
            app.pf.band = None;
            app.dirty = false;
        } else if let Some(t) = mine.get(k - LAYOUTS.len()).cloned() {
            from_template(app, &t);
        }
    }
    if !open {
        app.pf.chooser = false;
    }
}

fn ask(app: &mut App, ctx: &egui::Context) {
    let Some(mut q) = app.pf.ask.clone() else { return };
    let mut done: Option<bool> = None;
    let title = match &q {
        Ask::NewAlternative(_) => "Save as new Alternative",
        Ask::RenameAlternative(..) => "Rename Alternative",
        Ask::RenameProject(_) => "Rename Project",
        Ask::DeleteAlternative(..) => "Delete Alternative",
        Ask::CleanUp(_) => "Clean Up Project",
        Ask::Switch(_) => "Save changes?",
    };
    egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
        match &mut q {
            Ask::NewAlternative(n) | Ask::RenameAlternative(_, n) | Ask::RenameProject(n) => {
                let r = ui.text_edit_singleline(n);
                r.request_focus();
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
            }
            Ask::DeleteAlternative(_, n) => {
                ui.label(format!("Delete the alternative “{n}”? Its folder is removed from the project; this can't be undone."));
            }
            Ask::CleanUp(files) => {
                ui.label(format!("{} audio file(s) in the project aren't used by any alternative. Move them to the Trash?", files.len()));
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    for f in files.iter() {
                        ui.label(f.file_name().unwrap_or_default().to_string_lossy());
                    }
                });
            }
            Ask::Switch(_) => {
                ui.label("Save the changes to this alternative before opening the other one?");
            }
        }
        ui.horizontal(|ui| {
            let ok = match &q {
                Ask::DeleteAlternative(..) => "Delete",
                Ask::CleanUp(_) => "Move to Trash",
                Ask::Switch(_) => "Save",
                Ask::NewAlternative(_) => "Save",
                _ => "OK",
            };
            if ui.button(ok).clicked() {
                done = Some(true);
            }
            if matches!(q, Ask::Switch(_)) && ui.button("Don't Save").clicked() {
                done = Some(false);
            }
            if ui.button("Cancel").clicked() {
                app.pf.ask = None;
            }
        });
    });
    if app.pf.ask.is_none() {
        return;
    }
    app.pf.ask = Some(q.clone());
    let Some(yes) = done else { return };
    app.pf.ask = None;
    let pkg = package(app);
    match q {
        Ask::NewAlternative(n) if !n.trim().is_empty() => new_alternative(app, n.trim()),
        Ask::RenameAlternative(k, n) if !n.trim().is_empty() => {
            if let Some(pkg) = pkg {
                if let Err(e) = package::rename(&pkg, k, n.trim()) {
                    app.tell(format!("Couldn't rename: {e}"));
                }
            }
        }
        Ask::RenameProject(n) if !n.trim().is_empty() => {
            if let Some(pkg) = pkg {
                save(app);
                match package::rename_package(&pkg, n.trim()) {
                    Ok(dest) => app.open_path(&dest),
                    Err(e) => app.tell(format!("Couldn't rename: {e}")),
                }
            }
        }
        Ask::DeleteAlternative(k, _) => {
            if let (Some(pkg), Some(open)) = (pkg, open_key(app)) {
                if let Err(e) = package::delete(&pkg, k, open) {
                    app.tell(format!("Couldn't delete: {e}"));
                }
            }
        }
        Ask::CleanUp(files) => {
            let n = to_trash(&files);
            app.tell(format!("Clean Up: {n} unused file(s) moved to the Trash"));
        }
        Ask::Switch(k) => {
            if yes {
                save(app);
            }
            switch_alternative(app, k);
        }
        _ => {}
    }
}

fn edit_alternatives(app: &mut App, ctx: &egui::Context) {
    if !app.pf.edit_alternatives {
        return;
    }
    let Some(pkg) = package(app) else {
        app.pf.edit_alternatives = false;
        return;
    };
    let open_now = open_key(app);
    let mut open = true;
    let mut action: Option<Ask> = None;
    let mut switch: Option<u32> = None;
    egui::Window::new("Edit Alternatives").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let all = package::list(&pkg);
        egui::Grid::new("alts").striped(true).show(ui, |ui| {
            for a in &all {
                let on = Some(a.key) == open_now;
                ui.label(if on { format!("✓ {}", a.name) } else { format!("   {}", a.name) });
                if ui.small_button("Rename…").clicked() {
                    action = Some(Ask::RenameAlternative(a.key, a.name.clone()));
                }
                if ui.add_enabled(!on, egui::Button::new("Open").small()).clicked() {
                    switch = Some(a.key);
                }
                if ui.add_enabled(!on && all.len() > 1, egui::Button::new("Delete…").small()).clicked() {
                    action = Some(Ask::DeleteAlternative(a.key, a.name.clone()));
                }
                ui.end_row();
            }
        });
    });
    if action.is_some() {
        app.pf.ask = action;
    }
    if let Some(k) = switch {
        if app.dirty {
            app.pf.ask = Some(Ask::Switch(k));
        } else {
            switch_alternative(app, k);
        }
    }
    if !open {
        app.pf.edit_alternatives = false;
    }
}

fn project_settings(app: &mut App, ctx: &egui::Context) {
    let Some(mut tab) = app.pf.settings.clone() else { return };
    let mut open = true;
    crate::projsettings::window(app, ctx, &mut tab, &mut open);
    app.pf.settings = open.then_some(tab);
}

/// Project Settings ▸ General: the project, where it's saved, tempo, signature, key, and what
/// the last Save couldn't write.
pub fn general_info(app: &mut App, ui: &mut egui::Ui) {
    let Some(p) = app.project.clone() else { return };
    egui::Grid::new("ps-general").num_columns(2).show(ui, |ui| {
        ui.label("Project:");
        ui.label(&p.name);
        ui.end_row();
        ui.label("Saved in:");
        ui.label(match (&app.pf.band, package(app)) {
            (Some(b), _) => b.display().to_string(),
            (_, Some(pkg)) => pkg.display().to_string(),
            _ => "not saved yet".into(),
        });
        ui.end_row();
        if let Some(pkg) = package(app) {
            ui.label("Alternatives:");
            ui.label(package::list(&pkg).iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "));
            ui.end_row();
        }
        ui.label("Tempo:");
        let mut bpm = p.tempo;
        if ui.add(egui::DragValue::new(&mut bpm).range(5.0..=990.0).speed(0.1).suffix(" BPM")).changed() {
            app.checkpoint("Tempo");
            if let Some(pr) = app.project.as_mut() {
                pr.tempo = bpm;
                if let Some(first) = pr.tempo_map.first_mut() {
                    first.1 = bpm;
                }
            }
            app.after_edit();
        }
        ui.end_row();
        ui.label("Time signature:");
        ui.label(format!("{}/{}", p.signature.0, p.signature.1));
        ui.end_row();
        ui.label("Key:");
        ui.label(&p.key);
        ui.end_row();
    });
    if !app.pf.last_left_out.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Not in the Logic file after the last save:").color(crate::look::text_dim()));
        egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
            for l in &app.pf.last_left_out {
                ui.label(format!("• {l}"));
            }
        });
    }
    
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layout_makes_its_tracks() {
        for (k, (name, tracks)) in LAYOUTS.iter().enumerate() {
            let p = layout_project(k);
            assert_eq!(p.tracks.len(), tracks.len(), "{name}");
            assert!(p.tracks.iter().zip(tracks.iter()).all(|(t, (n, kind))| t.name == *n && t.kind == *kind));
        }
    }

    #[test]
    fn dates_read_as_logic_shows_them() {
        // 2026-11-09 21:02:27 UTC
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_794_258_147);
        assert_eq!(when(t), "11/9/26, 21:02:27");
    }

    #[test]
    fn commands_are_listed() {
        for c in ["Save", "Save A Copy As…", "New Alternative…", "Edit Alternatives…", "Clean Up…", "Consolidate…", "Revert to Saved"] {
            assert!(COMMANDS.contains(&c), "{c}");
        }
    }
}
