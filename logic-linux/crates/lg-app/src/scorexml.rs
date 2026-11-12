//! File ▸ Export ▸ Score as MusicXML…: the score shown (the Score Editor's tracks, else every
//! MIDI track) as a MusicXML file, written by lg-notation from the same engraving the editor
//! draws, with the score's dynamics, text, lyrics and bar lines and repeats.

use lg_core::score::{BarKind, SymbolKind};
use lg_notation as nt;
use lg_notation::musicxml::{self, Barline};

use crate::app::App;

pub const COMMANDS: &[&str] = &["Score as MusicXML…", "Export Score as MusicXML…", "MusicXML…", "Import MusicXML…"];

pub fn command(app: &mut App, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
    }
    if matches!(name, "MusicXML…" | "Import MusicXML…") {
        if app.project.is_none() {
            return true;
        }
        let path = if app.automated { std::env::var("LG_IMPORT_XML").ok().map(std::path::PathBuf::from) } else { kn_ui::files::FileDialog::new().set_title("Import MusicXML").add_filter("MusicXML", &["musicxml", "xml"]).pick_file() };
        if let Some(path) = path {
            import(app, &path);
        }
        return true;
    }
    let Some(p) = app.project.as_ref() else { return true };
    let title = p.name.clone();
    let Some(path) = (if app.automated { std::env::var("LG_EXPORT_XML").ok().map(std::path::PathBuf::from) } else { kn_ui::files::FileDialog::new().set_title("Export Score as MusicXML").set_file_name(format!("{title}.musicxml")).add_filter("MusicXML", &["musicxml", "xml"]).save_file() }) else { return true };
    let xml = text(app);
    match std::fs::write(&path, xml) {
        Ok(()) => app.tell(format!("Exported {}", path.display())),
        Err(e) => app.tell(format!("Couldn't export: {e}")),
    }
    true
}

/// The MusicXML text of the shown score.
pub fn text(app: &App) -> String {
    let ctx = app.ctx.clone();
    let mut st = crate::score::state(&ctx);
    // (with no region selected: every MIDI track, as Logic's All Instruments set)
    if app.selected_region.is_none() && st.score_set.is_none() {
        st.explode_folders = true;
    }
    let Some(p) = app.project.as_ref() else { return String::new() };
    let sig = p.signature;
    let bar_t = lg_core::project::bar_ticks(sig);
    let staffs = crate::score::engraved_staffs(app, &st);
    let mut parts: Vec<musicxml::Part> = Vec::new();
    let mut last_t = usize::MAX;
    for (t, staff, clef, voices) in staffs {
        if t != last_t {
            last_t = t;
            let tr = &p.tracks[t];
            let mut part = musicxml::Part { name: tr.name.clone(), staves: Vec::new(), directions: Vec::new(), lyrics: Vec::new() };
            if let Some(ts) = p.score.track(tr.extra.id) {
                for s in &ts.symbols {
                    match s.kind {
                        SymbolKind::Dynamic => part.directions.push((s.staff as usize, s.tick, true, s.text.clone())),
                        SymbolKind::Text | SymbolKind::Chord => part.directions.push((s.staff as usize, s.tick, false, s.text.clone())),
                        SymbolKind::Lyric => part.lyrics.push((s.staff as usize, s.tick, s.text.clone())),
                        _ => {}
                    }
                }
            }
            parts.push(part);
        }
        let _ = staff;
        if let Some(part) = parts.last_mut() {
            part.staves.push((clef, voices));
        }
    }
    let bars = parts
        .iter()
        .flat_map(|pt| pt.staves.iter().flat_map(|s| s.1.iter().flat_map(|v| v.items.iter().map(|i| i.tick / bar_t + 1))))
        .max()
        .unwrap_or(1);
    let barlines: Vec<(u32, Barline)> = p
        .score
        .bars
        .iter()
        .filter(|b| b.tick % bar_t == 0)
        .filter_map(|b| {
            let bar = b.tick / bar_t;
            Some(match b.kind {
                BarKind::Double => (bar, Barline::Double),
                BarKind::Final => (bar, Barline::Final),
                BarKind::Dashed => (bar, Barline::Dashed),
                BarKind::RepeatStart => (bar, Barline::RepeatStart),
                BarKind::RepeatEnd => (bar, Barline::RepeatEnd),
                _ => return None,
            })
        })
        .chain(p.score.bars.iter().filter(|b| b.kind == BarKind::RepeatBoth && b.tick % bar_t == 0).flat_map(|b| [(b.tick / bar_t, Barline::RepeatEnd), (b.tick / bar_t, Barline::RepeatStart)]))
        .collect();
    let fifths = nt::Key::parse(&p.key).fifths;
    musicxml::write(&p.name, &parts, sig, fifths, bars, &barlines)
}

/// File ▸ Import ▸ MusicXML: each part onto the track with its name (else a new software
/// instrument track below the others), as a MIDI region at the playhead's bar, with its clefs as
/// the track's staff style and its dynamics, words, wedges, slurs, chord symbols and lyrics as
/// score symbols. One undo step.
pub fn import(app: &mut App, path: &std::path::Path) {
    let Some(doc) = std::fs::read_to_string(path).ok().and_then(|x| lg_notation::musicxml_in::read(&x)) else {
        app.tell("Couldn't read that MusicXML file");
        return;
    };
    import_doc(app, doc, &path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
}

pub fn import_doc(app: &mut App, doc: lg_notation::musicxml_in::Imported, file: &str) {
    if app.project.is_none() {
        return;
    }
    let bar = app.bar_ticks();
    let at = ((app.position() as u32) / bar) * bar;
    let was_empty = app.project.as_ref().is_some_and(|p| p.tracks.iter().all(|t| t.regions.iter().all(|r| r.notes.is_empty() && r.audio.is_none())));
    app.checkpoint("Import MusicXML");
    let mut placed: Vec<(usize, usize)> = Vec::new();
    let n_parts = doc.parts.len();
    for (k, part) in doc.parts.iter().enumerate() {
        let Some(p) = app.project.as_mut() else { return };
        let name = if part.name.trim().is_empty() { if n_parts > 1 { format!("{file} {}", k + 1) } else { file.to_string() } } else { part.name.trim().to_string() };
        let end = part.notes.iter().map(|n| n.0 + n.1).max().unwrap_or(bar).max(part.symbols.iter().map(|s| s.tick + s.len).max().unwrap_or(0));
        let length = end.div_ceil(bar).max(1) * bar;
        let notes: Vec<lg_core::events::Note> = part.notes.iter().map(|&(pos, len, pitch, _, velocity)| lg_core::events::Note { channel: 0, pitch, velocity, pos: pos + at, len, muted: false }).collect();
        let region = lg_core::Region { name: name.clone(), start: at, length, notes, audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
        // (a track of the same name takes the part)
        let t = match p.tracks.iter().position(|t| t.kind != lg_core::TrackKind::Audio && t.name == name) {
            Some(t) => t,
            None => {
                let row = p.tracks.len() as u32 + 1;
                p.tracks.push(lg_core::Track { name: name.clone(), kind: lg_core::TrackKind::Instrument, channel: 0, row, regions: Vec::new(), volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new(), automation: Vec::new(), extra: Default::default() });
                app.programs.push(crate::app::program_for(&name, lg_core::TrackKind::Instrument));
                p.tracks.len() - 1
            }
        };
        p.tracks[t].regions.push(region);
        placed.push((t, p.tracks[t].regions.len() - 1));
    }
    // (new tracks get their ids before the score data is keyed by them)
    crate::trackarea::sync(app);
    let Some(p) = app.project.as_mut() else { return };
    if was_empty {
        // a project that was empty takes the file's key and time
        if let Some(sig) = doc.time {
            p.signature = sig;
        }
        const MAJOR: [&str; 15] = ["C♭", "G♭", "D♭", "A♭", "E♭", "B♭", "F", "C", "G", "D", "A", "E", "B", "F♯", "C♯"];
        p.key = format!("{} major", MAJOR[(doc.fifths.clamp(-7, 7) + 7) as usize]);
    }
    for (part, &(t, _)) in doc.parts.iter().zip(&placed) {
        let id = p.tracks[t].extra.id;
        let style = match part.clefs.as_slice() {
            [nt::Clef::Treble, nt::Clef::Bass] => "Piano",
            [nt::Clef::Bass] => "Bass",
            [nt::Clef::Alto] => "Alto Clef",
            [nt::Clef::Tenor] => "Tenor Clef",
            [nt::Clef::Percussion] => "Drums",
            _ => "",
        };
        let ts = p.score.track_mut(id);
        if !style.is_empty() {
            ts.style = style.to_string();
        }
        for s in &part.symbols {
            let kind = match s.kind {
                "dynamic" => SymbolKind::Dynamic,
                "lyric" => SymbolKind::Lyric,
                "chord" => SymbolKind::Chord,
                "crescendo" => SymbolKind::Crescendo,
                "decrescendo" => SymbolKind::Decrescendo,
                "slur" => SymbolKind::SlurUp,
                _ => SymbolKind::Text,
            };
            ts.symbols.push(lg_core::score::Symbol { tick: s.tick + at, kind, text: s.text.clone(), len: s.len, staff: s.staff, ..Default::default() });
        }
    }
    if let Some(&(t, r)) = placed.first() {
        app.selected_track = Some(t);
        app.selected_region = Some((t, r));
    }
    app.after_edit();
    app.tell(format!("Imported {} part{} from MusicXML", n_parts, if n_parts == 1 { "" } else { "s" }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use lg_notation::musicxml_in::{InPart, InSymbol, Imported};

    #[test]
    fn import_places_parts_on_named_or_new_tracks() {
        let mut app = crate::tm::test_app();
        let first = app.project.as_ref().unwrap().tracks.iter().find(|t| t.kind != lg_core::TrackKind::Audio).unwrap().name.clone();
        let n0 = app.project.as_ref().unwrap().tracks.len();
        let doc = Imported {
            title: "T".into(),
            fifths: 0,
            time: Some((3, 4)),
            parts: vec![
                InPart { name: first.clone(), notes: vec![(0, 960, 60, 0, 80)], clefs: vec![nt::Clef::Treble], symbols: vec![InSymbol { tick: 0, len: 0, staff: 0, kind: "dynamic", text: "mf".into() }] },
                InPart { name: "Cello Import".into(), notes: vec![(480, 480, 43, 0, 64)], clefs: vec![nt::Clef::Bass], symbols: vec![InSymbol { tick: 480, len: 0, staff: 0, kind: "lyric", text: "la".into() }] },
            ],
        };
        let undo = app.undo.len();
        import_doc(&mut app, doc, "file");
        let p = app.project.as_ref().unwrap();
        // the same-named track took the first part, a new track the second (below the others)
        assert_eq!(p.tracks.len(), n0 + 1);
        let cello = p.tracks.last().unwrap();
        assert_eq!(cello.name, "Cello Import");
        assert_eq!(cello.kind, lg_core::TrackKind::Instrument);
        assert_eq!(cello.regions[0].notes[0].pitch, 43);
        let ts = p.score.track(cello.extra.id).unwrap();
        assert_eq!(ts.style, "Bass");
        assert!(ts.symbols.iter().any(|s| s.kind == SymbolKind::Lyric && s.text == "la"));
        let named = p.tracks.iter().find(|t| t.name == first).unwrap();
        assert!(named.regions.iter().any(|r| r.notes.iter().any(|n| n.pitch == 60 && n.len == 960)));
        assert!(p.score.track(named.extra.id).unwrap().symbols.iter().any(|s| s.kind == SymbolKind::Dynamic));
        assert_eq!(app.undo.len(), undo + 1, "one undo step");
    }
}
