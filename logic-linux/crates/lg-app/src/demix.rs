//! Demix MIDI regions (Edit ▸ Separate MIDI Events, the region menu's MIDI ▸ Separate by …):
//! - by MIDI Channel: one region per event channel; channel 1 stays on the region's track, the
//!   others go onto tracks below it whose name ends in "Ch n" (made with the same channel strip
//!   when there is none yet);
//! - by Note Pitch: one region per note number, each on a new track with the original channel
//!   strip, named after the note.
//! Other events (controllers…) stay with the first region. One undo step.

use std::collections::BTreeMap;

use lg_core::events::Note;

use crate::app::App;

pub const COMMANDS: &[&str] = &["Separate MIDI Region by MIDI Channel", "Separate by MIDI Channel", "Separate MIDI Region by Note Pitch", "Separate by Note Pitch"];

/// The notes split by a key, in key order.
pub fn split_by(notes: &[Note], key: impl Fn(&Note) -> u8) -> BTreeMap<u8, Vec<Note>> {
    let mut m: BTreeMap<u8, Vec<Note>> = BTreeMap::new();
    for n in notes {
        m.entry(key(n)).or_default().push(*n);
    }
    m
}

pub fn command(app: &mut App, name: &str) -> bool {
    let by_pitch = match name {
        "Separate MIDI Region by MIDI Channel" | "Separate by MIDI Channel" => false,
        "Separate MIDI Region by Note Pitch" | "Separate by Note Pitch" => true,
        _ => return false,
    };
    let sel: Vec<(usize, usize)> = app.region_selection().into_iter().filter(|&(t, r)| app.project.as_ref().is_some_and(|p| p.tracks[t].regions[r].audio.is_none())).collect();
    if sel.is_empty() {
        app.tell("Select a MIDI region to separate");
        return true;
    }
    app.checkpoint(if by_pitch { "Separate by Note Pitch" } else { "Separate by MIDI Channel" });
    // bottom regions first, so track indexes above stay
    let mut sel = sel;
    sel.sort_by(|a, b| b.cmp(a));
    let mut made = 0;
    for (t, r) in sel {
        made += separate(app, t, r, by_pitch);
    }
    if made == 0 {
        app.tell(if by_pitch { "The region has only one note pitch" } else { "The region's events are all on one MIDI channel" });
    }
    app.after_edit();
    true
}

fn separate(app: &mut App, t: usize, r: usize, by_pitch: bool) -> usize {
    let Some(p) = app.project.as_mut() else { return 0 };
    let rg = p.tracks[t].regions[r].clone();
    let groups = split_by(&rg.notes, |n| if by_pitch { n.pitch } else { n.channel });
    if groups.len() < 2 {
        return 0;
    }
    let base = p.tracks[t].clone();
    let prog = app.programs.get(t).copied().unwrap_or(0);
    let mut new_tracks = 0;
    let mut first = true;
    for (key, notes) in groups {
        let mut n = rg.clone();
        n.notes = notes;
        n.source = None;
        n.origin = rg.source.or(rg.origin);
        if !first {
            n.ext.events.clear();
        }
        let label = if by_pitch { crate::app::note_name(key) } else { format!("Ch {}", key + 1) };
        n.name = format!("{} {}", rg.name, label);
        let p = app.project.as_mut().expect("project");
        if !by_pitch && key == 0 {
            // channel 1 stays where it was
            p.tracks[t].regions[r] = n;
            first = false;
            continue;
        }
        let tname = format!("{} {}", base.name, label);
        // a track for this channel already under the original
        let found = (!by_pitch).then(|| (t + 1..p.tracks.len()).take_while(|&k| p.tracks[k].name.starts_with(&base.name)).find(|&k| p.tracks[k].name == tname)).flatten();
        match found {
            Some(k) => p.tracks[k].regions.push(n),
            None => {
                let mut tr = base.clone();
                tr.name = tname;
                tr.regions = vec![n];
                tr.row = p.tracks.len() as u32 + 1;
                let at = t + 1 + new_tracks;
                p.tracks.insert(at, tr);
                app.programs.insert(at.min(app.programs.len()), prog);
                new_tracks += 1;
            }
        }
        first = false;
    }
    // the original region (when not reused by channel 1)
    let p = app.project.as_mut().expect("project");
    if by_pitch || !rg.notes.iter().any(|n| n.channel == 0) {
        p.tracks[t].regions.remove(r);
    }
    new_tracks.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits() {
        let n = |ch: u8, p: u8| Note { channel: ch, pitch: p, velocity: 90, pos: 0, len: 10, muted: false };
        let v = vec![n(0, 60), n(1, 62), n(0, 64), n(9, 36)];
        let by_ch = split_by(&v, |x| x.channel);
        assert_eq!(by_ch.keys().copied().collect::<Vec<_>>(), vec![0, 1, 9]);
        assert_eq!(by_ch[&0].len(), 2);
        let by_p = split_by(&v, |x| x.pitch);
        assert_eq!(by_p.len(), 4);
    }
}
