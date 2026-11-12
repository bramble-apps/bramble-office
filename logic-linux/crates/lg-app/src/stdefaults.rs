//! Project Settings ▸ Smart Tempo's defaults, as Logic's: Set New Recordings To and Set Imported
//! Files To (Flex Off / Flex On, Smart Tempo Off / Flex On, Smart Tempo On / … + Align Bars /
//! … + Align Bars and Beats), each with Trim start of new regions (the region starts at the
//! file's first downbeat), and Export Tempo Resolution (Smoothed / Beats) for Apply Region
//! Tempo. The Audio Defaults inspector's Flex and Smart Tempo rows edit Set New Recordings To.

use std::path::{Path, PathBuf};

use crate::app::App;

pub const REC_KEY: &str = "Smart Tempo /  / Set New Recordings To:";
pub const REC_TRIM: &str = "Smart Tempo /  / Trim start of new regions";
pub const IMP_KEY: &str = "Smart Tempo /  / Set Imported Files To:";
pub const IMP_TRIM: &str = "Smart Tempo /  / Trim start of new regions #2";
pub const EXPORT_KEY: &str = "Smart Tempo /  / Export Tempo Resolution:";

pub const CHOICES: [&str; 5] = [
    "Flex Off",
    "Flex On, Smart Tempo Off",
    "Flex On, Smart Tempo On",
    "Flex On, Smart Tempo On + Align Bars",
    "Flex On, Smart Tempo On + Align Bars and Beats",
];
pub const EXPORT: [&str; 2] = ["Smoothed", "Beats"];

/// A new region waiting for its file's analysis: what to do then.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub file: PathBuf,
    /// Trim its start to the first downbeat.
    pub trim: bool,
    /// Move it so the first downbeat sits on the nearest bar (imports with an Align option).
    pub align: bool,
}

pub fn choice(v: &crate::projsettings::Values, key: &str) -> usize {
    CHOICES.iter().position(|c| *c == v.get(key)).unwrap_or(0)
}

/// The Region inspector's Smart Tempo value (0 Off, 1 On, 2 + Align Bars, 3 + Beats) of a choice,
/// and whether it flexes.
pub fn smart_of(choice: usize) -> (bool, usize) {
    match choice {
        0 => (false, 0),
        1 => (true, 0),
        c => (true, (c - 1).min(3)),
    }
}

/// The choice for the Audio Defaults' Flex checkbox and Smart Tempo menu.
pub fn choice_of(flex: bool, smart: usize) -> usize {
    match (flex, smart) {
        (false, _) => 0,
        (true, 0) => 1,
        (true, s) => (s + 1).min(4),
    }
}

fn find_region(app: &App, file: &Path) -> Option<(usize, usize)> {
    let p = app.project.as_ref()?;
    p.tracks.iter().enumerate().find_map(|(t, tr)| tr.regions.iter().position(|r| r.audio.as_ref().is_some_and(|x| x.file == file)).map(|r| (t, r)))
}

/// Applies a choice to the region on `file`.
fn apply(app: &mut App, t: usize, r: usize, file: &Path, c: usize, trim: bool, align: bool) {
    let (flex, smart) = smart_of(c);
    if flex && smart == 0 {
        // Flex On, Smart Tempo Off: the file is taken to be at the project tempo and follows it
        let keep = (app.selected_region, app.selected_track);
        app.selected_region = Some((t, r));
        crate::smarttempo::apply_project_tempo(app);
        (app.selected_region, app.selected_track) = keep;
        let mut x = crate::audioedit::extras(app, t, r);
        x.follow = true;
        x.smart_tempo = 0;
        crate::audioedit::set_extras(app, t, r, x);
        let mut tf = app.flex.tracks.get(&t).cloned().unwrap_or_default();
        tf.on = true;
        app.flex.tracks.insert(t, tf);
    } else if flex {
        crate::tempomode::set_smart_tempo(app, t, r, smart);
    }
    if trim || align {
        if !app.audio_edit.st.maps.contains_key(file) && !crate::smarttempo::analysing(app, file) {
            crate::smarttempo::analyze(app, file);
        }
        app.tm2.st_pending.push(Pending { file: file.to_path_buf(), trim, align });
    }
}

/// A new recording (tempomode.rs saw its file): Set New Recordings To.
pub fn recorded(app: &mut App, file: &Path) {
    let Some((t, r)) = find_region(app, file) else { return };
    let c = choice(&app.proj, REC_KEY);
    let trim = app.proj.on(REC_TRIM);
    apply(app, t, r, file, c, trim, false);
}

/// An imported audio file's new region: Set Imported Files To (Flex Off: not analysed).
pub fn imported(app: &mut App, t: usize, r: usize) {
    let Some(file) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)?.audio.as_ref().map(|a| a.file.clone())) else { return };
    let c = choice(&app.proj, IMP_KEY);
    let trim = app.proj.on(IMP_TRIM);
    apply(app, t, r, &file, c, trim, c >= 3);
}

/// Each frame: analyses done → trim / align the waiting regions.
pub fn frame(app: &mut App) {
    if app.tm2.st_pending.is_empty() {
        return;
    }
    let ready: Vec<Pending> = app.tm2.st_pending.iter().filter(|p| app.audio_edit.st.maps.contains_key(&p.file)).cloned().collect();
    for p in ready {
        app.tm2.st_pending.retain(|x| *x != p);
        let Some((t, r)) = find_region(app, &p.file) else { continue };
        let Some(a) = app.audio_edit.st.maps.get(&p.file) else { continue };
        let Some(&down) = a.map.beats.get(a.map.downbeat) else { continue };
        if p.trim {
            trim_to(app, t, r, down);
        }
        if p.align {
            align_to_bar(app, t, r, down);
        }
    }
}

/// The region starts at `down` (seconds into the file) when that is inside it.
fn trim_to(app: &mut App, t: usize, r: usize, down: f64) {
    let Some(rt) = crate::smarttempo::region_time(app, t, r) else { return };
    let ds = down - rt.offset_sec;
    if ds <= 1e-4 || ds >= rt.len_sec {
        return;
    }
    let rate = crate::audioedit::rate_of(app, &rt.file).unwrap_or(48_000) as f64;
    let mut x = crate::audioedit::extras(app, t, r);
    x.offset += (ds * rate).round() as usize;
    crate::audioedit::set_extras(app, t, r, x);
    let a = crate::smarttempo::tick_at(app, rt.start_sec + ds).round() as u32;
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        let end = rg.start + rg.length;
        rg.start = a.min(end.saturating_sub(1));
        rg.length = end - rg.start;
    }
    app.after_edit();
}

/// Moves the region so the file's first downbeat sits on the nearest bar line.
fn align_to_bar(app: &mut App, t: usize, r: usize, down: f64) {
    let Some(rt) = crate::smarttempo::region_time(app, t, r) else { return };
    let at = crate::smarttempo::tick_at(app, rt.start_sec + (down - rt.offset_sec));
    let bar = app.bar_ticks() as f64;
    let target = (at / bar).round() * bar;
    let d = (target - at).round() as i64;
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        rg.start = (rg.start as i64 + d).max(0) as u32;
    }
    app.after_edit();
}

/// Export Tempo Resolution: Smoothed evens out beat-to-beat jumps (each beat's tempo the mean of
/// it and its neighbours); Beats keeps every beat's own tempo.
pub fn smooth(bpms: &mut [f64], v: &crate::projsettings::Values) {
    if v.get(EXPORT_KEY) == "Beats" || bpms.len() < 3 {
        return;
    }
    let src = bpms.to_vec();
    for i in 0..src.len() {
        let a = i.saturating_sub(1);
        let b = (i + 1).min(src.len() - 1);
        let s: f64 = src[a..=b].iter().sum();
        bpms[i] = (s / (b - a + 1) as f64 * 10_000.0).round() / 10_000.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_map_to_flex_and_smart_tempo() {
        assert_eq!(smart_of(0), (false, 0));
        assert_eq!(smart_of(1), (true, 0));
        assert_eq!(smart_of(2), (true, 1));
        assert_eq!(smart_of(4), (true, 3));
        for c in 0..5 {
            let (f, s) = smart_of(c);
            assert_eq!(choice_of(f, s), c);
        }
        assert_eq!(choice_of(false, 3), 0);
    }

    #[test]
    fn smoothing_follows_the_setting() {
        let mut v = crate::projsettings::Values::default();
        let mut b = vec![100.0, 110.0, 100.0, 110.0];
        smooth(&mut b, &v);
        assert!((b[1] - 103.3333).abs() < 1e-3, "{b:?}");
        crate::projsettings::set(&mut v, EXPORT_KEY, "Beats");
        let mut b = vec![100.0, 110.0, 100.0];
        smooth(&mut b, &v);
        assert_eq!(b, vec![100.0, 110.0, 100.0]);
    }
}
