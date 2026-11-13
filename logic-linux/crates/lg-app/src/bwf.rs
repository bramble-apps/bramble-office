//! Broadcast Wave timestamps, as Logic's: a BWF file's time reference (its bext chunk: samples
//! since midnight) is read; timestamped files show a clock beside their name in the Project
//! Audio Browser, and Edit ▸ Move ▸ To Recorded Position (⌥⇧⌘R) puts the selected regions back
//! where their audio was recorded (through the project's bar 1 SMPTE time).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::app::App;

pub const COMMANDS: &[&str] = &["Move Region to Recorded Position", "To Recorded Position"];

thread_local! {
    static CACHE: RefCell<HashMap<PathBuf, Option<(u64, u32)>>> = RefCell::new(HashMap::new());
}

/// A WAVE file's bext time reference (samples since midnight) and its sample rate.
pub fn read_time_reference(bytes: &[u8]) -> Option<(u64, u32)> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let (mut i, mut rate, mut tref) = (12usize, 0u32, None);
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().ok()?) as usize;
        let body = &bytes[i + 8..(i + 8 + len).min(bytes.len())];
        if id == b"fmt " && body.len() >= 8 {
            rate = u32::from_le_bytes(body[4..8].try_into().ok()?);
        }
        if id == b"bext" && body.len() >= 346 {
            let lo = u32::from_le_bytes(body[338..342].try_into().ok()?) as u64;
            let hi = u32::from_le_bytes(body[342..346].try_into().ok()?) as u64;
            tref = Some((hi << 32) | lo);
        }
        if id == b"data" {
            break;
        }
        i += 8 + len + (len & 1);
    }
    tref.filter(|t| *t > 0).map(|t| (t, rate.max(1)))
}

/// A file's timestamp (cached).
pub fn timestamp(p: &Path) -> Option<(u64, u32)> {
    if let Some(v) = CACHE.with(|c| c.borrow().get(p).copied()) {
        return v;
    }
    let v = {
        use std::io::Read;
        let mut b = vec![0u8; 65_536];
        let n = std::fs::File::open(p).and_then(|mut f| f.read(&mut b)).unwrap_or(0);
        b.truncate(n);
        read_time_reference(&b)
    };
    CACHE.with(|c| c.borrow_mut().insert(p.to_path_buf(), v));
    v
}

/// Seconds from bar 1 where audio stamped `(samples, rate)` belongs, with bar 1 at `bar1` s.
pub fn recorded_seconds((samples, rate): (u64, u32), bar1: f64) -> f64 {
    samples as f64 / rate as f64 - bar1
}

pub fn command(app: &mut App, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
    }
    let bar1 = crate::projsettings::smpte_real(&app.proj).0;
    let sel = app.region_selection();
    let mut moves: Vec<((usize, usize), u32)> = Vec::new();
    for (t, r) in sel {
        let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.get(r)) else { continue };
        let Some(a) = rg.audio.as_ref() else { continue };
        let Some(ts) = timestamp(&a.file) else { continue };
        // (the region's start in its file moves with it)
        let off = rg.props.audio_offset as f64 / ts.1 as f64;
        let secs = recorded_seconds(ts, bar1) + off;
        if secs >= 0.0 {
            moves.push(((t, r), crate::smarttempo::tick_at(app, secs).max(0.0) as u32));
        }
    }
    if moves.is_empty() {
        app.tell("No selected audio region has a recorded position (a Broadcast Wave timestamp)");
        return true;
    }
    app.checkpoint("Move Region to Recorded Position");
    if let Some(p) = app.project.as_mut() {
        for ((t, r), at) in moves {
            if let Some(rg) = p.tracks.get_mut(t).and_then(|tr| tr.regions.get_mut(r)) {
                rg.start = at;
            }
        }
    }
    app.after_edit();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_bext_time_reference() {
        let mut bext = vec![0u8; 602];
        bext[338..342].copy_from_slice(&(48_000u32 * 3600).to_le_bytes());
        let mut w: Vec<u8> = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        w.extend(16u32.to_le_bytes());
        w.extend([1, 0, 2, 0]);
        w.extend(48_000u32.to_le_bytes());
        w.extend([0; 8]);
        w.extend(b"bext");
        w.extend((bext.len() as u32).to_le_bytes());
        w.extend(bext);
        w.extend(b"data\0\0\0\0");
        let ts = read_time_reference(&w).unwrap();
        assert_eq!(ts, (48_000 * 3600, 48_000));
        // bar 1 at 01:00:00:00: the audio belongs at bar 1
        assert_eq!(recorded_seconds(ts, 3600.0), 0.0);
        assert_eq!(read_time_reference(b"RIFF\0\0\0\0WAVE"), None);
    }
}
