//! The Score editor (Logic Pro 13's Score tab, linear view): the selected track's MIDI regions
//! as notation on a treble, bass or grand staff chosen by range, in the project's key and time
//! signature. Notes are quantised for display (the region's display Quantize, Logic's 16,24 by
//! default), with rests filled in, ties across beats and bars, chords on one stem, beams and
//! flags, ledger lines and accidentals by key (engraving in lg-notation).
//!
//! Editing as Logic's: a click selects a note (Shift adds), a drag moves it diatonically up and
//! down and in time on the display grid, a double-click on a staff adds a note of the Part
//! box's value, Delete removes the selection (the app's Delete, as in the Piano Roll). The Part
//! box sits as a strip under the local menus: note values, rests and dynamics.
//!
//! Music symbols are drawn with shapes (no music font is bundled).

use egui::epaint::{CubicBezierShape, PathShape, PathStroke};
use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Shape, Stroke};
use lg_core::events::Note;
use lg_notation as nt;

use crate::app::App;
use crate::look;

const TABS_H: f32 = 40.0;
const LOCAL_H: f32 = 28.0;
const PART_H: f32 = 34.0;
const RULER_H: f32 = 26.0;
const HEAD_W: f32 = 120.0;

/// Which editor the editor area shows (Logic's tabs; pattern regions always open the Step
/// Sequencer).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorTab {
    #[default]
    PianoRoll,
    Score,
    StepSequencer,
}

fn tab_id() -> egui::Id {
    egui::Id::unique("lg-editor-tab")
}

/// The editor to show for the selected region.
pub fn editor_tab(app: &App, ctx: &egui::Context) -> EditorTab {
    if let Some((t, r)) = app.selected_region {
        if crate::stepseq::is_pattern(ctx, t, r) {
            return EditorTab::StepSequencer;
        }
    }
    match ctx.data(|d| d.get_temp::<EditorTab>(tab_id())).unwrap_or_default() {
        EditorTab::StepSequencer => EditorTab::PianoRoll,
        t => t,
    }
}

pub fn set_editor_tab(ctx: &egui::Context, tab: EditorTab) {
    ctx.data_mut(|d| d.insert_temp(tab_id(), tab));
}

/// Logic's editor tabs, Piano Roll | Score | Smart Tempo, in one capsule; a click switches.
pub fn tabs(app: &mut App, ui: &mut egui::Ui, area: Rect, current: EditorTab) {
    let p = ui.painter_at(area);
    let names = ["Piano Roll", "Score", "Smart Tempo"];
    let tw = 150.0;
    let tx0 = area.center().x - tw * 1.5;
    let pill = Rect::from_min_size(pos2(tx0, area.min.y + 7.0), vec2(tw * 3.0, 26.0));
    p.rect_filled(pill, 8.0, look::group_bg());
    let on = match current {
        EditorTab::Score => 1,
        _ => 0,
    };
    for (i, n) in names.iter().enumerate() {
        let r = Rect::from_min_size(pos2(tx0 + i as f32 * tw, area.min.y + 7.0), vec2(tw, 26.0));
        if i == on {
            p.rect_filled(r.shrink(2.0), 7.0, look::accent());
        }
        p.text(r.center(), Align2::CENTER_CENTER, *n, look::regular(13.0), if i == on { Color32::WHITE } else { look::text() });
        if i != on && ui.interact(r, ui.scope_id().with("lg-sc").with(("edtab", i)), Sense::click()).clicked() {
            match i {
                0 => set_editor_tab(ui.ctx(), EditorTab::PianoRoll),
                1 => set_editor_tab(ui.ctx(), EditorTab::Score),
                _ => app.tell(format!("The {n} editor isn't available yet")),
            }
        }
    }
}

/// The Score's own settings (kept in egui's memory: no App field needed).
#[derive(Clone, Debug)]
struct State {
    /// Part box: the note value inserted (index into VALUES), dotted, the dynamic (index).
    value: usize,
    dotted: bool,
    dynamic: Option<usize>,
    /// Display Quantize (index into nt::Quant::CHOICES).
    quant: usize,
    /// Staff space in points, bar width, scroll (bars, points).
    space: f32,
    bar_w: f32,
    scroll_x: f32,
    scroll_y: f32,
    drag: Option<DragState>,
}

impl Default for State {
    fn default() -> Self {
        State { value: 2, dotted: false, dynamic: None, quant: 9, space: 8.0, bar_w: 300.0, scroll_x: 0.0, scroll_y: 0.0, drag: None }
    }
}

#[derive(Clone, Debug)]
struct DragState {
    region: (usize, usize),
    orig: Vec<Note>,
    from: Pos2,
    moved: bool,
}

fn state_id() -> egui::Id {
    egui::Id::unique("lg-score-state")
}

/// Part box note values: (label, base, key command).
const VALUES: [(&str, u8); 7] = [("1/1", 1), ("1/2", 2), ("1/4", 4), ("1/8", 8), ("1/16", 16), ("1/32", 32), ("1/64", 64)];
/// Dynamics and the velocity each stands for (Logic's MIDI Meaning defaults).
const DYNAMICS: [(&str, u8); 8] = [("ppp", 16), ("pp", 32), ("p", 48), ("mp", 64), ("mf", 80), ("f", 96), ("ff", 112), ("fff", 127)];

/// Paper and ink (Logic's Score is light in dark mode too).
const PAPER: Color32 = Color32::from_rgb(0xbf, 0xbf, 0xb8);
const REGION_PAPER: Color32 = Color32::from_rgb(0xfe, 0xff, 0xf3);
const INK: Color32 = Color32::from_rgb(0x10, 0x10, 0x10);

/// A staff on screen: clef, its bottom line's y, and which notes it shows.
#[derive(Clone, Copy)]
struct Staff {
    clef: nt::Clef,
    bottom: f32,
    /// Grand staff: true for the upper (notes from middle C up), false for the lower.
    part: Option<bool>,
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let ctx = ui.ctx().clone();
    let mut st: State = ctx.data(|d| d.get_temp(state_id())).unwrap_or_default();
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let tabs_r = Rect::from_min_size(area.min, vec2(area.width(), TABS_H));
    tabs(app, ui, tabs_r, EditorTab::Score);
    let Some((ti, ri)) = app.selected_region else {
        p.text(area.center(), Align2::CENTER_CENTER, "Select a MIDI region to see its score", look::regular(14.0), look::text_dim());
        return;
    };
    let Some(track) = app.project.as_ref().and_then(|pr| pr.tracks.get(ti)).cloned() else { return };
    if track.regions.get(ri).is_some_and(|r| r.audio.is_some()) {
        p.text(area.center(), Align2::CENTER_CENTER, "An audio region has no score", look::regular(14.0), look::text_dim());
        return;
    }
    let (sig, key_text) = app.project.as_ref().map(|p| (p.signature, p.key.clone())).unwrap_or(((4, 4), "C major".into()));
    let key = nt::Key::parse(&key_text);
    let bar_t = lg_core::project::bar_ticks(sig) as f32;

    // the local menus: Layout / Edit / Functions / View, and the display Quantize
    let local = Rect::from_min_size(pos2(area.min.x, tabs_r.max.y), vec2(area.width(), LOCAL_H));
    p.rect_filled(local, 0.0, look::header_bg());
    ui.scope_builder(egui::UiBuilder::new().max_rect(local.shrink2(vec2(10.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        crate::menus::local_bar(app, ui, &[("Layout", "score-Layout"), ("Edit", "score-Edit"), ("Functions", "score-Functions"), ("View", "score-View")]);
        ui.label(egui::RichText::new("Quantize:").color(look::text_dim()).size(12.0));
        let names: Vec<&str> = nt::Quant::CHOICES.iter().map(|q| q.0).collect();
        let r = Rect::from_min_size(ui.cursor().min + vec2(4.0, 1.0), vec2(70.0, 22.0));
        if let Some(k) = crate::widgets::choice_slot(ui, r, ui.scope_id().with("sc-quant"), st.quant, &names) {
            st.quant = k;
        }
    });
    // the Part box strip
    let part = Rect::from_min_size(pos2(area.min.x, local.max.y), vec2(area.width(), PART_H));
    part_box(ui, part, &mut st, app);
    let ruler = Rect::from_min_max(pos2(area.min.x + HEAD_W, part.max.y), pos2(area.max.x, part.max.y + RULER_H));
    let body = Rect::from_min_max(pos2(area.min.x, ruler.max.y), area.max);
    let head = Rect::from_min_max(body.min, pos2(area.min.x + HEAD_W, body.max.y));
    let music = Rect::from_min_max(pos2(head.max.x, body.min.y), body.max);

    // scrolling and zoom (⌘ + wheel or a pinch zooms the staff, Shift + wheel scrolls in time)
    if ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| body.contains(h) || ruler.contains(h)) {
        let (scroll, zoom, cmd, shift) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.command, i.modifiers.shift));
        if cmd && scroll.y != 0.0 {
            let f = 1.0 + scroll.y * 0.003;
            st.space = (st.space * f).clamp(4.0, 24.0);
            st.bar_w = (st.bar_w * f).clamp(80.0, 2400.0);
        } else if (zoom - 1.0).abs() > 0.001 {
            st.space = (st.space * zoom).clamp(4.0, 24.0);
            st.bar_w = (st.bar_w * zoom).clamp(80.0, 2400.0);
        } else if shift {
            st.scroll_x = (st.scroll_x - scroll.y / st.bar_w).max(0.0);
        } else {
            st.scroll_x = (st.scroll_x - scroll.x / st.bar_w).max(0.0);
            st.scroll_y = (st.scroll_y - scroll.y).clamp(-400.0, 1200.0);
        }
    }
    // the region in view when it isn't
    let sel_region = &track.regions[ri];
    let first_bar = (sel_region.start as f32 / bar_t).floor();
    if first_bar < st.scroll_x || first_bar > st.scroll_x + music.width() / st.bar_w - 1.0 {
        if !ui.ctx().data(|d| d.get_temp::<(usize, usize)>(egui::Id::unique("lg-score-seen"))).is_some_and(|x| x == (ti, ri)) {
            st.scroll_x = (first_bar - 0.15).max(0.0);
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("lg-score-seen"), (ti, ri)));

    let sp = st.space;
    let (sx, bw) = (st.scroll_x, st.bar_w);
    let x_of = move |tick: f32| music.min.x + (tick / bar_t - sx) * bw;
    let tick_of = move |x: f32| ((x - music.min.x) / bw + sx) * bar_t;

    // which staves: by the whole track's range (piano parts get the grand staff)
    let pitches: Vec<u8> = track.regions.iter().flat_map(|r| r.notes.iter().map(|n| n.pitch)).collect();
    let lname = track.name.to_lowercase();
    let piano = ["piano", "keys", "grand", "steinway", "rhodes", "clav", "organ", "harpsichord"].iter().any(|w| lname.contains(w));
    let style = nt::staves_for(&pitches, piano);
    // the staves centred in the editor when they fit (the wheel scrolls them)
    let block_h = if style == nt::Staves::Piano { 14.0 * sp } else { 4.0 * sp } + 10.0 * sp;
    let top = body.min.y + 5.0 * sp + ((body.height() - block_h) / 2.0).max(0.0) - st.scroll_y;
    let staves: Vec<Staff> = match style {
        nt::Staves::Treble => vec![Staff { clef: nt::Clef::Treble, bottom: top + 4.0 * sp, part: None }],
        nt::Staves::Bass => vec![Staff { clef: nt::Clef::Bass, bottom: top + 4.0 * sp, part: None }],
        nt::Staves::Piano => vec![
            Staff { clef: nt::Clef::Treble, bottom: top + 4.0 * sp, part: Some(true) },
            Staff { clef: nt::Clef::Bass, bottom: top + 4.0 * sp + 10.0 * sp, part: Some(false) },
        ],
    };
    let block_top = top - 5.0 * sp;
    let block_bottom = staves.last().map_or(top, |s| s.bottom) + 5.0 * sp;

    // the ruler
    let rp = ui.painter_at(ruler);
    rp.rect_filled(ruler, 0.0, look::header_bg());
    rp.hline(ruler.x_range(), ruler.max.y - 0.5, Stroke::new(1.0, look::line()));
    let beats = sig.0.max(1) as i32;
    let first = sx.floor() as i32;
    for b in first..first + (music.width() / bw).ceil() as i32 + 1 {
        for k in 0..beats {
            let x = x_of(b as f32 * bar_t + k as f32 * bar_t / beats as f32);
            if k == 0 {
                rp.vline(x, ruler.y_range(), Stroke::new(1.0, look::text_dim()));
                rp.text(pos2(x + 4.0, ruler.min.y + 8.0), Align2::LEFT_CENTER, format!("{}", b + 1), look::semibold(12.0), look::text());
            } else {
                rp.vline(x, ruler.max.y - 6.0..=ruler.max.y, Stroke::new(1.0, look::text_dim()));
            }
        }
    }
    let hp = ui.painter_at(Rect::from_min_max(pos2(area.min.x, part.max.y), pos2(head.max.x, ruler.max.y)));
    hp.rect_filled(Rect::from_min_max(pos2(area.min.x, part.max.y), pos2(head.max.x, ruler.max.y)), 0.0, look::header_bg());

    // paper
    let mp = ui.painter_at(music);
    let headp = ui.painter_at(head);
    mp.rect_filled(music, 0.0, PAPER);
    headp.rect_filled(head, 0.0, PAPER);
    p.vline(head.max.x, body.y_range(), Stroke::new(1.0, Color32::from_gray(0x30)));

    // the background: clicks, double-clicks (insert), the right-click menu
    let bg = ui.interact(music, ui.scope_id().with("sc-bg"), Sense::click());
    crate::menus::context_menu(app, &bg, "score-background");

    let quant = nt::Quant::CHOICES[st.quant].1;
    let sel_color = look::mix(look::region_color(track.kind), Color32::BLACK, 0.12);
    let mut pressed: Option<(usize, usize, bool)> = None;
    for (rix, region) in track.regions.iter().enumerate() {
        if region.audio.is_some() {
            continue;
        }
        let from = (region.start as f32 / bar_t).floor() as u32 * bar_t as u32;
        let to = ((region.start + region.length) as f32 / bar_t).ceil().max(1.0) as u32 * bar_t as u32;
        let to = to.max(from + bar_t as u32);
        let (rx0, rx1) = (x_of(region.start as f32), x_of((region.start + region.length) as f32));
        if rx1 < music.min.x || rx0 > music.max.x {
            continue;
        }
        // the region's paper (Logic: a cream box under the staves)
        mp.rect_filled(Rect::from_min_max(pos2(rx0, block_top), pos2(rx1, block_bottom)), 4.0, REGION_PAPER);
        let opts = nt::Options { sig, key, quant, from, to };
        let notes: Vec<nt::NoteIn> = region.notes.iter().enumerate().map(|(i, n)| nt::NoteIn { index: i, pitch: n.pitch, pos: n.pos, len: n.len }).collect();
        for staff in &staves {
            let mine: Vec<nt::NoteIn> = notes.iter().copied().filter(|n| staff.part.is_none_or(|upper| (n.pitch >= 60) == upper)).collect();
            let voice = nt::engrave(&mine, staff.clef, &opts);
            let selected = |i: usize| app.selected_region == Some((ti, rix)) && app.selected_notes.contains(&i);
            draw_voice(&mp, &voice, staff, sp, &x_of, &selected, sel_color);
            // the notes' heads: click, drag (ids by note, so a head keeps its drag as it moves)
            let mut pieces: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
            for it in voice.items.iter() {
                for h in it.heads.iter() {
                    let piece = pieces.entry(h.note).or_default();
                    *piece += 1;
                    let c = head_center(it, h, staff, sp, x_of(it.tick as f32));
                    let r = Rect::from_center_size(c, vec2(sp * 1.6, sp * 1.2));
                    if !r.intersects(music) {
                        continue;
                    }
                    let resp = ui.interact(r.intersect(music), ui.scope_id().with("lg-sc").with(("sc-head", rix, h.note, *piece)), Sense::click_and_drag());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() || resp.drag_started() {
                        pressed = Some((rix, h.note, resp.drag_started()));
                    }
                    if resp.secondary_clicked() {
                        app.selected_region = Some((ti, rix));
                        if !app.selected_notes.contains(&h.note) {
                            app.selected_notes = vec![h.note];
                        }
                    }
                }
            }
        }
    }
    // staves: lines through the music area and the header (clef, key, time signature)
    for staff in &staves {
        for l in 0..5 {
            let y = staff.bottom - l as f32 * sp;
            mp.hline(music.x_range(), y, Stroke::new(1.0, INK));
            headp.hline(head.min.x + 8.0..=head.max.x, y, Stroke::new(1.0, INK));
        }
        let mut x = head.min.x + 14.0;
        match staff.clef {
            nt::Clef::Treble => treble_clef(&headp, pos2(x + 1.3 * sp, staff.bottom), sp, INK),
            nt::Clef::Bass => bass_clef(&headp, pos2(x + 0.3 * sp, staff.bottom), sp, INK),
        }
        x += 3.4 * sp;
        for (pos, acc) in key.signature_positions(staff.clef) {
            accidental(&headp, pos2(x, staff.bottom - pos as f32 * sp / 2.0), sp, acc, INK);
            x += 1.1 * sp;
        }
        x += 0.8 * sp;
        let f = look::bold(sp * 2.6);
        headp.text(pos2(x + sp, staff.bottom - 3.0 * sp), Align2::CENTER_CENTER, sig.0.to_string(), f.clone(), INK);
        headp.text(pos2(x + sp, staff.bottom - 1.0 * sp), Align2::CENTER_CENTER, sig.1.to_string(), f, INK);
        // bar lines
        for b in first..first + (music.width() / bw).ceil() as i32 + 2 {
            let x = x_of(b as f32 * bar_t);
            if b > 0 && track.regions.iter().any(|r| r.audio.is_none() && (r.start as f32) < b as f32 * bar_t + 1.0 && (r.start + r.length) as f32 > (b - 1) as f32 * bar_t) {
                mp.vline(x, staff.bottom - 4.0 * sp..=staff.bottom, Stroke::new(1.0, INK));
            }
        }
    }
    // the grand staff's brace line
    if staves.len() == 2 {
        let x = head.min.x + 8.0;
        headp.vline(x, staves[0].bottom - 4.0 * sp..=staves[1].bottom, Stroke::new(1.5, INK));
        brace(&headp, x - 3.0, staves[0].bottom - 4.0 * sp, staves[1].bottom, INK);
    }

    // a press: select (Shift toggles), start a move
    if let Some((rix, note, drag)) = pressed {
        let shift = ui.input(|i| i.modifiers.shift);
        if app.selected_region != Some((ti, rix)) {
            app.selected_region = Some((ti, rix));
            app.selected_notes.clear();
        }
        if shift && !drag {
            if let Some(i) = app.selected_notes.iter().position(|x| *x == note) {
                app.selected_notes.remove(i);
            } else {
                app.selected_notes.push(note);
            }
        } else if !app.selected_notes.contains(&note) {
            if shift {
                app.selected_notes.push(note);
            } else {
                app.selected_notes = vec![note];
            }
        }
        if drag {
            if let (Some(pp), Some(orig)) = (ui.input(|i| i.pointer.press_origin()), app.project.as_ref().and_then(|p| p.tracks.get(ti)?.regions.get(rix)).map(|r| r.notes.clone())) {
                st.drag = Some(DragState { region: (ti, rix), orig, from: pp, moved: false });
            }
        }
        if !drag {
            if let Some(n) = app.project.as_ref().and_then(|p| p.tracks.get(ti)?.regions.get(rix)?.notes.get(note)).copied() {
                crate::pianoroll::preview(app, ti, n.pitch);
            }
        }
    }
    // a drag: diatonic steps up and down, time on the display grid
    if let Some(d) = st.drag.clone() {
        let (pointer, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        if let (Some(pp), true) = (pointer, down) {
            let dy = pp.y - d.from.y;
            let steps = -(dy / (sp / 2.0)).round() as i32;
            let grid = quant.straight as f32;
            let dt = ((tick_of(pp.x) - tick_of(d.from.x)) / grid).round() * grid;
            if (steps != 0 || dt != 0.0) && !d.moved {
                app.checkpoint("Move Notes");
                if let Some(x) = st.drag.as_mut() {
                    x.moved = true;
                }
            }
            if steps != 0 || dt != 0.0 || d.moved {
                let sel = app.selected_notes.clone();
                if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(d.region.0)?.regions.get_mut(d.region.1)) {
                    for &i in &sel {
                        if let (Some(o), Some(n)) = (d.orig.get(i), rg.notes.get_mut(i)) {
                            let step = key.spell(o.pitch).step + steps;
                            n.pitch = nt::step_pitch(step, key);
                            n.pos = (o.pos as f32 + dt).max(0.0) as u32;
                        }
                    }
                    if let Some(n) = sel.first().and_then(|&i| rg.notes.get(i)) {
                        app.help_tag = Some(vec![("Position:".into(), crate::app::bbdt(n.pos, sig)), ("Length:".into(), crate::app::bbdt_len(n.len, sig)), ("Pitch:".into(), crate::app::note_name(n.pitch))]);
                    }
                }
            }
        }
        if !down {
            if st.drag.as_ref().is_some_and(|d| d.moved) {
                app.after_edit();
            }
            st.drag = None;
        }
    }
    if pressed.is_none() {
        if bg.double_clicked() {
            if let Some(pp) = bg.interact_pointer_pos() {
                insert_note(app, &st, pp, &staves, &track, ti, key, tick_of(pp.x), quant, sp);
            }
        } else if bg.clicked() {
            app.selected_notes.clear();
        }
    }
    // the playhead
    let ph = x_of(app.position() as f32);
    if music.x_range().contains(ph) {
        mp.vline(ph, music.y_range(), Stroke::new(1.2, Color32::from_gray(0x30)));
    }
    ctx.data_mut(|d| d.insert_temp(state_id(), st));
}

/// Double-click on a staff: a note of the Part box's value at the line or space clicked, on the
/// display grid, inside the region under the pointer (or the selected one).
#[allow(clippy::too_many_arguments)]
fn insert_note(app: &mut App, st: &State, pp: Pos2, staves: &[Staff], track: &lg_core::Track, ti: usize, key: nt::Key, tick: f32, quant: nt::Quant, sp: f32) {
    let Some(staff) = staves.iter().min_by(|a, b| (a.bottom - 2.0 * sp - pp.y).abs().total_cmp(&(b.bottom - 2.0 * sp - pp.y).abs())) else { return };
    let pos = ((staff.bottom - pp.y) / (sp / 2.0)).round() as i32;
    let pitch = nt::pitch_at(staff.clef, pos, key);
    let grid = quant.straight as f32;
    let tick = ((tick / grid).floor() * grid).max(0.0) as u32;
    let rix = track.regions.iter().position(|r| r.audio.is_none() && tick >= r.start && tick < r.start + r.length).or(app.selected_region.map(|x| x.1));
    let Some(rix) = rix else { return };
    let v = nt::Value { base: VALUES[st.value].1, dots: st.dotted as u8 };
    let velocity = st.dynamic.map_or(80, |d| DYNAMICS[d].1);
    app.checkpoint("Insert events");
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)?.regions.get_mut(rix)) {
        rg.notes.push(Note { channel: 0, pitch, velocity, pos: tick, len: v.ticks() });
        rg.notes.sort_by_key(|n| (n.pos, n.pitch));
        let idx = rg.notes.iter().position(|n| n.pos == tick && n.pitch == pitch).unwrap_or(0);
        app.selected_region = Some((ti, rix));
        app.selected_notes = vec![idx];
    }
    app.after_edit();
    crate::pianoroll::preview(app, ti, pitch);
}

/// The Part box as a strip: note values (and a dot), rests, dynamics.
fn part_box(ui: &mut egui::Ui, r: Rect, st: &mut State, app: &mut App) {
    let p = ui.painter_at(r);
    p.rect_filled(r, 0.0, look::pane_bg());
    p.hline(r.x_range(), r.max.y - 0.5, Stroke::new(1.0, look::line()));
    let cell = vec2(30.0, 26.0);
    let mut x = r.min.x + 10.0;
    let y = r.center().y - cell.y / 2.0;
    p.text(pos2(x, r.center().y), Align2::LEFT_CENTER, "Part Box:", look::regular(12.0), look::text_dim());
    x += 66.0;
    let ink = look::text();
    let button = |ui: &mut egui::Ui, x: f32, on: bool, id: (&str, usize), tip: &str, draw: &dyn Fn(&egui::Painter, Rect)| -> bool {
        let b = Rect::from_min_size(pos2(x, y), cell);
        let resp = ui.interact(b, ui.scope_id().with(id), Sense::click()).on_hover_text(tip);
        let pp = ui.painter_at(b);
        if on {
            pp.rect_filled(b, 5.0, look::accent());
        } else if resp.hovered() {
            pp.rect_filled(b, 5.0, look::group_bg());
        }
        draw(&pp, b);
        resp.clicked()
    };
    for (i, (label, base)) in VALUES.iter().enumerate() {
        let on = st.value == i;
        let col = if on { Color32::WHITE } else { ink };
        let v = nt::Value { base: *base, dots: 0 };
        if button(ui, x, on, ("pb-v", i), &format!("{label} Note"), &|pp, b| note_icon(pp, b, v, col)) {
            st.value = i;
        }
        x += cell.x + 2.0;
    }
    let on = st.dotted;
    if button(ui, x, on, ("pb-dot", 0), "Dotted", &|pp, b| {
        pp.circle_filled(b.center(), 2.6, if on { Color32::WHITE } else { ink });
    }) {
        st.dotted = !st.dotted;
    }
    x += cell.x + 12.0;
    p.vline(x - 6.0, r.y_range().shrink(6.0), Stroke::new(1.0, look::line()));
    for (i, (label, base)) in VALUES.iter().enumerate().take(6) {
        let v = nt::Value { base: *base, dots: 0 };
        if button(ui, x, false, ("pb-r", i), &format!("{label} Rest"), &|pp, b| {
            let sp = 4.0;
            rest(pp, b.center().x, b.center().y + 2.0 * sp, sp, v, false, ink);
        }) {
            app.tell("Rests are drawn by the Score itself (user rests aren't available yet)");
        }
        x += cell.x + 2.0;
    }
    x += 10.0;
    p.vline(x - 6.0, r.y_range().shrink(6.0), Stroke::new(1.0, look::line()));
    for (i, (label, vel)) in DYNAMICS.iter().enumerate() {
        let on = st.dynamic == Some(i);
        let col = if on { Color32::WHITE } else { ink };
        let b = Rect::from_min_size(pos2(x, y), vec2(34.0, cell.y));
        if button(ui, x, on, ("pb-d", i), &format!("{label}: velocity {vel} (new notes and the selected ones)"), &|pp, _| {
            pp.text(b.center(), Align2::CENTER_CENTER, *label, look::bold(13.0), col);
        }) {
            st.dynamic = if on { None } else { Some(i) };
            if !on && !app.selected_notes.is_empty() {
                if let Some((t, rg)) = app.selected_region {
                    app.checkpoint("Change Velocity");
                    let sel = app.selected_notes.clone();
                    if let Some(region) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(rg)) {
                        for &k in &sel {
                            if let Some(n) = region.notes.get_mut(k) {
                                n.velocity = *vel;
                            }
                        }
                    }
                    app.after_edit();
                }
            }
        }
        x += 36.0;
        if x > r.max.x - 40.0 {
            break;
        }
    }
}

// ---- drawing -------------------------------------------------------------------------------

fn y_of(staff: &Staff, pos: i32, sp: f32) -> f32 {
    staff.bottom - pos as f32 * sp / 2.0
}

/// Where a head is drawn: right of the item's time, set off for seconds.
fn head_center(it: &nt::Item, h: &nt::Head, staff: &Staff, sp: f32, x: f32) -> Pos2 {
    let w = head_w(it.value, sp);
    let mut cx = x + 0.8 * sp + w / 2.0;
    if h.shifted {
        cx += if it.stem_up { w - 0.1 * sp } else { -(w - 0.1 * sp) };
    }
    pos2(cx, y_of(staff, h.pos, sp))
}

fn head_w(v: nt::Value, sp: f32) -> f32 {
    if v.base == 1 { 1.6 * sp } else { 1.25 * sp }
}

fn ellipse(c: Pos2, rx: f32, ry: f32, angle: f32, n: usize) -> Vec<Pos2> {
    let (s, co) = angle.sin_cos();
    (0..n)
        .map(|k| {
            let t = k as f32 / n as f32 * std::f32::consts::TAU;
            let (x, y) = (rx * t.cos(), ry * t.sin());
            pos2(c.x + x * co - y * s, c.y + x * s + y * co)
        })
        .collect()
}

/// A notehead: filled (quarter and shorter), open (half), whole.
fn notehead(p: &egui::Painter, c: Pos2, sp: f32, base: u8, col: Color32, paper: Color32) {
    match base {
        1 => {
            p.add(Shape::convex_polygon(ellipse(c, 0.8 * sp, 0.5 * sp, 0.0, 28), col, Stroke::NONE));
            p.add(Shape::convex_polygon(ellipse(c, 0.36 * sp, 0.28 * sp, 1.0, 20), paper, Stroke::NONE));
        }
        2 => {
            p.add(Shape::convex_polygon(ellipse(c, 0.64 * sp, 0.45 * sp, -0.35, 28), col, Stroke::NONE));
            p.add(Shape::convex_polygon(ellipse(c, 0.52 * sp, 0.2 * sp, -0.55, 24), paper, Stroke::NONE));
        }
        _ => {
            p.add(Shape::convex_polygon(ellipse(c, 0.64 * sp, 0.45 * sp, -0.35, 28), col, Stroke::NONE));
        }
    }
}

fn draw_voice(p: &egui::Painter, v: &nt::Voice, staff: &Staff, sp: f32, x_of: &dyn Fn(f32) -> f32, selected: &dyn Fn(usize) -> bool, sel_color: Color32) {
    let stem_w = (0.12 * sp).max(1.0);
    // stem ends, for beams
    let mut stems: Vec<Option<(f32, f32, f32)>> = vec![None; v.items.len()];
    for (k, it) in v.items.iter().enumerate() {
        let x = x_of(it.tick as f32);
        if it.is_rest() {
            let cx = if it.full_bar { x_of((it.tick + it.dur / 2) as f32) } else { x + 0.8 * sp + 0.6 * sp };
            rest(p, cx, staff.bottom, sp, it.value, it.full_bar, INK);
            continue;
        }
        let any_sel = it.heads.iter().any(|h| selected(h.note));
        let w = head_w(it.value, sp);
        let hi = it.heads.iter().map(|h| h.pos).max().unwrap_or(0);
        let lo = it.heads.iter().map(|h| h.pos).min().unwrap_or(0);
        // ledger lines
        let base_x = x + 0.8 * sp;
        let shifted = it.heads.iter().any(|h| h.shifted);
        let lx0 = base_x - 0.35 * sp - if shifted && !it.stem_up { w } else { 0.0 };
        let lx1 = base_x + w + 0.35 * sp + if shifted && it.stem_up { w } else { 0.0 };
        let mut l = 10;
        while l <= hi {
            p.hline(lx0..=lx1, y_of(staff, l, sp), Stroke::new(1.0, INK));
            l += 2;
        }
        let mut l = -2;
        while l >= lo {
            p.hline(lx0..=lx1, y_of(staff, l, sp), Stroke::new(1.0, INK));
            l -= 2;
        }
        // heads, accidentals, dots, ties
        let mut acc_x = base_x - 0.9 * sp - if shifted && !it.stem_up { w } else { 0.0 };
        let mut last_acc_pos = i32::MAX;
        let n_heads = it.heads.len();
        for (h_k, h) in it.heads.iter().enumerate().rev() {
            let c = head_center(it, h, staff, sp, x);
            let col = if selected(h.note) { sel_color } else { INK };
            notehead(p, c, sp, it.value.base, col, REGION_PAPER);
            if let Some(a) = h.acc {
                if (last_acc_pos - h.pos).abs() < 6 && last_acc_pos != i32::MAX {
                    acc_x -= 1.1 * sp;
                }
                accidental(p, pos2(acc_x, c.y), sp, a, col);
                last_acc_pos = h.pos;
            }
            for d in 0..it.value.dots {
                let dy = if h.pos % 2 == 0 { -sp / 2.0 } else { 0.0 };
                p.circle_filled(pos2(base_x + w + 0.5 * sp + d as f32 * 0.6 * sp + if shifted && it.stem_up { w } else { 0.0 }, c.y + dy), 0.18 * sp, col);
            }
            if h.tie {
                if let Some(next) = v.items.get(k + 1) {
                    if let Some(nh) = next.heads.iter().find(|x| x.pitch == h.pitch) {
                        let c2 = head_center(next, nh, staff, sp, x_of(next.tick as f32));
                        // a chord's lower notes tie below, upper above; one note away from the stem
                        let below = if n_heads > 1 && (n_heads % 2 == 0 || h_k != n_heads / 2) { h_k < n_heads / 2 } else { it.stem_up };
                        let dir = if below { 1.0 } else { -1.0 };
                        let (a, b) = (pos2(c.x + 0.5 * sp, c.y + dir * 0.45 * sp), pos2(c2.x - 0.5 * sp, c2.y + dir * 0.45 * sp));
                        let lift = dir * (0.6 * sp + (b.x - a.x) * 0.05).min(1.6 * sp);
                        tie(p, a, b, lift, sp, col);
                    }
                }
            }
        }
        // the stem
        if it.value.base > 1 {
            let up = it.stem_up;
            let sx = if up { base_x + w - stem_w / 2.0 } else { base_x + stem_w / 2.0 };
            let (from_pos, to_pos) = if up { (lo, hi) } else { (hi, lo) };
            let y0 = y_of(staff, from_pos, sp);
            let mut y1 = y_of(staff, to_pos, sp) + if up { -3.5 * sp } else { 3.5 * sp };
            // far from the staff: the stem reaches the middle line
            let mid = y_of(staff, 4, sp);
            if up && y1 > mid {
                y1 = mid;
            }
            if !up && y1 < mid {
                y1 = mid;
            }
            let col = if any_sel { sel_color } else { INK };
            stems[k] = Some((sx, y0, y1));
            if it.beam.is_none() {
                p.line_segment([pos2(sx, y0), pos2(sx, y1)], Stroke::new(stem_w, col));
                for f in 0..it.value.flags() {
                    flag(p, pos2(sx, y1 + if up { f as f32 * 0.8 * sp } else { -(f as f32) * 0.8 * sp }), sp, up, col);
                }
            }
        }
    }
    // beams: a straight beam (slope kept gentle), stems lengthened to meet it
    for b in &v.beams {
        let pts: Vec<(usize, f32, f32, f32)> = b.items.iter().filter_map(|&i| stems[i].map(|(x, y0, y1)| (i, x, y0, y1))).collect();
        if pts.len() < 2 {
            continue;
        }
        let (f, l) = (pts[0], pts[pts.len() - 1]);
        let dx = (l.1 - f.1).max(1.0);
        let slope = ((l.3 - f.3) / dx).clamp(-0.18, 0.18);
        // move the line so every stem is at least 2.5 spaces
        let mut off = 0.0f32;
        for &(_, x, y0, _) in &pts {
            let line_y = f.3 + slope * (x - f.1);
            let need = if b.up { y0 - 2.6 * sp } else { y0 + 2.6 * sp };
            if b.up && line_y + off > need {
                off = need - line_y;
            }
            if !b.up && line_y + off < need {
                off = need - line_y;
            }
        }
        let line = |x: f32| f.3 + off + slope * (x - f.1);
        let thick = 0.5 * sp;
        let dir = if b.up { 1.0 } else { -1.0 };
        let any_sel = b.items.iter().any(|&i| v.items[i].heads.iter().any(|h| selected(h.note)));
        let col = if any_sel { sel_color } else { INK };
        for &(_, x, y0, _) in &pts {
            p.line_segment([pos2(x, y0), pos2(x, line(x))], Stroke::new(stem_w, col));
        }
        let max_level = pts.iter().map(|&(i, ..)| v.items[i].value.flags()).max().unwrap_or(1);
        for level in 0..max_level {
            let dy = dir * level as f32 * 0.75 * sp;
            let has = |i: usize| v.items[i].value.flags() > level;
            for (k, &(i, x, ..)) in pts.iter().enumerate() {
                if !has(i) {
                    continue;
                }
                let next = pts.get(k + 1).filter(|n| has(n.0));
                let prev = k.checked_sub(1).map(|j| pts[j]).filter(|n| has(n.0));
                let (xa, xb) = if let Some(n) = next {
                    (x, n.1)
                } else if prev.is_some() {
                    continue;
                } else if k + 1 < pts.len() {
                    (x, x + 1.1 * sp)
                } else {
                    (x - 1.1 * sp, x)
                };
                let (ya, yb) = (line(xa) + dy, line(xb) + dy);
                p.add(Shape::convex_polygon(vec![pos2(xa - stem_w / 2.0, ya), pos2(xb + stem_w / 2.0, yb), pos2(xb + stem_w / 2.0, yb + dir * thick), pos2(xa - stem_w / 2.0, ya + dir * thick)], col, Stroke::NONE));
            }
        }
    }
    // triplet numbers
    for t in &v.tuplets {
        let (a, b) = (&v.items[t.first], &v.items[t.last]);
        let xa = x_of(a.tick as f32) + 0.8 * sp;
        let xb = x_of(b.tick as f32) + 0.8 * sp + 1.25 * sp;
        let ys: Vec<f32> = (t.first..=t.last).filter_map(|i| stems[i].map(|s| s.2)).collect();
        let y = if t.up {
            ys.iter().copied().fold(staff.bottom - 4.0 * sp, f32::min) - 1.0 * sp
        } else {
            ys.iter().copied().fold(staff.bottom, f32::max) + 1.1 * sp
        };
        if a.beam.is_none() || a.beam != b.beam {
            let d = if t.up { 0.5 * sp } else { -0.5 * sp };
            p.line_segment([pos2(xa, y + d), pos2(xa, y)], Stroke::new(1.0, INK));
            p.line_segment([pos2(xa, y), pos2(xb, y)], Stroke::new(1.0, INK));
            p.line_segment([pos2(xb, y), pos2(xb, y + d)], Stroke::new(1.0, INK));
        }
        let c = pos2((xa + xb) / 2.0, y);
        p.rect_filled(Rect::from_center_size(c, vec2(sp * 1.1, sp * 1.3)), 0.0, REGION_PAPER);
        p.text(c, Align2::CENTER_CENTER, "3", look::bold(sp * 1.5), INK);
    }
}

/// A filled band between two curves of as many points (a tie's crescent, a flag): one mesh,
/// its edges stroked thinly for smoothness.
fn band(p: &egui::Painter, a: &[Pos2], b: &[Pos2], col: Color32) {
    let n = a.len().min(b.len());
    if n < 2 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for k in 0..n {
        mesh.colored_vertex(a[k], col);
        mesh.colored_vertex(b[k], col);
    }
    for k in 0..n as u32 - 1 {
        let i = 2 * k;
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
    }
    p.add(Shape::mesh(mesh));
    p.add(Shape::Path(PathShape::line(a[..n].to_vec(), PathStroke::new(0.7, col))));
    p.add(Shape::Path(PathShape::line(b[..n].to_vec(), PathStroke::new(0.7, col))));
}

fn bezier(pts: [Pos2; 4], n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|k| {
            let t = k as f32 / n as f32;
            let u = 1.0 - t;
            let f = |a: f32, b: f32, c: f32, d: f32| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
            pos2(f(pts[0].x, pts[1].x, pts[2].x, pts[3].x), f(pts[0].y, pts[1].y, pts[2].y, pts[3].y))
        })
        .collect()
}

fn tie(p: &egui::Painter, a: Pos2, b: Pos2, lift: f32, sp: f32, col: Color32) {
    let c1 = pos2(a.x + (b.x - a.x) * 0.25, a.y + lift);
    let c2 = pos2(a.x + (b.x - a.x) * 0.75, b.y + lift);
    let outer = bezier([a, c1, c2, b], 24);
    let thick = lift.signum() * 0.22 * sp;
    let inner = bezier([a, pos2(c1.x, c1.y - thick), pos2(c2.x, c2.y - thick), b], 24);
    band(p, &outer, &inner, col);
}

fn flag(p: &egui::Painter, at: Pos2, sp: f32, up: bool, col: Color32) {
    let d = if up { 1.0 } else { -1.0 };
    // the front edge sweeps out from the stem and down; the back edge comes back to it thinner
    let front = bezier([at, pos2(at.x + 0.15 * sp, at.y + d * 0.9 * sp), pos2(at.x + 1.35 * sp, at.y + d * 1.3 * sp), pos2(at.x + 0.95 * sp, at.y + d * 2.9 * sp)], 18);
    let back = bezier([pos2(at.x, at.y + d * 0.75 * sp), pos2(at.x + 0.25 * sp, at.y + d * 1.35 * sp), pos2(at.x + 1.1 * sp, at.y + d * 1.6 * sp), pos2(at.x + 0.95 * sp, at.y + d * 2.9 * sp)], 18);
    band(p, &front, &back, col);
}


/// Rests at x (centre) on a staff whose bottom line is at `bottom`.
fn rest(p: &egui::Painter, x: f32, bottom: f32, sp: f32, v: nt::Value, full_bar: bool, col: Color32) {
    let y = |pos: f32| bottom - pos * sp / 2.0;
    match v.base {
        1 => {
            // hangs from the fourth line
            p.rect_filled(Rect::from_min_max(pos2(x - 0.6 * sp, y(6.0)), pos2(x + 0.6 * sp, y(6.0) + 0.5 * sp)), 0.0, col);
            let _ = full_bar;
        }
        2 => {
            p.rect_filled(Rect::from_min_max(pos2(x - 0.6 * sp, y(4.0) - 0.5 * sp), pos2(x + 0.6 * sp, y(4.0))), 0.0, col);
        }
        4 => {
            let m = y(4.0);
            let pt = |a: f32, b: f32| pos2(x + a * sp, m + b * sp);
            let w = 0.22 * sp;
            p.line_segment([pt(-0.25, -1.5), pt(0.35, -0.75)], Stroke::new(w, col));
            p.add(Shape::convex_polygon(vec![pt(0.35, -0.75), pt(-0.25, -0.1), pt(-0.05, 0.1), pt(0.5, -0.55)], col, Stroke::NONE));
            p.line_segment([pt(-0.2, -0.1), pt(0.35, 0.55)], Stroke::new(w * 1.1, col));
            let hook = CubicBezierShape::from_points_stroke([pt(0.35, 0.55), pt(-0.3, 0.3), pt(-0.45, 1.0), pt(0.1, 1.4)], false, Color32::TRANSPARENT, Stroke::new(w, col));
            p.add(hook);
        }
        _ => {
            // 8th, 16th, 32nd, 64th: hooks down a slanted stem
            let n = v.flags() as f32;
            let top = y(5.0) + 0.0;
            let stem_top = pos2(x + 0.45 * sp, top - (n - 1.0).max(0.0) * 0.0);
            let stem_bot = pos2(x - 0.1 * sp - n * 0.15 * sp, y(5.0) + (n + 1.0) * sp);
            p.line_segment([stem_top, stem_bot], Stroke::new(0.14 * sp, col));
            for k in 0..n as usize {
                let t = k as f32 / (n + 1.0).max(1.0);
                let at = pos2(stem_top.x + (stem_bot.x - stem_top.x) * t, stem_top.y + (stem_bot.y - stem_top.y) * t);
                let dot = pos2(at.x - 0.85 * sp, at.y + 0.1 * sp);
                p.circle_filled(dot, 0.26 * sp, col);
                let curve = CubicBezierShape::from_points_stroke([dot, pos2(dot.x + 0.2 * sp, at.y + 0.45 * sp), pos2(at.x - 0.2 * sp, at.y + 0.3 * sp), at], false, Color32::TRANSPARENT, Stroke::new(0.16 * sp, col));
                p.add(curve);
            }
        }
    }
}

/// ♯ ♭ ♮ (and double signs as two) centred at `c`.
fn accidental(p: &egui::Painter, c: Pos2, sp: f32, alter: i8, col: Color32) {
    let thin = Stroke::new((0.11 * sp).max(1.0), col);
    let thick = (0.3 * sp).max(1.5);
    match alter {
        1 => {
            for dx in [-0.22, 0.22] {
                p.line_segment([pos2(c.x + dx * sp, c.y - 1.3 * sp + dx * 0.4 * sp), pos2(c.x + dx * sp, c.y + 1.3 * sp + dx * 0.4 * sp)], thin);
            }
            for dy in [-0.42, 0.42] {
                let y = c.y + dy * sp;
                p.add(Shape::convex_polygon(vec![pos2(c.x - 0.5 * sp, y + 0.15 * sp), pos2(c.x + 0.5 * sp, y - 0.15 * sp), pos2(c.x + 0.5 * sp, y - 0.15 * sp + thick), pos2(c.x - 0.5 * sp, y + 0.15 * sp + thick)], col, Stroke::NONE));
            }
        }
        2 => {
            let d = 0.32 * sp;
            p.line_segment([pos2(c.x - d, c.y - d), pos2(c.x + d, c.y + d)], Stroke::new(0.18 * sp, col));
            p.line_segment([pos2(c.x - d, c.y + d), pos2(c.x + d, c.y - d)], Stroke::new(0.18 * sp, col));
        }
        -1 | -2 => {
            let n = if alter == -2 { 2 } else { 1 };
            for k in 0..n {
                let x = c.x - 0.25 * sp - k as f32 * 0.7 * sp + if n == 2 { 0.35 * sp } else { 0.0 };
                p.line_segment([pos2(x, c.y - 1.7 * sp), pos2(x, c.y + 0.5 * sp)], thin);
                let bowl = CubicBezierShape::from_points_stroke([pos2(x, c.y - 0.1 * sp), pos2(x + 0.5 * sp, c.y - 0.55 * sp), pos2(x + 0.85 * sp, c.y - 0.05 * sp), pos2(x, c.y + 0.5 * sp)], false, Color32::TRANSPARENT, Stroke::new(0.2 * sp, col));
                p.add(bowl);
            }
        }
        _ => {
            p.line_segment([pos2(c.x - 0.3 * sp, c.y - 1.2 * sp), pos2(c.x - 0.3 * sp, c.y + 0.6 * sp)], thin);
            p.line_segment([pos2(c.x + 0.3 * sp, c.y - 0.6 * sp), pos2(c.x + 0.3 * sp, c.y + 1.2 * sp)], thin);
            for dy in [-0.45, 0.35] {
                let y = c.y + dy * sp;
                p.add(Shape::convex_polygon(vec![pos2(c.x - 0.3 * sp, y + 0.1 * sp), pos2(c.x + 0.3 * sp, y - 0.1 * sp), pos2(c.x + 0.3 * sp, y - 0.1 * sp + thick * 0.8), pos2(c.x - 0.3 * sp, y + 0.1 * sp + thick * 0.8)], col, Stroke::NONE));
            }
        }
    }
}

/// A smooth curve through points (Catmull-Rom), sampled.
fn spline(pts: &[(f32, f32)], per: usize) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for i in 0..pts.len().saturating_sub(1) {
        let p0 = pts[i.saturating_sub(1)];
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        for k in 0..per {
            let t = k as f32 / per as f32;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
            out.push((f(p0.0, p1.0, p2.0, p3.0), f(p0.1, p1.1, p2.1, p3.1)));
        }
    }
    if let Some(l) = pts.last() {
        out.push(*l);
    }
    out
}

/// A calligraphic stroke: thicker where the pen moves up and down, thin across.
fn pen(p: &egui::Painter, pts: &[Pos2], thin: f32, thick: f32, col: Color32) {
    for w in pts.windows(2) {
        let d = w[1] - w[0];
        let len = d.length().max(0.001);
        let vertical = (d.y / len).abs();
        let width = thin + (thick - thin) * vertical * vertical;
        let n = vec2(-d.y, d.x) / len * width / 2.0;
        p.add(Shape::convex_polygon(vec![w[0] + n, w[1] + n, w[1] - n, w[0] - n], col, Stroke::NONE));
        p.circle_filled(w[1], width / 2.0, col);
    }
}

/// The G clef, curling round the second line; `at` is the stroke's x on the bottom line.
pub(crate) fn treble_clef(p: &egui::Painter, at: Pos2, sp: f32, col: Color32) {
    // in spaces: x right, y up from the bottom line
    let pts = [
        (-0.55, -1.75), (-0.3, -2.2), (0.15, -2.25), (0.45, -1.8), (0.38, -0.8), (0.22, 1.5), (0.08, 3.5), (0.15, 4.8),
        (0.45, 5.65), (0.62, 6.3), (0.35, 6.55), (-0.05, 6.0), (-0.42, 5.0), (-0.75, 4.0), (-1.05, 3.0), (-1.2, 2.0),
        (-1.08, 0.9), (-0.6, 0.15), (0.1, -0.05), (0.75, 0.3), (1.0, 1.0), (0.82, 1.72), (0.2, 1.98), (-0.32, 1.62), (-0.4, 1.05), (-0.12, 0.62),
    ];
    let s = spline(&pts, 6);
    let pts: Vec<Pos2> = s.iter().map(|&(x, y)| pos2(at.x + x * sp, at.y - y * sp)).collect();
    pen(p, &pts, 0.1 * sp, 0.36 * sp, col);
    p.circle_filled(pos2(at.x - 0.55 * sp, at.y + 1.75 * sp), 0.36 * sp, col);
}

/// The F clef: its dot on the fourth line, two dots either side of it.
pub(crate) fn bass_clef(p: &egui::Painter, at: Pos2, sp: f32, col: Color32) {
    let pts = [(0.0, 3.0), (0.25, 3.55), (0.95, 3.92), (1.7, 3.6), (1.98, 2.7), (1.65, 1.6), (0.95, 0.75), (0.05, 0.05)];
    let s = spline(&pts, 6);
    let pts: Vec<Pos2> = s.iter().map(|&(x, y)| pos2(at.x + x * sp, at.y - y * sp)).collect();
    pen(p, &pts, 0.12 * sp, 0.42 * sp, col);
    p.circle_filled(pos2(at.x + 0.22 * sp, at.y - 3.0 * sp), 0.42 * sp, col);
    p.circle_filled(pos2(at.x + 2.5 * sp, at.y - 3.5 * sp), 0.18 * sp, col);
    p.circle_filled(pos2(at.x + 2.5 * sp, at.y - 2.5 * sp), 0.18 * sp, col);
}

fn brace(p: &egui::Painter, x: f32, y0: f32, y1: f32, col: Color32) {
    let h = y1 - y0;
    let m = (y0 + y1) / 2.0;
    let pts = [(0.0, y0), (-4.0, y0 + h * 0.15), (0.0, m - h * 0.12), (-5.0, m), (0.0, m + h * 0.12), (-4.0, y1 - h * 0.15), (0.0, y1)];
    let s = spline(&pts, 8);
    let pts: Vec<Pos2> = s.iter().map(|&(dx, y)| pos2(x + dx, y)).collect();
    p.add(Shape::Path(PathShape::line(pts, PathStroke::new(2.0, col))));
}

/// A Part box note icon: head, stem, flags.
fn note_icon(p: &egui::Painter, r: Rect, v: nt::Value, col: Color32) {
    let sp = 4.6;
    let c = pos2(r.center().x - 2.0, r.center().y + 6.0);
    notehead(p, c, sp, v.base, col, Color32::TRANSPARENT);
    if v.base == 1 || v.base == 2 {
        // the icons on a dark strip: open heads drawn as rings
        p.add(Shape::closed_line(ellipse(c, if v.base == 1 { 0.8 * sp } else { 0.64 * sp }, 0.45 * sp, if v.base == 1 { 0.0 } else { -0.35 }, 24), Stroke::new(1.3, col)));
    }
    if v.base > 1 {
        let x = c.x + 0.62 * sp;
        let top = c.y - 3.4 * sp;
        p.line_segment([pos2(x, c.y), pos2(x, top)], Stroke::new(1.0, col));
        for f in 0..v.flags() {
            flag(p, pos2(x, top + f as f32 * 0.7 * sp), sp * 0.8, true, col);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spline_passes_through_points() {
        let s = spline(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)], 4);
        assert_eq!(s.first(), Some(&(0.0, 0.0)));
        assert_eq!(s.last(), Some(&(2.0, 0.0)));
        assert!(s.iter().any(|&(x, y)| (x - 1.0).abs() < 1e-4 && (y - 1.0).abs() < 1e-4));
    }

    #[test]
    fn dynamics_rise() {
        assert!(DYNAMICS.windows(2).all(|w| w[0].1 < w[1].1));
    }
}
