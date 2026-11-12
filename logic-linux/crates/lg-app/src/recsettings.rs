//! Project Settings ▸ Recording, the rows recording.rs doesn't read itself: Count-in or Pre-roll
//! (a lead-in in seconds, up to 60, played without count-in clicks), Allow tempo change
//! recording (a tempo set while recording becomes a tempo change at the playhead), and
//! Automatically colorize takes (each take of a folder its own colour from the track colour's
//! palette row).

use crate::app::App;

pub const LEAD_IN: &str = "Recording / General / Lead-in:";
pub const PRE_ROLL: &str = "Recording / General / Pre-roll:";
pub const TEMPO_REC: &str = "Recording / General / Allow tempo change recording";
pub const COLORIZE: &str = "Recording / MIDI / Automatically colorize takes";
pub const LEADS: [&str; 2] = ["Count-in", "Pre-roll"];

pub const GB_MONITOR: &str = "Audio / General / Input monitoring only for the focused track, and only when input monitoring is enabled (as in GarageBand)";
pub const INDEPENDENT: &str = "Audio / General / Independent monitoring level for record-enabled channel strips";

/// Input monitoring as in GarageBand: of the tracks with Input Monitoring on, only the focused
/// (selected) one is heard; record-enabled tracks while recording still are.
pub fn gb_monitor_filter(app: &App, mon: &mut Vec<(usize, crate::recording::InputSel)>, recording: &[usize]) {
    if app.logic_settings.on(GB_MONITOR) {
        mon.retain(|m| Some(m.0) == app.selected_track || recording.contains(&m.0));
    }
}

/// Independent monitoring level: a record-enabled strip's fader is put back where it was when
/// its record enable goes off (the level set while armed is only for monitoring; recordings
/// are taken before the fader).
#[derive(Default)]
pub struct Armed {
    pub levels: std::collections::BTreeMap<usize, f32>,
}

pub fn frame(app: &mut App) {
    let on = app.logic_settings.on(INDEPENDENT);
    let Some(p) = app.project.as_mut() else { return };
    let st = &mut app.tm2.armed_levels;
    if !on {
        st.levels.clear();
        return;
    }
    let mut restored = false;
    for t in app.armed.iter() {
        if let Some(tr) = p.tracks.get(*t) {
            st.levels.entry(*t).or_insert(tr.volume_db);
        }
    }
    let gone: Vec<usize> = st.levels.keys().copied().filter(|t| !app.armed.contains(t)).collect();
    for t in gone {
        if let (Some(v), Some(tr)) = (st.levels.remove(&t), p.tracks.get_mut(t)) {
            if tr.volume_db != v {
                tr.volume_db = v;
                restored = true;
            }
        }
    }
    if restored {
        app.sync_mix();
    }
}

pub const REC_DELAY: &str = "Audio / Devices / Recording Delay:";

/// Recording Delay (Settings ▸ Audio ▸ Devices, samples): new audio recordings are moved by it
/// to make up for the driver's delay (positive: the audio came in late, so it moves earlier).
pub fn delay_regions(app: &App, regions: &mut [lg_core::Region]) {
    let d: f64 = app.logic_settings.get(REC_DELAY).parse().unwrap_or(0.0);
    if d == 0.0 {
        return;
    }
    for r in regions.iter_mut() {
        let s = app.seconds_at(r.start as f64) - d / crate::audio::RATE as f64;
        r.start = crate::smarttempo::tick_at(app, s.max(0.0)).round().max(0.0) as u32;
    }
}

pub const CONVERT_RATE: &str = "Assets /  / Convert audio file sample rate when importing";

/// Assets ▸ Convert audio file sample rate when importing: a WAV / AIFF at another rate than the
/// project's is converted into the project's Audio Files folder (the copy is used; the original
/// stays). Returns the file to use.
pub fn import_converted(app: &App, path: &std::path::Path) -> std::path::PathBuf {
    let rate = crate::audio::RATE as u32;
    if !app.proj.on(CONVERT_RATE) {
        return path.to_path_buf();
    }
    let Ok(a) = lg_audio::AudioFile::read(path) else { return path.to_path_buf() };
    if a.rate == rate || a.frames() == 0 {
        return path.to_path_buf();
    }
    let dir = crate::recording::audio_dir(app);
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Audio".into());
    let out = dir.join(format!("{stem} ({} kHz).wav", rate as f32 / 1000.0));
    if out.exists() {
        return out;
    }
    let frames = (a.frames() as f64 * rate as f64 / a.rate as f64).round() as usize;
    let mut b = lg_audio::AudioFile::new(rate, a.channels, lg_audio::stretch::resample(&a.data, a.channels, frames));
    b.bits = a.bits.max(16);
    b.float = a.float;
    if std::fs::create_dir_all(&dir).is_err() || b.write(&out).is_err() {
        return path.to_path_buf();
    }
    out
}

/// The lead-in before a recording: Some(ticks of pre-roll) when Pre-roll is chosen.
pub fn pre_roll_ticks(app: &App, from: f64) -> Option<f64> {
    if app.proj.get(LEAD_IN) != "Pre-roll" {
        return None;
    }
    let secs = app.proj.num(PRE_ROLL).clamp(0.0, 60.0) as f64;
    let at = app.seconds_at(from);
    let start = (at - secs).max(0.0);
    Some(from - crate::smarttempo::tick_at(app, start))
}

/// Allow tempo change recording: while recording, a tempo change goes into the tempo list at
/// the playhead (true: done here).
pub fn record_tempo(app: &mut App, bpm: f64) -> bool {
    if !app.recording() || !app.proj.on(TEMPO_REC) {
        return false;
    }
    let at = app.position().max(0.0) as u32;
    let Some(p) = app.project.as_mut() else { return false };
    insert_tempo(&mut p.tempo_map, p.tempo, at, bpm);
    let m = p.tempo_map.clone();
    app.dirty = true;
    if let Ok(mut e) = app.engine.lock() {
        e.tempo_map = m;
    }
    true
}

pub fn insert_tempo(map: &mut Vec<(u32, f64)>, base: f64, at: u32, bpm: f64) {
    if map.is_empty() {
        map.push((0, base));
    }
    map.retain(|e| e.0 != at || at == 0);
    if at == 0 {
        map[0].1 = bpm;
    } else {
        map.push((at, bpm));
        map.sort_by_key(|e| e.0);
    }
}

/// Automatically colorize takes: take `k`'s colour (None: the track's own).
pub fn take_color(app: &App, track: usize, k: usize) -> Option<egui::Color32> {
    if !app.proj.on(COLORIZE) {
        return None;
    }
    let t = app.project.as_ref()?.tracks.get(track)?;
    let cols = lg_core::trackmix::PALETTE_COLS;
    let base = t.extra.set.strip.color.map_or(match t.kind {
        lg_core::TrackKind::Audio => cols + 14,
        _ => cols + 7,
    }, |c| c as usize);
    let i = take_index(base, k, cols);
    let [r, g, b] = lg_core::trackmix::palette(i as u8);
    Some(egui::Color32::from_rgb(r, g, b))
}

/// The palette index of take `k`: the same row, three columns on per take.
pub fn take_index(base: usize, k: usize, cols: usize) -> usize {
    let row = base / cols;
    row * cols + (base % cols + k * 3) % cols
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempo_changes_go_in_at_the_playhead() {
        let mut m = vec![(0, 120.0)];
        insert_tempo(&mut m, 120.0, 1920, 90.0);
        insert_tempo(&mut m, 120.0, 960, 100.0);
        insert_tempo(&mut m, 120.0, 1920, 95.0);
        assert_eq!(m, vec![(0, 120.0), (960, 100.0), (1920, 95.0)]);
        insert_tempo(&mut m, 120.0, 0, 110.0);
        assert_eq!(m[0], (0, 110.0));
    }

    #[test]
    fn takes_stay_on_their_palette_row() {
        assert_eq!(take_index(30, 0, 24), 30);
        assert_eq!(take_index(30, 1, 24), 33);
        assert_eq!(take_index(46, 1, 24), 25);
        for k in 0..10 {
            assert_eq!(take_index(30, k, 24) / 24, 1);
        }
    }
}
