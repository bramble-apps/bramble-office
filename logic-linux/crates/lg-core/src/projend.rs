//! The Project End marker (Logic: File ▸ Project Settings, the ruler's end marker, the LCD's
//! Project End). Playback stops there; recording past it moves it later.
//!
//! In the .logicx the end sits in the Song chunk at +0x1A4 (from the chunk start, incl. its
//! 36-byte header) as an event position (bar 1 = `events::EVENT_BAR_ONE`). Every sample project
//! made from Logic's Empty Project holds 529920 there: bar 129, so a new project is 128 bars long.

use crate::chunks;
use crate::events::EVENT_BAR_ONE;
use crate::project::{bar_ticks, Project};

/// The Song chunk offset of the project end.
pub const SONG_END_AT: usize = 0x1A4;
/// Logic's default length for a new project, in bars (the end marker at bar 129).
pub const DEFAULT_BARS: u32 = 128;

/// The project end (ticks from bar 1): the one set, else Logic's default of 128 bars.
pub fn end(p: &Project) -> u32 {
    p.end.unwrap_or_else(|| DEFAULT_BARS * bar_ticks(p.signature))
}

/// For a song read without an end (MIDI files, Studio's own): Logic's default, or past the last
/// region (rounded up to a bar) when the song is longer.
pub fn fit(p: &mut Project) {
    if p.end.is_some() {
        return;
    }
    let bar = bar_ticks(p.signature).max(1);
    let content = p.tracks.iter().flat_map(|t| t.regions.iter()).map(|r| r.start + r.length).max().unwrap_or(0);
    p.end = Some((DEFAULT_BARS * bar).max(content.div_ceil(bar) * bar));
}

/// Recording past the end extends it (to the bar after `pos`). True when it moved.
pub fn extend_for_recording(p: &mut Project, pos: u32) -> bool {
    let e = end(p);
    if pos < e {
        return false;
    }
    let bar = bar_ticks(p.signature).max(1);
    p.end = Some((pos / bar + 1) * bar);
    true
}

/// Where a dragged end lands: on a bar, at least one bar after the project start.
pub fn snap_end(p: &Project, tick: f32) -> u32 {
    let bar = bar_ticks(p.signature).max(1) as f32;
    ((tick / bar).round().max(1.0) * bar) as u32
}

/// The end as stored in ProjectData (None: no Song chunk or no room for it).
pub fn read(data: &[u8]) -> Option<u32> {
    let chunks = chunks::walk(data).ok()?;
    let song = chunks.iter().find(|c| c.is("Song"))?;
    song.u32_at(SONG_END_AT).filter(|&v| v > EVENT_BAR_ONE).map(|v| v - EVENT_BAR_ONE)
}

/// Writes the end into ProjectData bytes (a whole file: 24-byte header, then chunks).
pub fn write(data: &mut [u8], end: u32) {
    let Ok(chunks) = chunks::walk(data) else { return };
    let Some(at) = chunks.iter().find(|c| c.is("Song")).filter(|c| c.u32_at(SONG_END_AT).is_some()).map(|c| c.start + SONG_END_AT) else { return };
    data[at..at + 4].copy_from_slice(&(end + EVENT_BAR_ONE).to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Region, Track, TrackKind};

    fn song_with_region(end_tick: u32) -> Project {
        let mut p = Project { signature: (4, 4), ..Default::default() };
        let mut t = Track::new("T", TrackKind::Instrument);
        t.regions.push(Region { name: "R".into(), start: 0, length: end_tick, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
        p.tracks.push(t);
        p
    }

    #[test]
    fn new_project_ends_at_bar_129() {
        let p = Project { signature: (4, 4), ..Default::default() };
        assert_eq!(end(&p), 128 * 3840);
        let p34 = Project { signature: (3, 4), ..Default::default() };
        assert_eq!(end(&p34), 128 * 2880);
    }

    #[test]
    fn fit_keeps_default_or_covers_a_longer_song() {
        let mut short = song_with_region(4 * 3840);
        fit(&mut short);
        assert_eq!(short.end, Some(128 * 3840));
        let mut long = song_with_region(200 * 3840 + 10);
        fit(&mut long);
        assert_eq!(long.end, Some(201 * 3840));
        // a stored end is left alone
        let mut set = song_with_region(200 * 3840);
        set.end = Some(16 * 3840);
        fit(&mut set);
        assert_eq!(set.end, Some(16 * 3840));
    }

    #[test]
    fn recording_past_the_end_extends_it() {
        let mut p = Project { signature: (4, 4), end: Some(8 * 3840), ..Default::default() };
        assert!(!extend_for_recording(&mut p, 7 * 3840));
        assert!(extend_for_recording(&mut p, 8 * 3840 + 100));
        assert_eq!(p.end, Some(9 * 3840));
    }

    #[test]
    fn snaps_to_bars_never_before_bar_2() {
        let p = Project { signature: (4, 4), ..Default::default() };
        assert_eq!(snap_end(&p, 3840.0 * 5.4), 5 * 3840);
        assert_eq!(snap_end(&p, -100.0), 3840);
    }

    #[test]
    fn reads_and_writes_the_song_chunk() {
        // header (24 bytes) + a Song chunk big enough for +0x1A4
        let payload = vec![0u8; SONG_END_AT + 8 - 36];
        let mut body = vec![0u8; 36];
        body[..4].copy_from_slice(b"gnoS");
        body[28..36].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        body.extend(payload);
        let mut data = crate::chunks::MAGIC.to_vec();
        data.extend_from_slice(&[0; 12]);
        data.extend_from_slice(&(body.len() as u32).to_le_bytes());
        data.extend_from_slice(&[0; 4]);
        data.append(&mut body);
        write(&mut data, 128 * 3840);
        assert_eq!(read(&data), Some(128 * 3840));
        assert_eq!(u32::from_le_bytes(data[24 + SONG_END_AT..24 + SONG_END_AT + 4].try_into().unwrap()), 529920);
        write(&mut data, 20 * 3840);
        assert_eq!(read(&data), Some(20 * 3840));
    }

    #[test]
    fn logic_empty_project_ends_at_bar_129() {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata-private/sample-files");
        let main = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("CascadeProjects/bramble-office/logic-linux/testdata-private/sample-files");
        let Some(dir) = [here, main].into_iter().find(|d| d.is_dir()) else { return };
        let Ok(data) = std::fs::read(dir.join("E00-empty-1-SI-track.logicx/Alternatives/000/ProjectData")) else { return };
        assert_eq!(read(&data), Some(128 * 3840));
        let p = crate::project::read_project_data(&data).unwrap();
        assert_eq!(end(&p), 128 * 3840);
    }
}
