//! The Score Editor's key commands (Logic's "Score Editor" group) and its local-menu items:
//! views, colours, note attributes (stems, ties, beams, accidentals, enharmonic spelling,
//! interpretation, syncopation, independence, staff assignment), attached symbols, slurs and
//! hairpins, fingering, the Part box, staff styles and the score settings. Attribute edits are
//! one undo step each (they live in the project's lg-core score).

use lg_core::events::Note;
use lg_core::score::{Symbol, SymbolKind, ARTICULATIONS};
use lg_notation as nt;

use crate::app::App;
use crate::score::{self, Colors, Part, View};

/// The commands this file runs (build.rs and tool/checklist.py read the list).
pub const COMMANDS: &[&str] = &[
    "Linear Axis View", "Wrapped View", "Page View", "Change View", "Linear View",
    "Explode Folders", "Explode Polyphony", "Show/Hide Instrument Names", "Show Instrument Names", "Show/Hide Page Rulers", "Show Page Rulers",
    "Color View: individual", "Color View: Show Pitch", "Color View: Show Velocity", "Color View: Show Polyphony", "Color View: Force Black and White",
    "Normal", "Show Pitch", "Show Velocity", "Show Voice Assignment", "Force Black&White",
    "Toggle Duration Bar Display", "Toggle Guide Line Display", "Go to Page…",
    "Paste Multiple", "Paste Multiple at original Position",
    "Diatonic Insert", "Snap on/off", "Snap", "Next Staff", "Previous Staff", "Select All Regions and Events", "All Regions and Events",
    "Default Syncopation", "Force Syncopation", "Defeat Syncopation", "Default Interpretation", "Force Interpretation", "Defeat Interpretation",
    "Fix Displayed Note Positions", "Fix Displayed Note Positions and Durations", "Fix displayed Note Positions", "Fix displayed Note Positions and Durations",
    "Stems: default", "Stems: up", "Stems: down", "Stems: hide",
    "Stem Position: Default", "Stem Position: Center", "Stem Position: Automatic", "Stem Position: Side",
    "Stem End: Default Length", "Stem End: Move Up", "Stem End: Move Down",
    "Voice/Staff Assignment: default", "Voice/Staff Assignment: staff above voice", "Voice/Staff Assignment: staff below voice", "Default Staff", "Staff Above Voice", "Staff Below Voice",
    "Ties: default", "Ties: up", "Ties: down",
    "Beam Selected Notes", "Unbeam Selected Notes", "Default Beams", "Beam Selected", "Unbeam Selected",
    "Not Independent", "Independent", "Independent Grace",
    "Default Accidentals", "Default Accidental", "Enharmonic Shift: #", "Enharmonic Shift: b", "Flats to Sharps", "Sharps to Flats", "Force Accidental", "Hide Accidental", "Guide Accidental", "Force Accidental (only flats & sharps)",
    "Reset Note Attributes", "Reset All Attributes",
    "Assign MIDI Channels based on Score Split",
    "Nudge Position Up", "Nudge Position Down", "Nudge Position Left", "Nudge Position Right",
    "Settings: Global Format", "Settings: Numbers and Names", "Settings: Chords and Grids", "Settings: Clefs and Signatures", "Settings: MIDI Meaning", "Settings: Score Colors",
    "Global Format…", "Numbers and Names…", "Chords and Grids…", "Clefs and Signatures…", "MIDI Meaning…", "Colors…",
    "Create Score Set from Selection",
    "Part Box: 1/1 Note", "Part Box: 1/2 Note", "Part Box: 1/4 Note", "Part Box: 1/8 Note", "Part Box: 1/16 Note", "Part Box: 1/32 Note",
    "Next Part Box Symbol", "Previous Part Box Symbol", "Next Part Box Group", "Previous Part Box Group",
    "Attach Symbol: Fermata Up", "Attach Symbol: Fermata Down", "Attach Symbol: Staccato", "Attach Symbol: Staccatissimo", "Attach Symbol: Accent", "Attach Symbol: Strong Accent", "Attach Symbol: Long Accent", "Attach Symbol: Tenuto", "Attach Symbol: Up-Bow", "Attach Symbol: Down-Bow", "Attach Symbol: Flageolet", "Attach Symbol: Pizzicato", "Attach Symbol: Left Hand", "Attach Symbol: Right Hand", "Attach Symbol: Soft", "Attach Symbol: Heavy", "Attach Symbol: Jazz 1", "Attach Symbol: Jazz 2", "Attach Symbol: Jazz 3", "Attach Symbol: Jazz 4", "Attach Symbol: Jazz 5", "Attach Symbol: Jazz 6",
    "Insert: Slur Up", "Insert: Slur Down", "Insert: Crescendo", "Insert: Decrescendo",
    "Clear main finger", "Set main finger 1", "Set main finger 2", "Set main finger 3", "Set main finger 4", "Set main finger 5",
    "Clear pick finger", "Set pick finger P", "Set pick finger I", "Set pick finger M", "Set pick finger A", "Set pick finger C",
    "Convert Auto Slur", "Reset Auto Slur", "Slur Auto Direction", "Auto Slur Above", "Auto Slur Below", "Slur Above", "Slur Below", "Slur Last Note", "Slurs for non-contiguous selected notes",
    "Open Staff Style Window", "Show Staff Styles", "Duplicate Staff Style",
    "Open Score Editor", "Open Score Editor…", "Show/Hide Score Editor",
];

fn score_open(app: &App, ctx: &egui::Context) -> bool {
    app.bottom == crate::app::Bottom::PianoRoll && crate::score::editor_tab(app, ctx) == crate::score::EditorTab::Score
}

/// Runs a Score command; false when it isn't one (or the Score isn't there for it).
pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
    }
    // opening the Score works from anywhere; the rest act on the Score shown
    match name {
        "Open Score Editor" | "Open Score Editor…" => {
            app.bottom = crate::app::Bottom::PianoRoll;
            score::set_editor_tab(ctx, score::EditorTab::Score);
            return true;
        }
        "Show/Hide Score Editor" => {
            if score_open(app, ctx) {
                app.bottom = crate::app::Bottom::None;
            } else {
                app.bottom = crate::app::Bottom::PianoRoll;
                score::set_editor_tab(ctx, score::EditorTab::Score);
            }
            return true;
        }
        _ => {}
    }
    if !score_open(app, ctx) {
        return false;
    }
    let st = score::state(ctx);
    let view = |ctx: &egui::Context, f: &dyn Fn(&mut score::State)| score::with_state(ctx, |s| f(s));
    let attr = |app: &mut App, what: &str, f: &dyn Fn(&mut lg_core::score::NoteAttr)| score::edit_attrs(app, what, f);
    match name {
        // ---- View
        "Linear Axis View" | "Linear View" => view(ctx, &|s| s.view = View::Linear),
        "Wrapped View" => view(ctx, &|s| s.view = View::Wrapped),
        "Page View" => view(ctx, &|s| s.view = View::Page),
        "Change View" => view(ctx, &|s| {
            s.view = match s.view {
                View::Linear => View::Wrapped,
                View::Wrapped => View::Page,
                View::Page => View::Linear,
            }
        }),
        "Explode Folders" => view(ctx, &|s| s.explode_folders = !s.explode_folders),
        "Explode Polyphony" => view(ctx, &|s| s.explode_poly = !s.explode_poly),
        "Show/Hide Instrument Names" | "Show Instrument Names" => view(ctx, &|s| s.names = !s.names),
        "Show/Hide Page Rulers" | "Show Page Rulers" => view(ctx, &|s| s.rulers = !s.rulers),
        "Color View: individual" | "Normal" => view(ctx, &|s| s.colors = Colors::Normal),
        "Color View: Show Pitch" | "Show Pitch" => view(ctx, &|s| s.colors = Colors::Pitch),
        "Color View: Show Velocity" | "Show Velocity" => view(ctx, &|s| s.colors = Colors::Velocity),
        "Color View: Show Polyphony" | "Show Voice Assignment" => view(ctx, &|s| s.colors = Colors::Voice),
        "Color View: Force Black and White" | "Force Black&White" => view(ctx, &|s| s.colors = Colors::BlackWhite),
        "Toggle Duration Bar Display" => view(ctx, &|s| s.duration_bars = (s.duration_bars + 1) % 3),
        "Toggle Guide Line Display" => view(ctx, &|s| {
            // All Objects ↔ the last mode chosen
            if s.guide_mode == 3 {
                s.guide_mode = if s.guide_last == 3 { 1 } else { s.guide_last };
            } else {
                s.guide_last = s.guide_mode;
                s.guide_mode = 3;
            }
            s.guides = s.guide_mode != 0;
        }),
        "Go to Page…" => {
            if st.view != View::Page {
                return false;
            }
            view(ctx, &|s| s.goto = Some(String::new()));
        }
        "Diatonic Insert" => view(ctx, &|s| s.diatonic = !s.diatonic),
        "Snap on/off" | "Snap" => view(ctx, &|s| s.snap = !s.snap),
        "Settings: Global Format" | "Global Format…" => view(ctx, &|s| s.settings = Some(0)),
        "Settings: Numbers and Names" | "Numbers and Names…" => view(ctx, &|s| s.settings = Some(1)),
        "Settings: Chords and Grids" | "Chords and Grids…" => view(ctx, &|s| s.settings = Some(2)),
        "Settings: Clefs and Signatures" | "Clefs and Signatures…" => view(ctx, &|s| s.settings = Some(3)),
        "Settings: MIDI Meaning" | "MIDI Meaning…" => view(ctx, &|s| s.settings = Some(4)),
        "Settings: Score Colors" | "Colors…" => view(ctx, &|s| s.settings = Some(5)),
        "Open Staff Style Window" | "Show Staff Styles" => view(ctx, &|s| s.styles_window = !s.styles_window),
        "Duplicate Staff Style" => {
            // Studio's staff styles are fixed: the track keeps its style as its own (no longer
            // automatic), as a duplicate of the automatic one
            let Some(t) = app.selected_region.map(|x| x.0) else { return false };
            let s = score::style_of(app, t);
            score::set_style(app, t, s.name);
            app.tell(format!("Staff style “{}” set on the track", s.name));
        }
        "Create Score Set from Selection" => {
            let Some(p) = app.project.as_ref() else { return false };
            let mut ids: Vec<u64> = app.selected_regions.iter().chain(app.selected_region.iter()).filter_map(|&(t, _)| p.tracks.get(t)).map(|t| t.extra.id).collect();
            ids.dedup();
            if ids.is_empty() {
                return false;
            }
            // a score set of the project, named after its instruments (one undo step)
            let mut uniq: Vec<u64> = Vec::new();
            for id in ids {
                if !uniq.contains(&id) {
                    uniq.push(id);
                }
            }
            let name = uniq.iter().filter_map(|id| p.tracks.iter().find(|t| t.extra.id == *id)).map(|t| t.name.clone()).collect::<Vec<_>>().join(", ");
            let k = crate::scoreinsp::new_set(app, &name, &uniq);
            view(ctx, &|s| {
                s.set = Some(k);
                s.score_set = None;
            });
        }
        // ---- Part box
        _ if name.starts_with("Part Box: ") => {
            let label = name["Part Box: ".len()..].trim_end_matches(" Note");
            let Some(i) = score::VALUES.iter().position(|v| v.0 == label) else { return false };
            view(ctx, &|s| s.part = Part::Note(i));
        }
        "Next Part Box Symbol" | "Previous Part Box Symbol" | "Next Part Box Group" | "Previous Part Box Group" => {
            let (g, i) = st.part.index();
            let n = Part::GROUPS.len();
            let part = match name {
                "Next Part Box Symbol" => {
                    if i + 1 < Part::GROUPS[g] { Part::from_index(g, i + 1) } else { Part::from_index((g + 1) % n, 0) }
                }
                "Previous Part Box Symbol" => {
                    if i > 0 { Part::from_index(g, i - 1) } else { let pg = (g + n - 1) % n; Part::from_index(pg, Part::GROUPS[pg] - 1) }
                }
                "Next Part Box Group" => Part::from_index((g + 1) % n, 0),
                _ => Part::from_index((g + n - 1) % n, 0),
            };
            view(ctx, &|s| s.part = part);
        }
        // ---- attached symbols
        _ if name.starts_with("Attach Symbol: ") => {
            let Some(k) = ARTICULATIONS.iter().position(|a| *a == &name["Attach Symbol: ".len()..]) else { return false };
            let bit = 1u32 << k;
            return attr(app, "Attach Symbol", &|a| a.artic ^= bit);
        }
        "Insert: Slur Up" | "Insert: Slur Down" | "Insert: Crescendo" | "Insert: Decrescendo" => {
            let k = ["Insert: Slur Up", "Insert: Slur Down", "Insert: Crescendo", "Insert: Decrescendo"].iter().position(|x| *x == name).unwrap_or(0);
            if score::apply_part(app, &st, Part::Line(k)) {
                return true;
            }
            // nothing selected: at the playhead, half a bar long
            let Some(t) = app.selected_region.map(|x| x.0).or(app.selected_track) else { return false };
            let kind = [SymbolKind::SlurUp, SymbolKind::SlurDown, SymbolKind::Crescendo, SymbolKind::Decrescendo][k];
            let at = app.position() as u32;
            let bar = app.bar_ticks();
            score::add_symbol(app, t, Symbol { tick: at, kind, text: String::new(), len: bar / 2, staff: 0, ..Default::default() }, &name["Insert: ".len()..]);
        }
        // ---- note attributes
        "Stems: default" => return attr(app, "Stems", &|a| a.stem = 0),
        "Stems: up" => return attr(app, "Stems", &|a| a.stem = 1),
        "Stems: down" => return attr(app, "Stems", &|a| a.stem = -1),
        "Stems: hide" => return attr(app, "Stems", &|a| a.stem = 2),
        "Stem Position: Default" => return attr(app, "Stem Position", &|a| a.stem_pos = 0),
        "Stem Position: Center" => return attr(app, "Stem Position", &|a| a.stem_pos = 1),
        "Stem Position: Automatic" => return attr(app, "Stem Position", &|a| a.stem_pos = 2),
        "Stem Position: Side" => return attr(app, "Stem Position", &|a| a.stem_pos = 3),
        "Stem End: Default Length" => return attr(app, "Stem End", &|a| a.stem_end = 0),
        "Stem End: Move Up" => return attr(app, "Stem End", &|a| a.stem_end = (a.stem_end + 1).min(12)),
        "Stem End: Move Down" => return attr(app, "Stem End", &|a| a.stem_end = (a.stem_end - 1).max(-12)),
        "Voice/Staff Assignment: default" | "Default Staff" => return attr(app, "Voice/Staff Assignment", &|a| a.staff = 0),
        "Voice/Staff Assignment: staff above voice" | "Staff Above Voice" => return attr(app, "Voice/Staff Assignment", &|a| a.staff = 1),
        "Voice/Staff Assignment: staff below voice" | "Staff Below Voice" => return attr(app, "Voice/Staff Assignment", &|a| a.staff = -1),
        "Ties: default" => return attr(app, "Ties", &|a| a.tie = 0),
        "Ties: up" => return attr(app, "Ties", &|a| a.tie = 1),
        "Ties: down" => return attr(app, "Ties", &|a| a.tie = -1),
        "Beam Selected Notes" | "Beam Selected" => return attr(app, "Beam Selected Notes", &|a| a.beam = 1),
        "Unbeam Selected Notes" | "Unbeam Selected" => return attr(app, "Unbeam Selected Notes", &|a| a.beam = -1),
        "Default Beams" => return attr(app, "Default Beams", &|a| a.beam = 0),
        "Not Independent" => return attr(app, "Independent", &|a| a.independent = 0),
        "Independent" => return attr(app, "Independent", &|a| a.independent = 1),
        "Independent Grace" => return attr(app, "Independent", &|a| a.independent = 2),
        "Default Syncopation" => return attr(app, "Syncopation", &|a| a.synco = 0),
        "Force Syncopation" => return attr(app, "Syncopation", &|a| a.synco = 1),
        "Defeat Syncopation" => return attr(app, "Syncopation", &|a| a.synco = -1),
        "Default Interpretation" => return attr(app, "Interpretation", &|a| a.interp = 0),
        "Force Interpretation" => return attr(app, "Interpretation", &|a| a.interp = 1),
        "Defeat Interpretation" => return attr(app, "Interpretation", &|a| a.interp = -1),
        "Default Accidentals" | "Default Accidental" => return attr(app, "Accidentals", &|a| {
            a.accidental = 0;
            a.spell = 0;
        }),
        "Enharmonic Shift: #" => return attr(app, "Enharmonic Shift", &|a| a.spell = 1),
        "Enharmonic Shift: b" => return attr(app, "Enharmonic Shift", &|a| a.spell = -1),
        "Flats to Sharps" | "Sharps to Flats" => {
            // only the notes written with flats (or sharps) change
            let Some((t, r)) = app.selected_region else { return false };
            let key = app.project.as_ref().map_or(nt::Key::default(), |p| nt::Key::parse(&p.key));
            let style = score::style_of(app, t);
            let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return false };
            let id = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(0, |t| t.extra.id);
            let to_sharp = name == "Flats to Sharps";
            let targets: Vec<(u32, u8)> = app
                .selected_notes
                .iter()
                .filter_map(|&i| rg.notes.get(i))
                .filter(|n| {
                    let a = app.project.as_ref().map(|p| p.score.attr(id, n.pos, n.pitch)).unwrap_or_default();
                    let alter = style.written_key(key).spell_pref(style.written(n.pitch), a.spell).alter;
                    if to_sharp { alter < 0 } else { alter > 0 }
                })
                .map(|n| (n.pos, n.pitch))
                .collect();
            if targets.is_empty() {
                return true;
            }
            app.checkpoint(name);
            if let Some(p) = app.project.as_mut() {
                for (pos, pitch) in targets {
                    p.score.edit(id, pos, pitch, |a| a.spell = if to_sharp { 1 } else { -1 });
                }
            }
            app.after_edit();
        }
        "Force Accidental" => return attr(app, "Accidentals", &|a| a.accidental = 1),
        "Hide Accidental" => return attr(app, "Accidentals", &|a| a.accidental = 2),
        "Guide Accidental" => return attr(app, "Accidentals", &|a| a.accidental = 3),
        "Force Accidental (only flats & sharps)" => return attr(app, "Accidentals", &|a| a.accidental = 4),
        "Reset Note Attributes" | "Reset All Attributes" => return attr(app, "Reset Note Attributes", &|a| *a = Default::default()),
        "Clear main finger" => return attr(app, "Fingering", &|a| a.finger = 0),
        _ if name.starts_with("Set main finger ") => {
            let f: u8 = name[name.len() - 1..].parse().unwrap_or(0);
            return attr(app, "Fingering", &|a| a.finger = f);
        }
        "Clear pick finger" => return attr(app, "Fingering", &|a| a.pick = 0),
        _ if name.starts_with("Set pick finger ") => {
            let f = "PIMAC".find(&name[name.len() - 1..]).map_or(0, |k| k as u8 + 1);
            return attr(app, "Fingering", &|a| a.pick = f);
        }
        "Reset Auto Slur" => return attr(app, "Auto Slur", &|a| a.slur = 0),
        "Slur Auto Direction" => return attr(app, "Auto Slur", &|a| a.slur = 2),
        "Auto Slur Above" | "Slur Above" => return attr(app, "Auto Slur", &|a| a.slur = 1),
        "Auto Slur Below" | "Slur Below" => return attr(app, "Auto Slur", &|a| a.slur = -1),
        "Slur Last Note" => {
            // an automatic slur from the last selected note to the one after it
            let Some((id, keys)) = score::selected_keys(app) else { return false };
            let Some(&last) = keys.iter().max() else { return false };
            app.checkpoint("Slur Last Note");
            if let Some(p) = app.project.as_mut() {
                p.score.edit(id, last.0, last.1, |a| a.slur = 2);
            }
            app.after_edit();
        }
        "Convert Auto Slur" => {
            // the selected notes' automatic slurs become slur symbols of their own
            let Some((t, r)) = app.selected_region else { return false };
            let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return false };
            let Some((id, keys)) = score::selected_keys(app) else { return false };
            let auto: Vec<(u32, u8, i8)> = keys.iter().filter_map(|&(pos, pitch)| app.project.as_ref().map(|p| (pos, pitch, p.score.attr(id, pos, pitch).slur))).filter(|x| x.2 != 0).collect();
            if auto.is_empty() {
                return true;
            }
            app.checkpoint("Convert Auto Slur");
            if let Some(p) = app.project.as_mut() {
                for (pos, pitch, dir) in auto {
                    let next = rg.notes.iter().filter(|n| n.pos > pos).map(|n| n.pos).min().unwrap_or(pos + 480);
                    p.score.edit(id, pos, pitch, |a| a.slur = 0);
                    let kind = if dir == -1 { SymbolKind::SlurDown } else { SymbolKind::SlurUp };
                    p.score.track_mut(id).symbols.push(Symbol { tick: pos, kind, text: String::new(), len: next - pos, staff: 0, ..Default::default() });
                }
                p.score.track_mut(id).symbols.sort_by_key(|s| s.tick);
            }
            app.after_edit();
        }
        "Slurs for non-contiguous selected notes" => {
            // one slur over each run of selected notes that follow each other in the region
            let Some((t, r)) = app.selected_region else { return false };
            let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return false };
            let id = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(0, |t| t.extra.id);
            let runs = selected_runs(&rg.notes, &app.selected_notes);
            if runs.is_empty() {
                return true;
            }
            app.checkpoint("Insert Slurs");
            if let Some(p) = app.project.as_mut() {
                let ts = p.score.track_mut(id);
                for (a, b) in runs {
                    ts.symbols.push(Symbol { tick: a, kind: SymbolKind::SlurUp, text: String::new(), len: b - a, staff: 0, ..Default::default() });
                }
                ts.symbols.sort_by_key(|s| s.tick);
            }
            app.after_edit();
        }
        // ---- notes
        "Fix Displayed Note Positions" | "Fix Displayed Note Positions and Durations" | "Fix displayed Note Positions" | "Fix displayed Note Positions and Durations" => {
            let durations = name.ends_with("Durations");
            let Some((t, r)) = app.selected_region else { return false };
            let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return false };
            let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
            let (q, _, _) = quant(app, t);
            let notes: Vec<nt::NoteIn> = rg.notes.iter().enumerate().map(|(i, n)| nt::NoteIn::new(i, n.pitch, n.pos, n.len)).collect();
            let fixed = nt::display_positions(&notes, sig, nt::Quant::CHOICES[q].1, rg.start / lg_core::project::bar_ticks(sig) * lg_core::project::bar_ticks(sig));
            let sel: Vec<usize> = if app.selected_notes.is_empty() { (0..rg.notes.len()).collect() } else { app.selected_notes.clone() };
            app.checkpoint(name);
            let id = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(0, |t| t.extra.id);
            if let Some(p) = app.project.as_mut() {
                let mut moves = Vec::new();
                if let Some(region) = p.tracks.get_mut(t).and_then(|tr| tr.regions.get_mut(r)) {
                    for (i, on, off) in fixed {
                        if !sel.contains(&i) {
                            continue;
                        }
                        if let Some(n) = region.notes.get_mut(i) {
                            moves.push(((n.pos, n.pitch), (on, n.pitch)));
                            n.pos = on;
                            if durations {
                                n.len = off.saturating_sub(on).max(1);
                            }
                        }
                    }
                }
                for (a, b) in moves {
                    p.score.moved(id, a, b);
                }
            }
            app.after_edit();
        }
        "Assign MIDI Channels based on Score Split" => {
            // the upper staff (or voice) channel 1, the lower channel 2
            let Some((t, r)) = app.selected_region else { return false };
            let style = score::style_of(app, t);
            let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return false };
            let notes: Vec<nt::NoteIn> = rg.notes.iter().enumerate().map(|(i, n)| nt::NoteIn::new(i, n.pitch, n.pos, n.len)).collect();
            let voices = nt::split_voices(&notes);
            app.checkpoint("Assign MIDI Channels");
            if let Some(region) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                for (i, n) in region.notes.iter_mut().enumerate() {
                    n.channel = if style.clefs.len() > 1 { style.staff_of(n.pitch) as u8 } else { voices[i] };
                }
            }
            app.after_edit();
        }
        "Nudge Position Left" | "Nudge Position Right" => {
            let d = app.nudge.max(1) as i64 * if name.ends_with("Left") { -1 } else { 1 };
            return move_selected(app, name, d, 0);
        }
        "Nudge Position Up" | "Nudge Position Down" => {
            return move_selected(app, name, 0, if name.ends_with("Up") { 1 } else { -1 });
        }
        "Next Staff" | "Previous Staff" => {
            // the region on the next (previous) shown staff at the playhead, its first note
            let Some(p) = app.project.as_ref() else { return false };
            let at = app.position() as u32;
            let cur = app.selected_region.map_or(0, |x| x.0);
            let midi: Vec<usize> = p.tracks.iter().enumerate().filter(|(_, t)| t.kind != lg_core::TrackKind::Audio && t.regions.iter().any(|r| r.audio.is_none())).map(|(i, _)| i).collect();
            if midi.is_empty() {
                return false;
            }
            let k = midi.iter().position(|&t| t == cur).unwrap_or(0);
            let next = if name == "Next Staff" { midi[(k + 1) % midi.len()] } else { midi[(k + midi.len() - 1) % midi.len()] };
            let regions = &p.tracks[next].regions;
            let rix = regions.iter().position(|r| r.audio.is_none() && at >= r.start && at < r.start + r.length).or_else(|| regions.iter().position(|r| r.audio.is_none()));
            if let Some(rix) = rix {
                let first = regions[rix].notes.iter().position(|n| n.pos >= at);
                app.selected_track = Some(next);
                app.selected_region = Some((next, rix));
                app.selected_notes = first.into_iter().collect();
            }
        }
        "Select All Regions and Events" | "All Regions and Events" => {
            let Some(p) = app.project.as_ref() else { return false };
            let tracks: Vec<usize> = if st.explode_folders { (0..p.tracks.len()).collect() } else { app.selected_region.map(|x| vec![x.0]).unwrap_or_default() };
            let all: Vec<(usize, usize)> = tracks.iter().flat_map(|&t| p.tracks[t].regions.iter().enumerate().filter(|(_, r)| r.audio.is_none()).map(move |(r, _)| (t, r))).collect();
            app.selected_regions = all;
            if let Some((t, r)) = app.selected_region {
                app.selected_notes = (0..p.tracks[t].regions[r].notes.len()).collect();
            }
        }
        "Paste Multiple" | "Paste Multiple at original Position" => {
            // the clipboard pasted at each selected note's position (or, at original position,
            // into each selected region)
            let clip = app.midi.clipboard.clone();
            if clip.is_empty() {
                return true;
            }
            let first = clip.iter().map(|n| n.pos).min().unwrap_or(0);
            let Some((t, r)) = app.selected_region else { return false };
            let targets: Vec<(usize, usize, i64)> = if name == "Paste Multiple" {
                let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)) else { return false };
                let mut at: Vec<u32> = app.selected_notes.iter().filter_map(|&i| rg.notes.get(i)).map(|n| n.pos).collect();
                if at.is_empty() {
                    at.push(app.position() as u32);
                }
                at.sort_unstable();
                at.dedup();
                at.into_iter().map(|a| (t, r, a as i64 - first as i64)).collect()
            } else {
                let mut v: Vec<(usize, usize)> = app.selected_regions.clone();
                if !v.contains(&(t, r)) {
                    v.push((t, r));
                }
                v.into_iter().map(|(a, b)| (a, b, 0)).collect()
            };
            app.checkpoint(name);
            if let Some(p) = app.project.as_mut() {
                for (tt, rr, d) in targets {
                    if let Some(rg) = p.tracks.get_mut(tt).and_then(|x| x.regions.get_mut(rr)) {
                        for n in &clip {
                            rg.notes.push(Note { pos: (n.pos as i64 + d).max(0) as u32, ..*n });
                        }
                        rg.notes.sort_by_key(|n| (n.pos, n.pitch));
                        rg.notes.dedup_by(|a, b| a.pos == b.pos && a.pitch == b.pitch);
                    }
                }
            }
            app.selected_notes.clear();
            app.after_edit();
        }
        _ => return false,
    }
    true
}

fn quant(app: &App, t: usize) -> (usize, bool, bool) {
    let id = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(0, |t| t.extra.id);
    let ts = app.project.as_ref().and_then(|p| p.score.track(id));
    (ts.and_then(|t| nt::Quant::CHOICES.iter().position(|c| c.0 == t.quantize)).unwrap_or(9), true, false)
}

/// Moves the selected notes by ticks and diatonic steps (Nudge Position), keeping their score
/// attributes.
fn move_selected(app: &mut App, what: &str, dt: i64, steps: i32) -> bool {
    let Some((t, r)) = app.selected_region else { return false };
    if app.selected_notes.is_empty() {
        return false;
    }
    let key = app.project.as_ref().map_or(nt::Key::default(), |p| nt::Key::parse(&p.key));
    let style = score::style_of(app, t);
    let id = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(0, |t| t.extra.id);
    let sel = app.selected_notes.clone();
    app.checkpoint(what);
    let mut moves = Vec::new();
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        for &i in &sel {
            if let Some(n) = rg.notes.get_mut(i) {
                let from = (n.pos, n.pitch);
                n.pos = (n.pos as i64 + dt).max(0) as u32;
                if steps != 0 {
                    n.pitch = if style.clefs[0] == nt::Clef::Percussion {
                        (n.pitch as i32 + steps).clamp(0, 127) as u8
                    } else {
                        let wk = style.written_key(key);
                        (nt::step_pitch(wk.spell(style.written(n.pitch)).step + steps, wk) as i32 - style.transpose as i32).clamp(0, 127) as u8
                    };
                }
                moves.push((from, (n.pos, n.pitch)));
            }
        }
    }
    if let Some(p) = app.project.as_mut() {
        for (a, b) in moves {
            p.score.moved(id, a, b);
        }
    }
    app.after_edit();
    true
}

/// Runs of selected notes that follow each other in the region (by position): (start, end).
pub fn selected_runs(notes: &[Note], sel: &[usize]) -> Vec<(u32, u32)> {
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by_key(|&i| (notes[i].pos, notes[i].pitch));
    // one entry per onset (chords count once)
    let mut onsets: Vec<(u32, bool)> = Vec::new();
    for &i in &order {
        let s = sel.contains(&i);
        match onsets.last_mut() {
            Some(l) if l.0 == notes[i].pos => l.1 |= s,
            _ => onsets.push((notes[i].pos, s)),
        }
    }
    let mut out = Vec::new();
    let mut k = 0;
    while k < onsets.len() {
        if onsets[k].1 {
            let a = onsets[k].0;
            let mut j = k;
            while j + 1 < onsets.len() && onsets[j + 1].1 {
                j += 1;
            }
            if j > k {
                out.push((a, onsets[j].0));
            }
            k = j + 1;
        } else {
            k += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(pos: u32, pitch: u8) -> Note {
        Note { channel: 0, pitch, velocity: 80, pos, len: 240, muted: false }
    }

    #[test]
    fn slur_runs() {
        let notes = [n(0, 60), n(240, 62), n(480, 64), n(720, 65), n(960, 67), n(1200, 69)];
        // 0,1,2 then 4,5: two slurs; a lone note gets none
        assert_eq!(selected_runs(&notes, &[0, 1, 2, 4, 5]), [(0, 480), (960, 1200)]);
        assert!(selected_runs(&notes, &[3]).is_empty());
    }

    #[test]
    fn commands_are_logic_names() {
        // every Score Editor key command of Logic's that Studio runs is in the research list
        let data = include_str!("../data/commands.txt");
        let score: Vec<&str> = data.lines().filter_map(|l| l.strip_prefix("Score Editor\t")).map(|l| l.split('\t').next().unwrap_or("")).collect();
        let ours = score.iter().filter(|c| COMMANDS.contains(c)).count();
        assert!(ours >= 130, "{ours}");
    }
}
