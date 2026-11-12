//! File ▸ Export ▸ Score as MusicXML…: the score shown (the Score Editor's tracks, else every
//! MIDI track) as a MusicXML file, written by lg-notation from the same engraving the editor
//! draws, with the score's dynamics, text, lyrics and bar lines and repeats.

use lg_core::score::{BarKind, SymbolKind};
use lg_notation as nt;
use lg_notation::musicxml::{self, Barline};

use crate::app::App;

pub const COMMANDS: &[&str] = &["Score as MusicXML…", "Export Score as MusicXML…"];

pub fn command(app: &mut App, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
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
