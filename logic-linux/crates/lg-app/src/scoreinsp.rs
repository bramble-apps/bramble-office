//! The Score inspector, as Logic's (shown in the main window's inspector while the Score Editor
//! is open): "Region:" with the region's score parameters (Style, Quantize, Interpretation,
//! Syncopation, Max. Dots), "Event:" with the selected notes (MIDI Channel, Pitch, Velocity,
//! Length) or the selected text object (Staff, Style, Align, Vertical / Horizontal Pos, Lyric),
//! and the Part box: group buttons that show or hide symbol groups (All shows every group), the
//! groups stacked in the order they were shown and reordered by their handles, Part Box sets
//! (New / Rename / Delete Set), symbols picked by a click (applied to the selected notes) or
//! dragged into the score. Under it the inspector channel strips when there is room.
//!
//! Also here: what the newer Part box groups insert (sustain pedal markings as controller 64
//! events, Bars and Repeats signs, line and page breaks, time signatures), the Key Signature,
//! Time Signature and Text Styles windows, and Delete for a selected symbol.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::score::{BarKind, BarSym, Break, TextStyle, ALIGNS, NOTE_HEADS, ORNAMENTS};
use lg_notation as nt;

use crate::app::App;
use crate::look;
use crate::score::{self, Part, GROUP_NAMES, TIMES};

pub const COMMANDS: &[&str] = &["Text Styles…", "Reset Line Layout", "Show Score Sets", "Show/Hide Score Sets Window", "Show Local Inspector", "Hide Local Inspector", "Show/Hide Local Inspector"];

const ROW: f32 = 25.0;

thread_local! {
    /// Where the current section's rows start (for the alternating shade).
    static SECTION_TOP: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
}

fn section(y: f32) {
    SECTION_TOP.with(|c| c.set(y));
}
const HEAD: f32 = 32.0;

/// The Score is shown in the editor area (its inspector replaces the main one's rows).
pub fn active(app: &App, ctx: &egui::Context) -> bool {
    app.bottom == crate::app::Bottom::PianoRoll && score::editor_tab(app, ctx) == score::EditorTab::Score
}

// ---- Part box state and sets ---------------------------------------------------------------

/// Which groups the Part box shows (in order), the set's name, which headers are open.
#[derive(Clone, Debug)]
pub struct PartBox {
    pub shown: Vec<usize>,
    pub set: String,
    pub open: bool,
    pub region_open: bool,
    pub event_open: bool,
    /// Linear view's Insert / Quantize / Velocity rows (closed by default, as Logic 13 opens
    /// the Score inspector at Region).
    pub linear_open: bool,
    /// A group handle being dragged: (group, its block's top when the drag began).
    drag: Option<usize>,
    /// Set name being typed (New / Rename).
    naming: Option<(bool, String)>,
}

impl Default for PartBox {
    fn default() -> Self {
        PartBox { shown: vec![0], set: String::new(), open: true, region_open: true, event_open: true, linear_open: false, drag: None, naming: None }
    }
}

fn pb_id() -> egui::Id {
    egui::Id::unique("lg-score-partbox")
}

pub fn part_box_state(ctx: &egui::Context) -> PartBox {
    ctx.data(|d| d.get_temp::<PartBox>(pb_id())).unwrap_or_default()
}

fn sets_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("partbox-sets.txt"))
}

/// The user's Part Box sets: (name, groups in order).
pub fn load_sets() -> Vec<(String, Vec<usize>)> {
    let text = sets_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    parse_sets(&text)
}

pub fn parse_sets(text: &str) -> Vec<(String, Vec<usize>)> {
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(n, g)| (n.to_string(), g.split(',').filter_map(|x| x.parse::<usize>().ok()).filter(|&x| x < GROUP_NAMES.len()).collect()))
        .collect()
}

pub fn sets_text(sets: &[(String, Vec<usize>)]) -> String {
    sets.iter().map(|(n, g)| format!("{}\t{}\n", n.replace(['\t', '\n'], " "), g.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","))).collect()
}

fn save_sets(sets: &[(String, Vec<usize>)]) {
    if let Some(p) = sets_path() {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, sets_text(sets));
    }
}

/// A group button: shows the group below the others, or hides it when shown.
pub fn toggle_group(shown: &mut Vec<usize>, g: usize) {
    if let Some(i) = shown.iter().position(|&x| x == g) {
        shown.remove(i);
    } else {
        shown.push(g);
    }
}

/// A group dragged by its handle to another place in the stack.
pub fn move_group(shown: &mut Vec<usize>, g: usize, to: usize) {
    if let Some(i) = shown.iter().position(|&x| x == g) {
        let v = shown.remove(i);
        shown.insert(to.min(shown.len()), v);
    }
}

// ---- the inspector -------------------------------------------------------------------------

fn label(p: &egui::Painter, area: Rect, y: f32, text: &str) {
    // (Logic's inspector rows alternate in shade)
    if ((y - SECTION_TOP.with(|c| c.get())) / ROW).round() as i32 % 2 == 1 {
        p.rect_filled(Rect::from_min_size(pos2(area.min.x, y), vec2(area.width() - 1.0, ROW)), 0.0, look::mix(look::pane_bg(), Color32::BLACK, 0.18));
    }
    p.text(pos2(area.min.x + area.width() * 0.5, y + ROW / 2.0), Align2::RIGHT_CENTER, format!("{text}:"), look::regular(13.0), look::text());
}

fn slot(area: Rect, y: f32) -> Rect {
    Rect::from_min_max(pos2(area.min.x + area.width() * 0.5 + 6.0, y + 1.0), pos2(area.max.x - 14.0, y + ROW - 1.0))
}

/// Up/down chevrons (Logic's pop-up rows).
fn updown(p: &egui::Painter, at: egui::Pos2, col: Color32) {
    for dy in [-3.0f32, 3.0] {
        let s = dy.signum();
        p.line_segment([at + vec2(-3.0, dy - s * 1.5), at + vec2(0.0, dy + s * 1.5)], Stroke::new(1.3, col));
        p.line_segment([at + vec2(0.0, dy + s * 1.5), at + vec2(3.0, dy - s * 1.5)], Stroke::new(1.3, col));
    }
}

/// A pop-up row: the value left, chevrons at the right; returns the choice made.
fn popup(ui: &mut egui::Ui, r: Rect, id: egui::Id, current: usize, choices: &[String]) -> Option<usize> {
    let resp = ui.interact(r, id, Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(r, 4.0, look::group_bg());
    }
    p.with_clip_rect(r.shrink2(vec2(0.0, 0.0))).text(pos2(r.min.x, r.center().y), Align2::LEFT_CENTER, choices.get(current).map_or("", |s| s.as_str()), look::regular(13.0), look::text_dim());
    updown(p, pos2(r.max.x - 4.0, r.center().y), look::text_dim());
    let mut out = None;
    egui::Popup::menu(&resp).show(|ui| {
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (i, c) in choices.iter().enumerate() {
                if ui.add(egui::Button::selectable(i == current, c.as_str())).clicked() {
                    out = Some(i);
                    ui.close();
                }
            }
        });
    });
    out
}

/// A checkbox right after the label (Logic's inspector).
fn check(ui: &mut egui::Ui, r: Rect, id: egui::Id, on: &mut bool) -> bool {
    let b = Rect::from_min_size(pos2(r.min.x, r.center().y - 7.0), vec2(14.0, 14.0));
    let resp = ui.interact(b.expand(3.0), id, Sense::click());
    let p = ui.painter();
    p.rect_filled(b, 3.0, if *on { look::accent() } else { look::lcd_bg() });
    if !*on {
        p.rect_stroke(b, 3.0, Stroke::new(1.0, look::line()), egui::StrokeKind::Inside);
    } else {
        p.line_segment([b.center() + vec2(-3.5, 0.0), b.center() + vec2(-1.0, 3.0)], Stroke::new(1.8, Color32::WHITE));
        p.line_segment([b.center() + vec2(-1.0, 3.0), b.center() + vec2(4.0, -3.5)], Stroke::new(1.8, Color32::WHITE));
    }
    if resp.clicked() {
        *on = !*on;
        return true;
    }
    false
}

/// A value dragged up and down (Shift finer), typed after a double-click; left-aligned.
fn value(ui: &mut egui::Ui, r: Rect, id: egui::Id, v: &mut f32, min: f32, max: f32, shown: &dyn Fn(f32) -> String) -> bool {
    let typing_id = id.with("typing");
    if let Some(mut t) = ui.data(|d| d.get_temp::<String>(typing_id)) {
        let te = ui.put(r, egui::TextEdit::singleline(&mut t).font(look::regular(13.0)));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(typing_id));
            if let Ok(x) = t.trim().parse::<f32>() {
                *v = x.clamp(min, max);
                return true;
            }
        } else {
            ui.data_mut(|d| d.insert_temp(typing_id, t));
        }
        return false;
    }
    let resp = ui.interact(r, id, Sense::click_and_drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    ui.painter().text(pos2(r.min.x, r.center().y), Align2::LEFT_CENTER, shown(*v), look::regular(13.0), look::text_dim());
    let mut changed = false;
    if resp.dragged() {
        let acc_id = id.with("acc");
        let mut acc: f32 = ui.data(|d| d.get_temp(acc_id)).unwrap_or(0.0);
        acc += -resp.drag_delta().y / if ui.input(|i| i.modifiers.shift) { 12.0 } else { 4.0 };
        let steps = acc.trunc();
        ui.data_mut(|d| d.insert_temp(acc_id, acc - steps));
        if steps != 0.0 {
            *v = (*v + steps).clamp(min, max);
            changed = true;
        }
    }
    if resp.double_clicked() {
        ui.data_mut(|d| d.insert_temp(typing_id, format!("{}", *v)));
    }
    changed
}

/// A section header with its disclosure chevron (Region:, Event:).
fn header(ui: &mut egui::Ui, area: Rect, y: f32, id: &str, title: &str, value: &str, open: bool) -> bool {
    let r = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), HEAD));
    crate::widgets::disclosure(ui, r, ui.scope_id().with(id), title, value, open)
}

/// LG_SCORE_SYMBOLS=1 (checks): a set of the newer symbols into the shown track, once.
fn check_symbols(app: &mut App, t: usize) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    if std::env::var("LG_SCORE_SYMBOLS").is_err() {
        return;
    }
    ONCE.call_once(|| {
        let bar = app.bar_ticks();
        let first = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.iter().find(|r| r.audio.is_none())).map(|r| (r.start / bar, r.notes.first().map(|n| (n.pos, n.pitch))));
        let Some((b0, note)) = first else { return };
        let at = |k: u32| (b0 + k) * bar;
        insert_bar(app, at(0), BarKind::RepeatStart);
        insert_bar(app, at(1), BarKind::RepeatEnd);
        insert_bar(app, at(0), BarKind::Ending1);
        insert_bar(app, at(1), BarKind::Ending2);
        insert_bar(app, at(1) + bar / 2, BarKind::Segno);
        insert_bar(app, at(3), BarKind::Repeat1);
        insert_bar(app, at(4), BarKind::Final);
        insert_pedal(app, t, at(0));
        insert_pedal(app, t, at(0) + bar / 2);
        use lg_core::score::{Symbol, SymbolKind};
        let staff = 0;
        score::add_symbol(app, t, Symbol { tick: at(1), kind: SymbolKind::Ornament, text: "Trill Line".into(), len: bar / 2, staff, ..Default::default() }, "x");
        score::add_symbol(app, t, Symbol { tick: at(2), kind: SymbolKind::Ornament, text: "Mordent".into(), staff, ..Default::default() }, "x");
        score::add_symbol(app, t, Symbol { tick: at(0), kind: SymbolKind::Chord, text: "Cmaj7/E".into(), staff, ..Default::default() }, "x");
        score::add_symbol(app, t, Symbol { tick: at(2), kind: SymbolKind::AutoText, text: "SONG".into(), style: "Title".into(), staff, ..Default::default() }, "x");
        score::add_symbol(app, t, Symbol { tick: at(1), kind: SymbolKind::Lyric, text: "la".into(), staff, ..Default::default() }, "x");
        if let (Some((pos, pitch)), Some(id)) = (note, app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|t| t.extra.id)) {
            if let Some(p) = app.project.as_mut() {
                p.score.edit(id, pos, pitch, |a| a.head = 3);
            }
        }
        insert_break(app, b0 + 2, Break::Line);
        // (=2: every MIDI track in a set, bracketed and joined by bar lines)
        if std::env::var("LG_SCORE_SYMBOLS").as_deref() == Ok("2") {
            let ids: Vec<u64> = app.project.as_ref().map(|p| p.tracks.iter().filter(|t| t.kind != lg_core::TrackKind::Audio && t.regions.iter().any(|r| r.audio.is_none())).map(|t| t.extra.id).collect()).unwrap_or_default();
            let k = new_set(app, "Check", &ids);
            if let Some(set) = app.project.as_mut().and_then(|p| p.score.sets.get_mut(k)) {
                for e in set.entries.iter_mut() {
                    e.bracket = true;
                    e.barline = true;
                }
                if let Some(e) = set.entries.first_mut() {
                    e.full = "Keys".into();
                }
            }
            let ctx = app.ctx.clone();
            score::with_state(&ctx, |s| s.set = Some(k));
        }
    });
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let ctx = ui.ctx().clone();
    if let Some(t) = app.selected_region.map(|x| x.0) {
        check_symbols(app, t);
    }
    let mut pb = part_box_state(&ctx);
    let mut st = score::state(&ctx);
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.vline(area.max.x, area.y_range(), Stroke::new(1.0, look::line()));
    let mut y = area.min.y;
    // Filter: the score set shown (All Instruments, the project's sets)
    {
        let r = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), HEAD));
        p.text(pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, "Filter:", look::regular(13.0), look::text());
        let mut names = vec!["All Instruments".to_string()];
        names.extend(app.project.as_ref().map(|p| p.score.sets.iter().map(|s| s.name.clone()).collect::<Vec<_>>()).unwrap_or_default());
        let cur = st.set.map_or(0, |k| k + 1).min(names.len() - 1);
        let pr = Rect::from_min_max(pos2(r.min.x + 62.0, r.center().y - 11.0), pos2(r.max.x - 14.0, r.center().y + 11.0));
        // Option-click: the instruments, to extract one as a part (its track's name)
        let ext_id = ui.scope_id().with("si-filter-extract");
        if ui.input(|i| i.pointer.primary_pressed() && i.pointer.interact_pos().is_some_and(|pp| pr.contains(pp))) {
            let alt = ui.input(|i| i.modifiers.alt);
            ctx.data_mut(|d| d.insert_temp(ext_id, alt));
        }
        let extract = ctx.data(|d| d.get_temp::<bool>(ext_id)).unwrap_or(false);
        let parts = crate::scoreattr::part_choices(app);
        if extract {
            let pn: Vec<String> = parts.iter().map(|x| x.1.clone()).collect();
            let shown_now = st.score_set.as_ref().and_then(|v| (v.len() == 1).then(|| v[0])).and_then(|id| parts.iter().position(|x| x.0 == id)).unwrap_or(0);
            if let Some(k) = popup(ui, pr, ui.scope_id().with("si-filter-parts"), shown_now, &pn) {
                let id = parts[k].0;
                st.set = None;
                st.score_set = Some(vec![id]);
                score::with_state(&ctx, |s| {
                    s.set = None;
                    s.score_set = Some(vec![id]);
                });
                ctx.data_mut(|d| d.insert_temp(ext_id, false));
            }
        } else if let Some(k) = popup(ui, pr, ui.scope_id().with("si-filter"), cur, &names) {
            st.set = (k > 0).then(|| k - 1);
            st.score_set = None;
            score::with_state(&ctx, |s| {
                s.set = st.set;
                s.score_set = None;
            });
        }
        p.hline(r.x_range(), r.max.y, Stroke::new(1.0, look::line()));
        y += HEAD + 1.0;
    }
    let sel = app.selected_region.filter(|&(t, r)| app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).is_some_and(|r| r.audio.is_none()));
    let t = sel.map(|x| x.0).or(app.selected_track);
    // (Linear view: Insert, Time Quantize, Scale Quantize and Velocity, as Logic's Score
    // inspector shows them above the Region parameters)
    if score::state(&ctx).view == score::View::Linear {
        let ins = match st.part {
            Part::Note(i) => score::VALUES[i].0.to_string(),
            _ => String::new(),
        };
        if header(ui, area, y, "si-linear", "Insert:", &ins, pb.linear_open) {
            pb.linear_open = !pb.linear_open;
        }
        y += HEAD + 1.0;
        if pb.linear_open {
            y = linear_rows(app, ui, area, y, &mut st, sel);
        }
    }
    // Region:
    let n_regions = { let mut v = app.selected_regions.clone(); v.extend(app.selected_region); v.sort_unstable(); v.dedup(); v.len() };
    let rname = if sel.is_none() { "Insert Defaults".to_string() } else if n_regions > 1 { format!("{n_regions} selected") } else { sel.and_then(|(t, r)| app.project.as_ref()?.tracks.get(t)?.regions.get(r)).map(|r| r.name.clone()).unwrap_or_default() };
    if header(ui, area, y, "si-region", "Region:", &rname, pb.region_open) {
        pb.region_open = !pb.region_open;
    }
    y += HEAD + 1.0;
    if pb.region_open {
        if sel.is_none() {
            // no region selected: the Insert Defaults new regions take
            y = defaults_rows(app, ui, area, y);
        } else if let Some(t) = t {
            y = region_rows(app, ui, area, y, t);
        }
    }
    // Event:
    let n_sel = app.selected_notes.len();
    let ev = if st.sel_symbol.is_some() { "1 Text selected".to_string() } else if n_sel == 1 { "1 Note selected".into() } else if n_sel > 1 { format!("{n_sel} Notes selected") } else { String::new() };
    if header(ui, area, y, "si-event", "Event:", &ev, pb.event_open) {
        pb.event_open = !pb.event_open;
    }
    y += HEAD + 1.0;
    if pb.event_open {
        y = if st.sel_symbol.is_some() { text_rows(app, ui, area, y, &mut st) } else { note_rows(app, ui, area, y) };
    }
    // Part Box:
    y = part_box(app, ui, area, y, &mut pb, &mut st);
    let auto_on = crate::scoreattr::automation_shown(&ctx) && t.is_some();
    // the channel strips, when there's room
    let strips = Rect::from_min_max(pos2(area.min.x, y + 6.0), pos2(area.max.x - 1.0, area.max.y - if auto_on { 40.0 } else { 4.0 }));
    if strips.height() > 260.0 {
        p.vline(strips.center().x, strips.y_range(), Stroke::new(1.0, look::line()));
        let w = strips.width() / 2.0;
        ui.scope_builder(egui::UiBuilder::new().id_salt("inspector-strips").max_rect(strips), |ui| {
            if let Some(t) = app.selected_track.filter(|t| app.project.as_ref().is_some_and(|p| *t < p.tracks.len())) {
                crate::mixer::track_strip(app, ui, Rect::from_min_size(strips.min + vec2(4.0, 0.0), vec2(w - 8.0, strips.height())), t);
            }
            crate::mixer::output_strip(app, ui, Rect::from_min_size(strips.min + vec2(w + 4.0, 0.0), vec2(w - 8.0, strips.height())));
        });
    }
    // with automation shown: the Automation button, mode and parameter pop-ups at the bottom
    if let (true, Some(t)) = (auto_on, t) {
        let r = Rect::from_min_size(pos2(area.min.x + 10.0, area.max.y - 30.0), vec2(area.width() - 20.0, 22.0));
        p.hline(area.x_range(), r.min.y - 6.0, Stroke::new(1.0, look::line()));
        crate::automation::header(app, ui, r, t);
    }
    ctx.data_mut(|d| d.insert_temp(pb_id(), pb));
    score::with_state(&ctx, |s| {
        s.part = st.part;
        s.dotted = st.dotted;
        s.triplet = st.triplet;
    });
}

/// Linear view: Insert (the note value inserted), Time Quantize with Strength and Swing and its
/// Q button, Scale Quantize (the project's key) and its Q, and the selected notes' Velocity —
/// the same quantizing as the Piano Roll's (its values shared).
fn linear_rows(app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32, st: &mut score::State, sel: Option<(usize, usize)>) -> f32 {
    section(y);
    let p = ui.painter().clone();
    let id = ui.scope_id().with("si-lin");
    y += 4.0;
    label(&p, area, y, "Insert");
    let values: Vec<String> = score::VALUES.iter().map(|v| v.0.to_string()).collect();
    let cur = match st.part {
        score::Part::Note(i) => i,
        _ => values.len(),
    };
    let mut shown = values.clone();
    if cur >= values.len() {
        shown.push(st.part.name());
    }
    if let Some(k) = popup(ui, slot(area, y), id.with("insert"), cur, &shown) {
        if k < values.len() {
            st.part = score::Part::Note(k);
        }
    }
    y += ROW;
    label(&p, area, y, "Time Quantize");
    let names: Vec<String> = crate::inspector::QUANTIZE.iter().map(|q| q.0.to_string()).collect();
    let qc = crate::inspector::QUANTIZE.iter().position(|q| q.1 == app.roll_quantize).unwrap_or(0);
    let s = slot(area, y);
    let (pr, qb) = (Rect::from_min_max(s.min, pos2(s.max.x - 30.0, s.max.y)), Rect::from_min_max(pos2(s.max.x - 26.0, s.min.y), s.max));
    if let Some(k) = popup(ui, pr, id.with("tq"), qc, &names) {
        app.roll_quantize = crate::inspector::QUANTIZE[k].1;
    }
    let notes_of = |app: &App| sel.and_then(|(t, r)| app.project.as_ref()?.tracks.get(t)?.regions.get(r)).cloned();
    if ui.put(qb, egui::Button::new("Q")).on_hover_text("Quantize the selected notes").clicked() && app.roll_quantize > 0 {
        if let (Some((t, r)), Some(region)) = (sel, notes_of(app)) {
            let (grid, strength, swing) = (app.roll_quantize, app.roll_strength, app.roll_swing);
            let which = app.selected_notes.clone();
            app.checkpoint("Quantize");
            if let Some(x) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)?.regions.get_mut(r)) {
                lg_core::functions::quantize(&mut x.notes, &which, region.start, grid, strength, swing);
            }
            app.after_edit();
        }
    }
    y += ROW;
    for (k, l) in ["Strength", "Swing"].into_iter().enumerate() {
        label(&p, area, y, l);
        let mut v = if k == 0 { app.roll_strength } else { app.roll_swing } as f32;
        if value(ui, slot(area, y), id.with(("qs", k)), &mut v, 0.0, 100.0, &|v| format!("{v:.0}")) {
            if k == 0 {
                app.roll_strength = v as u8;
            } else {
                app.roll_swing = v as u8;
            }
        }
        y += ROW;
    }
    label(&p, area, y, "Scale Quantize");
    let key = app.project.as_ref().map(|pr| pr.key.clone()).unwrap_or_else(|| "C major".into());
    let s = slot(area, y);
    p.text(pos2(s.min.x + 4.0, s.center().y), egui::Align2::LEFT_CENTER, &key, look::regular(12.0), look::text_dim());
    let sq = Rect::from_min_max(pos2(s.max.x - 26.0, s.min.y), s.max);
    if ui.put(sq, egui::Button::new("Q")).on_hover_text("Move the selected notes onto the key's scale").clicked() {
        if let Some((t, r)) = sel {
            let scale = crate::pianoroll::scale_of(&key);
            let which = app.selected_notes.clone();
            app.checkpoint("Scale Quantize");
            if let Some(x) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)?.regions.get_mut(r)) {
                crate::scoreattr::scale_quantize(&mut x.notes, &which, &scale);
            }
            app.after_edit();
        }
    }
    y += ROW;
    // Velocity: the selected notes' (the first's shown; a change moves them all by as much)
    label(&p, area, y, "Velocity");
    if let (Some((t, r)), Some(region)) = (sel, notes_of(app)) {
        let which: Vec<usize> = app.selected_notes.clone();
        if let Some(first) = which.first().and_then(|&i| region.notes.get(i)).map(|n| n.velocity) {
            let mut v = first as f32;
            if value(ui, slot(area, y), id.with("vel"), &mut v, 1.0, 127.0, &|v| format!("{v:.0}")) {
                let d = v as i32 - first as i32;
                app.checkpoint("Change Velocity");
                if let Some(x) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)?.regions.get_mut(r)) {
                    for &i in &which {
                        if let Some(n) = x.notes.get_mut(i) {
                            n.velocity = (n.velocity as i32 + d).clamp(1, 127) as u8;
                        }
                    }
                }
                app.after_edit();
            }
        }
    }
    y += ROW + 6.0;
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    y + 1.0
}

/// The Insert Defaults (no region selected): Quantize (with Default: from the LCD division),
/// Interpretation, Syncopation, No Overlap, Max. Dots; no Style (a track's).
fn defaults_rows(_app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32) -> f32 {
    section(y);
    let p = ui.painter().clone();
    let id = ui.scope_id().with("si-defaults");
    let mut d = crate::scoreattr::defaults();
    let before = d.clone();
    let mut qn: Vec<String> = vec!["Default".to_string()];
    qn.extend(nt::Quant::CHOICES.iter().map(|q| q.0.to_string()));
    let cur = if d.quantize.is_empty() { 0 } else { nt::Quant::CHOICES.iter().position(|q| q.0 == d.quantize).map_or(0, |k| k + 1) };
    label(&p, area, y, "Quantize");
    if let Some(k) = popup(ui, slot(area, y), id.with("q"), cur, &qn) {
        d.quantize = if k == 0 { String::new() } else { nt::Quant::CHOICES[k - 1].0.to_string() };
    }
    y += ROW;
    for (l, v) in [("Interpretation", &mut d.interp), ("Syncopation", &mut d.synco), ("No Overlap", &mut d.no_overlap)] {
        label(&p, area, y, l);
        check(ui, slot(area, y), id.with(l), v);
        y += ROW;
    }
    label(&p, area, y, "Max. Dots");
    let mut m = d.max_dots as f32;
    if value(ui, slot(area, y), id.with("dots"), &mut m, 0.0, 3.0, &|v| format!("{v:.0}")) {
        d.max_dots = m as u8;
    }
    if d != before {
        crate::scoreattr::set_defaults(d);
    }
    y += ROW + 6.0;
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    y + 1.0
}

fn region_rows(app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32, t: usize) -> f32 {
    section(y);
    let p = ui.painter().clone();
    let id = ui.scope_id().with(("si-r", t));
    let tid = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(0, |t| t.extra.id);
    // Style
    let style = score::style_of(app, t);
    let own = app.project.as_ref().and_then(|p| p.score.track(tid)).is_some_and(|x| !x.style.is_empty());
    // (every staff style: Logic's and the project's own, mapped ones with a #)
    let all = crate::scorestyle::all(app);
    let mut names: Vec<String> = vec![format!("Auto ({})", style.name)];
    names.extend(all.iter().map(|s| s.shown_name()));
    let cur = if own { all.iter().position(|s| s.name == style.name).map_or(0, |k| k + 1) } else { 0 };
    label(&p, area, y, "Style");
    if let Some(k) = popup(ui, slot(area, y), id.with("style"), cur, &names) {
        let name = if k == 0 { String::new() } else { all[k - 1].name.clone() };
        score::set_style(app, t, &name);
    }
    y += ROW;
    // Quantize
    let (q, interp, synco) = score::quant_of(app, tid);
    let qn: Vec<String> = nt::Quant::CHOICES.iter().map(|q| q.0.to_string()).collect();
    label(&p, area, y, "Quantize");
    if let Some(k) = popup(ui, slot(area, y), id.with("q"), q, &qn) {
        score::region_param(app, t, "Quantize", |ts| ts.quantize = if k == 9 { String::new() } else { nt::Quant::CHOICES[k].0.to_string() });
    }
    y += ROW;
    label(&p, area, y, "Interpretation");
    let mut i = interp;
    if check(ui, slot(area, y), id.with("interp"), &mut i) {
        score::region_param(app, t, "Interpretation", |ts| ts.no_interpretation = !i);
    }
    y += ROW;
    label(&p, area, y, "Syncopation");
    let mut s = synco;
    if check(ui, slot(area, y), id.with("synco"), &mut s) {
        score::region_param(app, t, "Syncopation", |ts| ts.syncopation = s);
    }
    y += ROW;
    label(&p, area, y, "No Overlap");
    let mut no = app.project.as_ref().and_then(|p| p.score.track(tid)).is_none_or(|x| !x.overlap);
    if check(ui, slot(area, y), id.with("overlap"), &mut no) {
        score::region_param(app, t, "No Overlap", |ts| ts.overlap = !no);
    }
    y += ROW;
    label(&p, area, y, "Max. Dots");
    let mut d = score::max_dots_of(app, tid) as f32;
    if value(ui, slot(area, y), id.with("dots"), &mut d, 0.0, 3.0, &|v| format!("{v:.0}")) {
        score::region_param(app, t, "Max. Dots", |ts| ts.max_dots = d as u8 + 1);
    }
    y += ROW;
    // Score: Show / Hide (a hidden region is left out above its own display level)
    if let Some((rt, rix)) = app.selected_region.filter(|x| x.0 == t) {
        let start = app.project.as_ref().and_then(|p| p.tracks.get(rt)?.regions.get(rix)).map_or(0, |r| r.start);
        let hidden = app.project.as_ref().and_then(|p| p.score.track(tid)).is_some_and(|x| x.hidden.contains(&start));
        label(&p, area, y, "Score");
        if let Some(k) = popup(ui, slot(area, y), id.with("score"), hidden as usize, &["Show".to_string(), "Hide".to_string()]) {
            crate::scoreattr::set_region_hidden(app, rt, rix, k == 1);
        }
        y += ROW;
    }
    y += 6.0;
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    y + 1.0
}

/// The selected notes: one note's values, or (several) the first one's, changed for all by the
/// same amount.
fn note_rows(app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32) -> f32 {
    section(y);
    let p = ui.painter().clone();
    let Some((t, r)) = app.selected_region else { return y };
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return y };
    let Some(&first) = app.selected_notes.first() else { return y };
    let Some(n) = rg.notes.get(first).copied() else { return y };
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let id = ui.scope_id().with(("si-n", t, r));
    let rows: [(&str, f32, f32, f32, Box<dyn Fn(f32) -> String>); 4] = [
        ("MIDI Channel", n.channel as f32 + 1.0, 1.0, 16.0, Box::new(|v: f32| format!("{v:.0}"))),
        ("Pitch", n.pitch as f32, 0.0, 127.0, Box::new(|v: f32| crate::app::note_name(v as u8))),
        ("Velocity", n.velocity as f32, 1.0, 127.0, Box::new(|v: f32| format!("{v:.0}"))),
        ("Length", n.len as f32, 1.0, 3840.0 * 64.0, Box::new(move |v: f32| crate::app::bbdt_len(v as u32, sig))),
    ];
    let sel_notes: Vec<lg_core::events::Note> = app.selected_notes.iter().filter_map(|&i| rg.notes.get(i)).copied().collect();
    for (k, (name, mut v, min, max, shown)) in rows.into_iter().enumerate() {
        label(&p, area, y, name);
        // (an asterisk when the selected notes differ; a drag changes them all by the same amount)
        let of = |n: &lg_core::events::Note| match k {
            0 => n.channel as u32,
            1 => n.pitch as u32,
            2 => n.velocity as u32,
            _ => n.len,
        };
        let differ = sel_notes.iter().any(|x| of(x) != of(&n));
        let shown: Box<dyn Fn(f32) -> String> = if differ { Box::new(|_| "*".to_string()) } else { shown };
        let before = v;
        // (Length moves by sixteenths when dragged)
        let changed = if k == 3 {
            let mut steps = v / 240.0;
            let c = value(ui, slot(area, y), id.with(k), &mut steps, 0.0, 1024.0, &|s| shown(s * 240.0));
            v = (steps * 240.0).max(1.0);
            c
        } else {
            value(ui, slot(area, y), id.with(k), &mut v, min, max, &*shown)
        };
        if changed {
            let dv = v - before;
            let sel = app.selected_notes.clone();
            app.checkpoint(name);
            if let Some(region) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                for i in sel {
                    if let Some(x) = region.notes.get_mut(i) {
                        match k {
                            0 => x.channel = (x.channel as f32 + dv).clamp(0.0, 15.0) as u8,
                            1 => x.pitch = (x.pitch as f32 + dv).clamp(0.0, 127.0) as u8,
                            2 => x.velocity = (x.velocity as f32 + dv).clamp(1.0, 127.0) as u8,
                            _ => x.len = (x.len as f32 + dv).max(1.0) as u32,
                        }
                    }
                }
            }
            app.after_edit();
        }
        y += ROW;
    }
    y += 6.0;
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    y + 1.0
}

/// A selected text object: Staff, Style, Align, Vertical / Horizontal Pos, Lyric.
fn text_rows(app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32, st: &mut score::State) -> f32 {
    section(y);
    let p = ui.painter().clone();
    let Some((tid, si)) = st.sel_symbol else { return y };
    let Some(sym) = app.project.as_ref().and_then(|p| p.score.track(tid)?.symbols.get(si)).cloned() else {
        st.sel_symbol = None;
        return y;
    };
    let styles: Vec<String> = app.project.as_ref().map(|p| p.score.styles().into_iter().map(|s| s.name).collect()).unwrap_or_default();
    let cur_style = app.project.as_ref().map(|p| p.score.style_for(&sym).name).unwrap_or_default();
    let id = ui.scope_id().with(("si-t", tid, si));
    let edit = |app: &mut App, what: &str, f: &dyn Fn(&mut lg_core::score::Symbol)| {
        app.checkpoint(what);
        if let Some(s) = app.project.as_mut().and_then(|p| p.score.track_mut(tid).symbols.get_mut(si)) {
            f(s);
        }
        app.after_edit();
    };
    label(&p, area, y, "Staff");
    let mut v = sym.staff as f32 + 1.0;
    if value(ui, slot(area, y), id.with("staff"), &mut v, 1.0, 2.0, &|v| format!("{v:.0}")) {
        edit(app, "Staff", &|s| s.staff = v as u8 - 1);
    }
    y += ROW;
    label(&p, area, y, "Style");
    if let Some(k) = popup(ui, slot(area, y), id.with("style"), styles.iter().position(|s| *s == cur_style).unwrap_or(0), &styles) {
        let name = styles[k].clone();
        edit(app, "Text Style", &|s| s.style = name.clone());
    }
    y += ROW;
    label(&p, area, y, "Align");
    let aligns: Vec<String> = ALIGNS.iter().map(|s| s.to_string()).collect();
    if let Some(k) = popup(ui, slot(area, y), id.with("align"), sym.align as usize, &aligns) {
        edit(app, "Align", &|s| s.align = k as u8);
    }
    y += ROW;
    for (k, name) in ["Vertical Pos", "Horizontal Pos"].into_iter().enumerate() {
        label(&p, area, y, name);
        let mut v = if k == 0 { sym.dy } else { sym.dx } as f32;
        if value(ui, slot(area, y), id.with(name), &mut v, -400.0, 400.0, &|v| format!("{v:.0}")) {
            edit(app, name, &|s| if k == 0 { s.dy = v as i16 } else { s.dx = v as i16 });
        }
        y += ROW;
    }
    label(&p, area, y, "Lyric");
    let mut lyric = sym.kind == lg_core::score::SymbolKind::Lyric;
    if matches!(sym.kind, lg_core::score::SymbolKind::Text | lg_core::score::SymbolKind::Lyric) && check(ui, slot(area, y), id.with("lyric"), &mut lyric) {
        edit(app, "Lyric", &|s| s.kind = if lyric { lg_core::score::SymbolKind::Lyric } else { lg_core::score::SymbolKind::Text });
    }
    y += ROW + 6.0;
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    y + 1.0
}

// ---- the Part box --------------------------------------------------------------------------

/// A group button's icon.
fn group_icon(p: &egui::Painter, r: Rect, g: usize, col: Color32) {
    let c = r.center();
    match g {
        0 => {
            score::note_icon(p, Rect::from_center_size(c - vec2(3.0, 0.0), vec2(16.0, r.height() + 4.0)), nt::Value { base: 8, dots: 0 }, col);
            p.text(c + vec2(7.0, -6.0), Align2::CENTER_CENTER, "3", look::regular(9.0), col);
        }
        1 => score::smufl(p, '\u{E650}', c + vec2(0.0, 6.0), 5.4, col, true),
        2 => score::smufl(p, score::FCLEF, c - vec2(1.0, 4.0), 5.0, col, true),
        3 => score::smufl_text(p, &score::dynamic_glyphs("fp"), c + vec2(0.0, 6.0), 5.0, col),
        4 => {
            score::smufl(p, '\u{E0A9}', c + vec2(-5.0, -4.0), 3.6, col, true);
            score::smufl(p, '\u{E0DB}', c + vec2(5.0, 4.0), 3.6, col, true);
        }
        5 => {
            score::artic_glyph(p, 4, c - vec2(0.0, 5.0), 6.0, true, col);
            score::artic_glyph(p, 2, c + vec2(0.0, 5.0), 6.0, true, col);
        }
        6 => {
            let (a, z) = (pos2(r.min.x + 9.0, c.y + 3.0), pos2(r.max.x - 9.0, c.y + 3.0));
            p.add(egui::Shape::line(score::bezier([a, a + vec2(4.0, -10.0), z + vec2(-4.0, -10.0), z], 12), Stroke::new(2.0, col)));
        }
        7 => {
            for k in 0..3 {
                score::smufl(p, '\u{E260}', c + vec2(-7.0 + k as f32 * 6.0, 3.0 + (k as f32 - 1.0) * 2.5), 4.2, col, true);
            }
        }
        8 => {
            score::smufl(p, '\u{E084}', c - vec2(0.0, 5.0), 4.4, col, true);
            score::smufl(p, '\u{E084}', c + vec2(0.0, 4.5), 4.4, col, true);
        }
        9 => {
            let r2 = Rect::from_center_size(c, vec2(12.0, 19.0));
            p.vline(r2.min.x + 1.0, r2.y_range(), Stroke::new(2.4, col));
            p.vline(r2.min.x + 5.0, r2.y_range(), Stroke::new(1.0, col));
            p.circle_filled(pos2(r2.min.x + 9.0, c.y - 3.0), 1.4, col);
            p.circle_filled(pos2(r2.min.x + 9.0, c.y + 3.0), 1.4, col);
        }
        10 => score::smufl(p, '\u{E566}', c + vec2(0.0, 7.0), 6.4, col, true),
        11 => score::smufl(p, '\u{E4E5}', c, 5.0, col, true),
        12 => {
            p.text(c, Align2::CENTER_CENTER, "A", egui::FontId::new(20.0, egui::FontFamily::Proportional), col);
        }
        _ => {
            p.text(c, Align2::CENTER_CENTER, "¶", look::regular(20.0), col);
        }
    }
}

/// A symbol cell's picture.
fn symbol_icon(p: &egui::Painter, r: Rect, part: Part, dotted: bool, triplet: bool, col: Color32) {
    let c = r.center();
    match part {
        Part::Note(i) => {
            note_down(p, r, nt::Value { base: score::VALUES[i].1, dots: dotted as u8 }, col, if triplet { 8.0 } else { 0.0 });
            if triplet {
                p.text(pos2(c.x + 1.0, r.min.y), Align2::CENTER_TOP, "3", look::regular(9.0), col);
            }
        }
        Part::Rest(i) => score::rest(p, c.x - 2.0, c.y + 8.0, 4.0, nt::Value { base: score::VALUES[i].1, dots: 0 }, false, col),
        Part::Artic(i) => score::artic_glyph(p, score::PART_ARTICS[i], c + vec2(0.0, 2.0), 4.4, true, col),
        Part::Dynamic(i) => score::smufl_text(p, &score::dynamic_glyphs(score::DYNAMICS[i]), c + vec2(0.0, 4.0), 3.4, col),
        Part::Line(i) => {
            let (a, z) = (pos2(r.min.x + 3.0, c.y), pos2(r.max.x - 3.0, c.y));
            match i {
                0 | 1 => {
                    let d = if i == 0 { -6.0 } else { 6.0 };
                    p.add(egui::Shape::line(score::bezier([a, a + vec2(4.0, d), z + vec2(-4.0, d), z], 12), Stroke::new(1.3, col)));
                }
                2 => {
                    p.line_segment([a, z - vec2(0.0, 4.0)], Stroke::new(1.1, col));
                    p.line_segment([a, z + vec2(0.0, 4.0)], Stroke::new(1.1, col));
                }
                _ => {
                    p.line_segment([a - vec2(0.0, 4.0), z], Stroke::new(1.1, col));
                    p.line_segment([a + vec2(0.0, 4.0), z], Stroke::new(1.1, col));
                }
            }
        }
        Part::Clef(i) => {
            let ch = [score::GCLEF, score::FCLEF, score::CCLEF, score::CCLEF, score::PERC_CLEF][i];
            score::smufl(p, ch, c + vec2(0.0, [5.0, -2.0, 1.0, -2.0, 1.0][i]), 2.6, col, true);
        }
        Part::Text(7) => {
            // a small chord grid
            let g = lg_core::chordgrid::Grid::shape(0, 0, "x32010");
            crate::scoregrid::draw_grid(p, Rect::from_center_size(c + vec2(0.0, 3.0), vec2(12.0, 12.0)), &g, 4, false, false, col, look::pane_bg(), (None, None));
        }
        Part::Text(i) => {
            let s = ["A", "Lyr", "C7", "REG", "INS", "SNG", "DAT"][i];
            p.text(c, Align2::CENTER_CENTER, s, look::regular(if i == 0 { 15.0 } else { 9.5 }), col);
        }
        Part::Head(i) => {
            if i == 0 {
                score::smufl(p, '\u{E0A4}', c, 3.2, col, true);
            } else {
                score::special_head(p, c, 3.2, 4, i as u8, col);
            }
        }
        Part::Pedal(i) => score::smufl(p, if i == 0 { '\u{E650}' } else { '\u{E655}' }, c + vec2(0.0, 3.0), 2.4, col, true),
        Part::Bar(i) => {
            let k = BarKind::ALL[i];
            match k {
                BarKind::Repeat1 => score::smufl(p, '\u{E500}', c, 2.4, col, true),
                BarKind::Repeat2 => score::smufl(p, '\u{E501}', c, 2.4, col, true),
                BarKind::Ending1 | BarKind::Ending2 => {
                    let (x0, x1, y0) = (r.min.x + 4.0, r.max.x - 3.0, c.y - 5.0);
                    p.line_segment([pos2(x0, y0 + 9.0), pos2(x0, y0)], Stroke::new(1.0, col));
                    p.line_segment([pos2(x0, y0), pos2(x1, y0)], Stroke::new(1.0, col));
                    if k == BarKind::Ending1 {
                        p.line_segment([pos2(x1, y0), pos2(x1, y0 + 9.0)], Stroke::new(1.0, col));
                    }
                    p.text(pos2(x0 + 2.0, y0 + 1.0), Align2::LEFT_TOP, if k == BarKind::Ending1 { "1." } else { "2." }, look::regular(8.0), col);
                }
                BarKind::Segno => score::smufl(p, '\u{E047}', c + vec2(0.0, 1.0), 2.6, col, true),
                BarKind::Coda => score::smufl(p, '\u{E048}', c + vec2(0.0, 1.0), 2.6, col, true),
                _ => {
                    // (a short staff with the bar line on it)
                    let sp = 2.6;
                    let bottom = c.y + 2.0 * sp;
                    for l in 0..5 {
                        p.hline(r.min.x + 2.0..=r.max.x - 2.0, bottom - l as f32 * sp, Stroke::new(0.6, look::mix(col, look::pane_bg(), 0.6)));
                    }
                    let x = match k {
                        BarKind::RepeatStart => c.x - 4.0,
                        BarKind::RepeatEnd | BarKind::Final => c.x + 4.0,
                        _ => c.x,
                    };
                    score_bar_icon(p, k, x, bottom, sp, col);
                }
            }
        }
        Part::Ornament(i) => {
            let sp = 2.6;
            score::ornament(p, ORNAMENTS[i], c.x - 0.6 * sp, r.max.x - 2.0, c.y + 1.6 * sp, c.y + 5.6 * sp, sp, col);
        }
        Part::Key(i) => {
            let f = i as i8 - 7;
            let n = f.unsigned_abs();
            let glyph = if f < 0 { '\u{E260}' } else { '\u{E262}' };
            if n == 0 {
                p.text(c, Align2::CENTER_CENTER, "C", look::regular(12.0), col);
            } else {
                p.text(c + vec2(-4.0, 0.0), Align2::CENTER_CENTER, n.to_string(), look::regular(11.0), col);
                score::smufl(p, glyph, c + vec2(5.0, 0.0), 2.8, col, true);
            }
        }
        Part::Time(i) => {
            if let Some((a, b)) = TIMES[i].split_once('/') {
                p.text(c - vec2(0.0, 5.5), Align2::CENTER_CENTER, a, look::bold(11.0), col);
                p.text(c + vec2(0.0, 5.5), Align2::CENTER_CENTER, b, look::bold(11.0), col);
            }
        }
        Part::Break(i) => {
            let s = ["⏎", "⇟", "⏎̸", "⇟̸"][i];
            p.text(c, Align2::CENTER_CENTER, s, look::regular(13.0), col);
            if i >= 2 {
                p.line_segment([r.left_bottom() + vec2(5.0, -5.0), r.right_top() + vec2(-5.0, 5.0)], Stroke::new(1.2, col));
            }
        }
        Part::NTuplet => {
            // Logic's N-tuplet: a bracket with "n" over two beamed notes
            let y = r.min.y + 4.0;
            p.line_segment([pos2(r.min.x + 4.0, y + 3.0), pos2(r.min.x + 4.0, y)], Stroke::new(1.0, col));
            p.line_segment([pos2(r.min.x + 4.0, y), pos2(r.max.x - 4.0, y)], Stroke::new(1.0, col));
            p.line_segment([pos2(r.max.x - 4.0, y), pos2(r.max.x - 4.0, y + 3.0)], Stroke::new(1.0, col));
            p.rect_filled(Rect::from_center_size(pos2(c.x, y), vec2(8.0, 9.0)), 0.0, look::pane_bg());
            p.text(pos2(c.x, y), Align2::CENTER_CENTER, "n", look::regular(10.0), col);
            for dx in [-4.0, 4.0] {
                score::notehead(p, pos2(c.x + dx, r.max.y - 5.0), 3.6, 8, col, Color32::TRANSPARENT);
                p.vline(c.x + dx + 2.6, r.max.y - 15.0..=r.max.y - 5.0, Stroke::new(1.0, col));
            }
            p.line_segment([pos2(c.x - 1.4, r.max.y - 15.0), pos2(c.x + 6.6, r.max.y - 15.0)], Stroke::new(2.0, col));
        }
    }
}

/// A Part box note value as Logic draws them: the head at the top, the stem down at its left,
/// flags at the stem's end.
fn note_down(p: &egui::Painter, r: Rect, v: nt::Value, col: Color32, pad: f32) {
    let sp = 4.6;
    let c = pos2(r.center().x + 0.2 * sp, r.min.y + 1.2 * sp + pad);
    score::notehead(p, c, sp, v.base, col, Color32::TRANSPARENT);
    if v.base <= 2 {
        p.add(egui::Shape::closed_line(
            (0..24).map(|k| {
                let t = k as f32 / 24.0 * std::f32::consts::TAU;
                let (rx, ry, a) = (if v.base == 1 { 0.8 * sp } else { 0.62 * sp }, 0.44 * sp, if v.base == 1 { 0.0f32 } else { -0.35 });
                let (x, y) = (rx * t.cos(), ry * t.sin());
                pos2(c.x + x * a.cos() - y * a.sin(), c.y + x * a.sin() + y * a.cos())
            }).collect(),
            Stroke::new(1.2, col),
        ));
    }
    if v.base > 1 {
        let x = c.x - 0.6 * sp;
        let end = (c.y + 5.4 * sp + v.flags().saturating_sub(2) as f32 * 0.7 * sp).min(r.max.y - 1.0);
        p.line_segment([pos2(x, c.y + 0.1 * sp), pos2(x, end)], Stroke::new(1.0, col));
        for f in 0..v.flags() {
            score::flag(p, pos2(x, end - f as f32 * 0.7 * sp), sp * 0.85, false, col);
        }
    }
    for k in 0..v.dots {
        p.circle_filled(pos2(c.x + (1.1 + 0.6 * k as f32) * sp, c.y - 0.1 * sp), 0.24 * sp, col);
    }
}

/// A bar sign at small size for the Part box.
fn score_bar_icon(p: &egui::Painter, k: BarKind, x: f32, bottom: f32, sp: f32, col: Color32) {
    let (y0, y1) = (bottom - 4.0 * sp, bottom);
    let thin = Stroke::new(1.0, col);
    let thick = Stroke::new(2.2, col);
    let dots = |dx: f32| {
        for y in [bottom - 1.5 * sp, bottom - 2.5 * sp] {
            p.circle_filled(pos2(x + dx, y), 1.0, col);
        }
    };
    match k {
        BarKind::Double => {
            p.vline(x - 1.5, y0..=y1, thin);
            p.vline(x + 1.5, y0..=y1, thin);
        }
        BarKind::Final => {
            p.vline(x - 3.0, y0..=y1, thin);
            p.vline(x, y0..=y1, thick);
        }
        BarKind::Dashed => {
            let mut y = y0;
            while y < y1 {
                p.vline(x, y..=(y + 1.6).min(y1), thin);
                y += 2.8;
            }
        }
        BarKind::RepeatStart => {
            p.vline(x, y0..=y1, thick);
            p.vline(x + 3.0, y0..=y1, thin);
            dots(5.5);
        }
        BarKind::RepeatEnd => {
            p.vline(x, y0..=y1, thick);
            p.vline(x - 3.0, y0..=y1, thin);
            dots(-5.5);
        }
        BarKind::RepeatBoth => {
            p.vline(x, y0..=y1, thick);
            p.vline(x - 3.0, y0..=y1, thin);
            p.vline(x + 3.0, y0..=y1, thin);
            dots(-5.5);
            dots(5.5);
        }
        _ => {
            p.vline(x, y0..=y1, thin);
        }
    }
}

/// The symbols of a group, in rows (Notes: plain, triplet and dotted values).
fn group_rows(g: usize) -> Vec<Vec<(Part, bool, bool)>> {
    let one = |n: usize, f: &dyn Fn(usize) -> Part| -> Vec<Vec<(Part, bool, bool)>> { vec![(0..n).map(|i| (f(i), false, false)).collect()] };
    match g {
        0 => vec![
            (0..score::VALUES.len()).map(|i| (Part::Note(i), false, false)).collect(),
            (1..7).map(|i| (Part::Note(i), false, true)).chain(std::iter::once((Part::NTuplet, false, false))).collect(),
            (0..7).map(|i| (Part::Note(i), true, false)).collect(),
        ],
        _ => one(Part::GROUPS[g], &|i| Part::from_index(g, i)),
    }
}

fn part_box(app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32, pb: &mut PartBox, st: &mut score::State) -> f32 {
    let p = ui.painter().clone();
    let id = ui.scope_id().with("si-pb");
    // the header: Part Box: [set ⌃⌄]   ⌄
    let hr = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), HEAD));
    p.text(pos2(hr.min.x + 12.0, hr.center().y), Align2::LEFT_CENTER, "Part Box:", look::regular(13.0), look::text_dim());
    let sets = load_sets();
    let all: Vec<usize> = (0..GROUP_NAMES.len()).collect();
    let set_name = if pb.shown == all { "All".to_string() } else { sets.iter().find(|s| s.1 == pb.shown).map_or("Customized".to_string(), |s| s.0.clone()) };
    let br = Rect::from_min_size(pos2(hr.min.x + 72.0, hr.center().y - 11.0), vec2(hr.width() - 72.0 - 46.0, 22.0));
    let resp = ui.interact(br, id.with("sets"), Sense::click());
    p.rect_filled(br, 5.0, if resp.hovered() { look::group_bg() } else { look::mix(look::group_bg(), look::pane_bg(), 0.4) });
    p.text(pos2(br.min.x + 8.0, br.center().y), Align2::LEFT_CENTER, &set_name, look::regular(13.0), look::text());
    updown(&p, pos2(br.max.x - 10.0, br.center().y), look::text());
    egui::Popup::menu(&resp).show(|ui| {
        if ui.button("New Set").clicked() {
            pb.naming = Some((true, String::new()));
            ui.close();
        }
        let user = sets.iter().any(|s| s.1 == pb.shown);
        if ui.add_enabled(user, egui::Button::new("Rename Set")).clicked() {
            pb.naming = Some((false, set_name.clone()));
            ui.close();
        }
        if ui.add_enabled(user, egui::Button::new("Delete Set")).clicked() {
            let mut s = sets.clone();
            s.retain(|x| x.0 != set_name);
            save_sets(&s);
            ui.close();
        }
        ui.separator();
        if ui.add(egui::Button::selectable(set_name == "All", "All")).clicked() {
            pb.shown = all.clone();
            ui.close();
        }
        for (n, g) in &sets {
            if ui.add(egui::Button::selectable(*n == set_name, n.as_str())).clicked() {
                pb.shown = g.clone();
                ui.close();
            }
        }
    });
    let dr = Rect::from_min_size(pos2(hr.max.x - 30.0, hr.min.y), vec2(30.0, HEAD));
    if ui.interact(dr, id.with("disc"), Sense::click()).clicked() {
        pb.open = !pb.open;
    }
    crate::glyphs::paint(&p, Rect::from_center_size(dr.center() - vec2(4.0, 0.0), vec2(14.0, 14.0)), if pb.open { crate::glyphs::ph::CARET_DOWN } else { crate::glyphs::ph::CARET_RIGHT }, 13.0, look::text_dim());
    y += HEAD;
    // naming a set
    if let Some((new, mut name)) = pb.naming.clone() {
        let r = Rect::from_min_size(pos2(area.min.x + 12.0, y), vec2(area.width() - 24.0, 22.0));
        let te = ui.put(r, egui::TextEdit::singleline(&mut name).hint_text("Set name").font(look::regular(13.0)));
        crate::widgets::focus_once(ui, &te);
        pb.naming = Some((new, name.clone()));
        if te.lost_focus() {
            pb.naming = None;
            let name = name.trim().to_string();
            if !name.is_empty() && ui.input(|i| !i.key_pressed(egui::Key::Escape)) {
                let mut s = sets.clone();
                if new {
                    s.retain(|x| x.0 != name);
                    s.push((name, pb.shown.clone()));
                } else if let Some(x) = s.iter_mut().find(|x| x.0 == set_name) {
                    x.0 = name;
                }
                save_sets(&s);
            }
        }
        y += 26.0;
    }
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    if !pb.open {
        return y + 1.0;
    }
    // the group buttons: 7 to a row, All over the first three
    let cols = 7usize;
    let cw = (area.width() - 2.0) / cols as f32;
    let ch = 26.0;
    let mut cells: Vec<(Rect, Option<usize>)> = vec![(Rect::from_min_size(pos2(area.min.x + 1.0, y + 4.0), vec2(cw * 3.0, ch)), None)];
    for g in 0..GROUP_NAMES.len() {
        let k = match g {
            11 => 14,
            12 => 15,
            13 => 19,
            g => g + 3,
        };
        cells.push((Rect::from_min_size(pos2(area.min.x + 1.0 + (k % cols) as f32 * cw, y + 4.0 + (k / cols) as f32 * ch), vec2(cw, ch)), Some(g)));
    }
    for (r, g) in &cells {
        let on = match g {
            None => pb.shown.len() == GROUP_NAMES.len(),
            Some(g) => pb.shown.contains(g),
        };
        let tip = g.map_or("All".to_string(), |g| GROUP_NAMES[g].to_string());
        let resp = ui.interact(*r, id.with(("g", g.map_or(99, |x| x))), Sense::click()).on_hover_text(tip);
        let inner = r.shrink(1.0);
        // (each group button sits on its own darker tile)
        if g.is_some() {
            p.rect_filled(inner, 2.0, look::mix(look::pane_bg(), Color32::BLACK, 0.22));
        }
        if on {
            p.rect_filled(inner, 3.0, look::accent());
        } else if resp.hovered() {
            p.rect_filled(inner, 3.0, look::group_bg());
        }
        let col = if on { Color32::WHITE } else { look::text() };
        match g {
            None => {
                p.text(inner.center(), Align2::CENTER_CENTER, "All", look::regular(14.0), col);
            }
            Some(g) => group_icon(&p, inner, *g, col),
        }
        if resp.clicked() {
            match g {
                None => pb.shown = if on { vec![] } else { (0..GROUP_NAMES.len()).collect() },
                Some(g) => toggle_group(&mut pb.shown, *g),
            }
        }
    }
    let rows_n = 3;
    y += 8.0 + rows_n as f32 * ch;
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    y += 6.0;
    // the shown groups, each with its handle at the left
    let x0 = area.min.x + 26.0;
    let mut blocks: Vec<(usize, f32, f32)> = Vec::new();
    let shown = pb.shown.clone();
    for &g in &shown {
        let top = y;
        // (Logic's notes: tall narrow cells; other symbols square)
        let (sw, cell_h) = if g == 0 { (17.0, 44.0) } else { (24.0, 26.0) };
        let per_row = ((area.width() - 32.0) / sw).floor().max(1.0) as usize;
        for row in group_rows(g) {
            for (k, (part, dotted, triplet)) in row.into_iter().enumerate() {
                let r = Rect::from_min_size(pos2(x0 + (k % per_row) as f32 * sw, y + (k / per_row) as f32 * cell_h), vec2(sw - 1.0, cell_h - 2.0));
                let on = st.part == part && (g != 0 || (st.dotted == dotted && st.triplet == triplet));
                let resp = ui.interact(r, id.with(("s", g, part.index(), dotted, triplet)), Sense::click_and_drag()).on_hover_text(part.name());
                if on {
                    p.rect_filled(r, 3.0, look::accent());
                } else if resp.hovered() {
                    p.rect_filled(r, 3.0, look::group_bg());
                }
                symbol_icon(&p, r, part, dotted, triplet, if on { Color32::WHITE } else { look::text() });
                if resp.clicked() || resp.drag_started() {
                    st.part = part;
                    if g == 0 {
                        st.dotted = dotted;
                        st.triplet = triplet;
                    }
                    if resp.clicked() && !app.selected_notes.is_empty() {
                        score::apply_part(app, st, part);
                    }
                }
                resp.dnd_set_drag_payload(part);
            }
            let lines = if g == 0 { 1 } else { Part::GROUPS[g].div_ceil(per_row) };
            y += lines as f32 * cell_h;
        }
        y += 4.0;
        // the handle (≡): drag up or down to reorder
        let hr = Rect::from_min_max(pos2(area.min.x + 2.0, top), pos2(area.min.x + 20.0, y - 4.0));
        let resp = ui.interact(hr, id.with(("h", g)), Sense::drag()).on_hover_text(format!("{}: drag to move the group", GROUP_NAMES[g]));
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let hc = hr.center();
        for dy in [-2.5f32, 0.0, 2.5] {
            p.hline(hc.x - 4.0..=hc.x + 4.0, hc.y + dy, Stroke::new(1.0, look::text_dim()));
        }
        if resp.drag_started() {
            pb.drag = Some(g);
        }
        blocks.push((g, top, y));
        p.hline(area.min.x + 8.0..=area.max.x - 8.0, y - 2.0, Stroke::new(0.5, look::mix(look::line(), look::pane_bg(), 0.5)));
    }
    if let Some(g) = pb.drag {
        if let Some(pp) = ui.input(|i| i.pointer.interact_pos()) {
            let to = blocks.iter().filter(|b| b.0 != g).take_while(|b| (b.1 + b.2) / 2.0 < pp.y).count();
            let ly = blocks.iter().filter(|b| b.0 != g).nth(to).map_or(y, |b| b.1);
            p.hline(area.min.x + 4.0..=area.max.x - 4.0, ly - 1.0, Stroke::new(2.0, look::accent()));
            if !ui.input(|i| i.pointer.primary_down()) {
                move_group(&mut pb.shown, g, to);
                pb.drag = None;
            }
        } else {
            pb.drag = None;
        }
    }
    // a symbol being dragged into the score: drawn at the pointer
    if let (Some(part), Some(pp)) = (egui::DragAndDrop::payload::<Part>(ui.ctx()), ui.input(|i| i.pointer.hover_pos())) {
        let lp = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("lg-pb-drag")));
        let r = Rect::from_center_size(pp + vec2(14.0, 14.0), vec2(22.0, 28.0));
        lp.rect_filled(r, 4.0, look::mix(look::pane_bg(), Color32::BLACK, 0.1));
        symbol_icon(&lp, r, *part, st.dotted, st.triplet, look::text());
    }
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    y + 1.0
}

// ---- what the groups insert ----------------------------------------------------------------

/// A sustain pedal event (controller 64).
pub fn is_pedal(e: &lg_core::events::Control) -> bool {
    e.kind & 0xF0 == 0xB0 && e.number == 64
}

/// A pedal marking into the region at `tick`: down, or up when the pedal before it is down
/// (markings alternate). It is a controller 64 event, so it plays.
pub fn insert_pedal(app: &mut App, t: usize, tick: u32) {
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return };
    let Some(rix) = tr.regions.iter().position(|r| r.audio.is_none() && tick >= r.start && tick < r.start + r.length) else {
        app.tell("Pedal markings go into a MIDI region");
        return;
    };
    let rel = tick - tr.regions[rix].start;
    let channel = tr.regions[rix].notes.first().map_or(0, |n| n.channel);
    app.checkpoint("Insert Pedal Marking");
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(rix)) {
        rg.ext.events.retain(|e| !(is_pedal(e) && e.pos == rel));
        let down = pedal_down_before(&rg.ext.events, rel);
        rg.ext.events.push(lg_core::events::Control { kind: 0xB0 | channel, channel, pos: rel, number: 64, value: if down { 0 } else { 127 } });
        rg.ext.events.sort_by_key(|e| e.pos);
    }
    app.after_edit();
    app.load_engine();
}

/// Whether the pedal is down just before `pos`.
pub fn pedal_down_before(events: &[lg_core::events::Control], pos: u32) -> bool {
    events.iter().filter(|e| is_pedal(e) && e.pos < pos).last().is_some_and(|e| e.value >= 64)
}

/// Where a bar sign dropped at `tick` goes: bar lines and repeats near a bar line on it (else
/// on the display grid), bar repeats and endings at their bar's start, segno and coda on the grid.
pub fn bar_snap(tick: f32, bar_t: u32, grid: u32, kind: BarKind) -> u32 {
    let bar = bar_t as f32;
    let near = (tick / bar).round() * bar;
    match kind {
        BarKind::Repeat1 | BarKind::Repeat2 | BarKind::Ending1 | BarKind::Ending2 => ((tick / bar).floor() * bar).max(0.0) as u32,
        k if k.is_line() && (tick - near).abs() <= bar / 8.0 => near.max(0.0) as u32,
        _ => ((tick / grid.max(1) as f32).round() * grid.max(1) as f32).max(0.0) as u32,
    }
}

/// A Bars and Repeats sign into the score (global; one undo step). Endings get their default
/// text and a bar's length.
pub fn insert_bar(app: &mut App, tick: u32, kind: BarKind) {
    let bar_t = app.bar_ticks();
    let (text, len) = match kind {
        BarKind::Ending1 => ("1.".to_string(), bar_t),
        BarKind::Ending2 => ("2.".to_string(), bar_t),
        _ => (String::new(), 0),
    };
    app.checkpoint(&format!("Insert {}", kind.name()));
    if let Some(p) = app.project.as_mut() {
        p.score.add_bar(BarSym { tick, kind, text, len });
    }
    app.after_edit();
}

/// A line or page break symbol before a bar (another at the same bar is replaced).
pub fn insert_break(app: &mut App, bar: u32, kind: Break) {
    app.checkpoint(&format!("Insert {}", kind.name()));
    if let Some(p) = app.project.as_mut() {
        p.score.breaks.insert(bar, kind);
    }
    app.after_edit();
}

/// A time signature at a bar (0 = the project's own); the undefined one asks for its values.
pub fn insert_time(app: &mut App, bar: u32, k: usize) {
    let Some((a, b)) = TIMES[k].split_once('/') else { return };
    let (Ok(n), Ok(d)) = (a.parse::<u8>(), b.parse::<u8>()) else {
        open_dialog(&app.ctx.clone(), Dialog::Time { bar, beats: "4".into(), value: 2 });
        return;
    };
    set_time(app, bar, (n, d));
}

fn set_time(app: &mut App, bar: u32, sig: (u8, u8)) {
    app.checkpoint("Insert Time Signature");
    if let Some(p) = app.project.as_mut() {
        lg_core::timeline::set_signature(p, bar, sig);
    }
    app.after_edit();
    app.load_engine();
    if bar > 0 {
        app.tell("Time signature change added: the Score lays its bars out in the first signature");
    }
}

/// Delete with a Score symbol selected (text, bar sign, pedal marking): it goes (one undo step).
pub fn delete_selected(app: &mut App) -> bool {
    let ctx = app.ctx.clone();
    if !active(app, &ctx) {
        return false;
    }
    let st = score::state(&ctx);
    if let Some((tid, si)) = st.sel_symbol {
        app.checkpoint("Delete");
        if let Some(p) = app.project.as_mut() {
            let ts = p.score.track_mut(tid);
            if si < ts.symbols.len() {
                ts.symbols.remove(si);
            }
        }
        app.after_edit();
    } else if let Some(bi) = st.sel_bar {
        app.checkpoint("Delete");
        if let Some(p) = app.project.as_mut() {
            if bi < p.score.bars.len() {
                p.score.bars.remove(bi);
            }
        }
        app.after_edit();
    } else if let Some((t, r, e)) = st.sel_pedal {
        app.checkpoint("Delete");
        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            if e < rg.ext.events.len() {
                rg.ext.events.remove(e);
            }
        }
        app.after_edit();
        app.load_engine();
    } else {
        return false;
    }
    score::with_state(&ctx, |s| {
        s.sel_symbol = None;
        s.sel_bar = None;
        s.sel_pedal = None;
    });
    true
}

// ---- commands ------------------------------------------------------------------------------

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    match name {
        "Text Styles…" => open_dialog(ctx, Dialog::Styles { sel: 0, renaming: None }),
        "Show Score Sets" | "Show/Hide Score Sets Window" => {
            let open = ctx.data(|m| m.get_temp::<Option<Dialog>>(dlg_id())).flatten().is_some_and(|d| matches!(d, Dialog::Sets { .. }));
            if open && name.starts_with("Show/Hide") {
                ctx.data_mut(|m| m.insert_temp::<Option<Dialog>>(dlg_id(), None));
            } else {
                let sel = score::state(ctx).set;
                open_dialog(ctx, Dialog::Sets { sel, renaming: None });
            }
        }
        "Reset Line Layout" => {
            app.checkpoint("Reset Line Layout");
            if let Some(p) = app.project.as_mut() {
                p.score.breaks.retain(|_, k| matches!(k, Break::Page | Break::NoPage));
            }
            app.after_edit();
        }
        // (the Score's inspector is the main window's while the Score is shown there)
        "Show Local Inspector" | "Hide Local Inspector" | "Show/Hide Local Inspector" if active(app, ctx) => app.show_inspector = !app.show_inspector,
        _ => return false,
    }
    true
}

/// A bar line's Control-click menu: line and page breaks there.
pub fn bar_menu(app: &mut App, ui: &mut egui::Ui, bar: u32) {
    let cur = app.project.as_ref().and_then(|p| p.score.breaks.get(&bar).copied());
    for k in Break::ALL {
        if ui.add(egui::Button::selectable(cur == Some(k), format!("Add {}", k.name()))).clicked() {
            insert_break(app, bar, k);
            ui.close();
        }
    }
    if ui.add_enabled(cur.is_some(), egui::Button::new("Delete Break")).clicked() {
        app.checkpoint("Delete Break");
        if let Some(p) = app.project.as_mut() {
            p.score.breaks.remove(&bar);
        }
        app.after_edit();
        ui.close();
    }
    if ui.button("Reset Line Layout").clicked() {
        command(app, &ui.ctx().clone(), "Reset Line Layout");
        ui.close();
    }
}

// ---- windows: Key Signature, Time Signature, Text Styles ------------------------------------

#[derive(Clone, Debug)]
pub enum Dialog {
    /// The key at a tick: (fifths, minor).
    Key { at: u32, fifths: i8, minor: bool },
    /// The time signature at a bar: beats typed, note value (index into VALUES).
    Time { bar: u32, beats: String, value: usize },
    Styles { sel: usize, renaming: Option<String> },
    /// The Score Sets window: the set selected (None: All Instruments), a name being typed.
    Sets { sel: Option<usize>, renaming: Option<String> },
}

fn dlg_id() -> egui::Id {
    egui::Id::unique("lg-score-dialog")
}

pub fn open_dialog(ctx: &egui::Context, d: Dialog) {
    ctx.data_mut(|m| m.insert_temp(dlg_id(), Some(d)));
}

const NOTE_VALUES: [(&str, u8); 6] = [("1/1", 1), ("1/2", 2), ("1/4", 4), ("1/8", 8), ("1/16", 16), ("1/32", 32)];
const MAJOR: [&str; 15] = ["C♭", "G♭", "D♭", "A♭", "E♭", "B♭", "F", "C", "G", "D", "A", "E", "B", "F♯", "C♯"];
const MINOR: [&str; 15] = ["A♭", "E♭", "B♭", "F", "C", "G", "D", "A", "E", "B", "F♯", "C♯", "G♯", "D♯", "A♯"];

pub fn windows(app: &mut App, ctx: &egui::Context, _st: &mut score::State) {
    let Some(mut d) = ctx.data(|m| m.get_temp::<Option<Dialog>>(dlg_id())).flatten() else { return };
    let mut open = true;
    let mut close = false;
    match &mut d {
        Dialog::Key { at, fifths, minor } => {
            egui::Window::new("Key Signature").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Key:");
                    let names = if *minor { MINOR } else { MAJOR };
                    egui::ComboBox::from_id_salt("ks-key").selected_text(names[(*fifths + 7) as usize]).show_ui(ui, |ui| {
                        for (k, n) in names.iter().enumerate() {
                            ui.selectable_value(fifths, k as i8 - 7, *n);
                        }
                    });
                    ui.selectable_value(minor, false, "Major");
                    ui.selectable_value(minor, true, "Minor");
                });
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("OK").clicked() {
                        score::insert_key(app, *at, *fifths);
                        if *at == 0 {
                            if let Some(p) = app.project.as_mut() {
                                p.key = if *minor { format!("{} minor", MINOR[(*fifths + 7) as usize]) } else { format!("{} major", MAJOR[(*fifths + 7) as usize]) };
                            }
                        }
                        close = true;
                    }
                });
            });
        }
        Dialog::Time { bar, beats, value } => {
            egui::Window::new("Time Signature").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Number of beats:");
                    ui.add(egui::TextEdit::singleline(beats).desired_width(40.0));
                    ui.label("Note Value:");
                    egui::ComboBox::from_id_salt("ts-val").selected_text(NOTE_VALUES[*value].0).show_ui(ui, |ui| {
                        for (k, v) in NOTE_VALUES.iter().enumerate() {
                            ui.selectable_value(value, k, v.0);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("OK").clicked() {
                        // (A+B: the beats added up)
                        let n: u32 = beats.split('+').filter_map(|x| x.trim().parse::<u32>().ok()).sum();
                        if n > 0 {
                            set_time(app, *bar, (n.min(99) as u8, NOTE_VALUES[*value].1));
                        }
                        close = true;
                    }
                });
            });
        }
        Dialog::Sets { sel, renaming } => {
            egui::Window::new("Score Sets").collapsible(false).resizable(true).default_size(vec2(560.0, 320.0)).open(&mut open).show(ctx, |ui| {
                sets_window(app, ui, sel, renaming);
            });
        }
        Dialog::Styles { sel, renaming } => {
            egui::Window::new("Text Styles").collapsible(false).resizable(true).default_size(vec2(420.0, 440.0)).open(&mut open).show(ctx, |ui| {
                styles_window(app, ui, sel, renaming);
            });
        }
    }
    let keep = open && !close;
    ctx.data_mut(|m| m.insert_temp(dlg_id(), keep.then_some(d)));
}

/// The Text Styles window: every style (defaults first, then the user's), its name and an
/// example in its font; New Text Style; a double-clicked name renames a user style; the
/// selected style's font, size and attributes below.
fn styles_window(app: &mut App, ui: &mut egui::Ui, sel: &mut usize, renaming: &mut Option<String>) {
    let styles = app.project.as_ref().map(|p| p.score.styles()).unwrap_or_default();
    let n_default = lg_core::score::DEFAULT_STYLES.len();
    ui.horizontal(|ui| {
        ui.menu_button("New", |ui| {
            if ui.button("New Text Style").clicked() {
                let mut k = 1;
                while styles.iter().any(|s| s.name == format!("Style {k}")) {
                    k += 1;
                }
                app.checkpoint("New Text Style");
                if let Some(p) = app.project.as_mut() {
                    p.score.set_style(TextStyle { name: format!("Style {k}"), family: String::new(), size: 12.0, bold: false, italic: false, underline: false });
                }
                app.after_edit();
                *sel = styles.len();
                ui.close();
            }
        });
    });
    ui.separator();
    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        egui::Grid::new("ts-grid").num_columns(2).spacing(vec2(16.0, 4.0)).striped(true).show(ui, |ui| {
            ui.label(egui::RichText::new("Name").strong());
            ui.label(egui::RichText::new("Example").strong());
            ui.end_row();
            for (k, s) in styles.iter().enumerate() {
                if renaming.is_some() && *sel == k {
                    let mut t = renaming.clone().unwrap_or_default();
                    let te = ui.text_edit_singleline(&mut t);
                    crate::widgets::focus_once(ui, &te);
                    *renaming = Some(t.clone());
                    if te.lost_focus() {
                        let name = t.trim().to_string();
                        if !name.is_empty() && !styles.iter().any(|x| x.name == name) {
                            app.checkpoint("Rename Text Style");
                            if let Some(p) = app.project.as_mut() {
                                let old = s.name.clone();
                                if let Some(x) = p.score.text_styles.iter_mut().find(|x| x.name == old) {
                                    x.name = name.clone();
                                }
                                for ts in p.score.tracks.values_mut() {
                                    for sym in ts.symbols.iter_mut().filter(|y| y.style == old) {
                                        sym.style = name.clone();
                                    }
                                }
                            }
                            app.after_edit();
                        }
                        *renaming = None;
                    }
                } else {
                    let r = ui.add(egui::Button::selectable(*sel == k, s.name.as_str()));
                    if r.clicked() {
                        *sel = k;
                    }
                    // (default styles keep their names)
                    if r.double_clicked() && k >= n_default {
                        *sel = k;
                        *renaming = Some(s.name.clone());
                    }
                }
                let font = score::style_font(s, 7.0);
                let mut rt = egui::RichText::new("Example").font(font);
                if s.italic {
                    rt = rt.italics();
                }
                if s.underline {
                    rt = rt.underline();
                }
                if ui.add(egui::Label::new(rt).sense(Sense::click())).clicked() {
                    *sel = k;
                }
                ui.end_row();
            }
        });
    });
    ui.separator();
    // the selected style's font
    let Some(mut s) = styles.get(*sel).cloned() else { return };
    let before = s.clone();
    ui.horizontal(|ui| {
        ui.label(format!("{}:", s.name));
        let fams = ["", "Monospace", "Bravura"];
        egui::ComboBox::from_id_salt("ts-fam").selected_text(if s.family.is_empty() { "Sfera" } else { s.family.as_str() }).show_ui(ui, |ui| {
            for f in fams {
                ui.selectable_value(&mut s.family, f.to_string(), if f.is_empty() { "Sfera" } else { f });
            }
        });
        ui.add(egui::DragValue::new(&mut s.size).range(4.0..=96.0).speed(0.2).suffix(" pt"));
        ui.toggle_value(&mut s.bold, egui::RichText::new("B").strong());
        ui.toggle_value(&mut s.italic, egui::RichText::new("I").italics());
        ui.toggle_value(&mut s.underline, egui::RichText::new("U").underline());
    });
    if s != before {
        app.checkpoint("Text Style");
        if let Some(p) = app.project.as_mut() {
            p.score.set_style(s);
        }
        app.after_edit();
    }
    if *sel >= n_default && ui.button("Delete Style").clicked() {
        let name = styles[*sel].name.clone();
        app.checkpoint("Delete Text Style");
        if let Some(p) = app.project.as_mut() {
            p.score.text_styles.retain(|x| x.name != name);
        }
        app.after_edit();
        *sel = 0;
    }
}

/// A new score set of the project (one undo step); its index.
pub fn new_set(app: &mut App, name: &str, tracks: &[u64]) -> usize {
    app.checkpoint("New Score Set");
    let mut k = 0;
    if let Some(p) = app.project.as_mut() {
        p.score.sets.push(lg_core::score::ScoreSet::new(name, tracks));
        k = p.score.sets.len() - 1;
    }
    app.after_edit();
    k
}

/// The Score Sets window: New ▸ New Empty Set / New Complete Set / Duplicate Set / Delete Set,
/// the sets at the left (double-click renames), the selected set's instruments in score order
/// with Full Name, Short Name, Bracket and Bar Line (joined to the next), moved up and down or
/// removed, an instrument added; Scale (25–250 %). All Instruments can't
/// be edited.
fn sets_window(app: &mut App, ui: &mut egui::Ui, sel: &mut Option<usize>, renaming: &mut Option<String>) {
    let Some(p) = app.project.as_ref() else { return };
    let sets = p.score.sets.clone();
    let tracks: Vec<(u64, String)> = p.tracks.iter().filter(|t| t.kind != lg_core::TrackKind::Audio).map(|t| (t.extra.id, t.name.clone())).collect();
    let edit = |app: &mut App, what: &str, f: &mut dyn FnMut(&mut Vec<lg_core::score::ScoreSet>)| {
        app.checkpoint(what);
        if let Some(p) = app.project.as_mut() {
            f(&mut p.score.sets);
        }
        app.after_edit();
    };
    ui.horizontal(|ui| {
        ui.menu_button("New", |ui| {
            if ui.button("New Empty Set").clicked() {
                *sel = Some(new_set(app, &format!("Score Set {}", sets.len() + 1), &[]));
                ui.close();
            }
            if ui.button("New Complete Set").clicked() {
                let all: Vec<u64> = tracks.iter().map(|t| t.0).collect();
                *sel = Some(new_set(app, "Complete Set", &all));
                ui.close();
            }
            if ui.add_enabled(sel.is_some(), egui::Button::new("Duplicate Set")).clicked() {
                if let Some(s) = sel.and_then(|k| sets.get(k)) {
                    let mut d = s.clone();
                    d.name = format!("{} copy", s.name);
                    edit(app, "Duplicate Set", &mut |v| v.push(d.clone()));
                    *sel = Some(sets.len());
                }
                ui.close();
            }
            if ui.add_enabled(sel.is_some(), egui::Button::new("Delete Set")).clicked() {
                if let Some(k) = *sel {
                    edit(app, "Delete Set", &mut |v| {
                        if k < v.len() {
                            v.remove(k);
                        }
                    });
                    *sel = None;
                    score::with_state(&ui.ctx().clone(), |s| s.set = None);
                }
                ui.close();
            }
        });
    });
    ui.separator();
    ui.horizontal_top(|ui| {
        // the sets
        ui.vertical(|ui| {
            ui.set_width(150.0);
            if ui.add(egui::Button::selectable(sel.is_none(), "All Instruments")).clicked() {
                *sel = None;
            }
            for (k, s) in sets.iter().enumerate() {
                if renaming.is_some() && *sel == Some(k) {
                    let mut t = renaming.clone().unwrap_or_default();
                    let te = ui.text_edit_singleline(&mut t);
                    crate::widgets::focus_once(ui, &te);
                    *renaming = Some(t.clone());
                    if te.lost_focus() {
                        if !t.trim().is_empty() {
                            let name = t.trim().to_string();
                            edit(app, "Rename Set", &mut |v| v[k].name = name.clone());
                        }
                        *renaming = None;
                    }
                    continue;
                }
                let r = ui.add(egui::Button::selectable(*sel == Some(k), s.name.as_str()));
                if r.clicked() {
                    *sel = Some(k);
                }
                if r.double_clicked() {
                    *renaming = Some(s.name.clone());
                }
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            let Some(k) = *sel else {
                ui.label(egui::RichText::new("All Instruments shows every instrument and can't be edited. Choose New ▸ New Complete Set to make one you can edit.").color(look::text_dim()));
                return;
            };
            let Some(set) = sets.get(k).cloned() else { return };
            egui::Grid::new("sets-grid").num_columns(6).spacing(vec2(10.0, 4.0)).striped(true).show(ui, |ui| {
                for h in ["Instrument", "Full Name", "Short Name", "Bracket", "Bar Line", ""] {
                    ui.label(egui::RichText::new(h).strong());
                }
                ui.end_row();
                for (i, e) in set.entries.iter().enumerate() {
                    let tname = tracks.iter().find(|t| t.0 == e.track).map_or("(deleted)".to_string(), |t| t.1.clone());
                    ui.label(&tname);
                    let mut full = e.full.clone();
                    if ui.add(egui::TextEdit::singleline(&mut full).hint_text(&tname).desired_width(110.0)).lost_focus() && full != e.full {
                        edit(app, "Full Name", &mut |v| v[k].entries[i].full = full.clone());
                    }
                    let mut short = e.short.clone();
                    if ui.add(egui::TextEdit::singleline(&mut short).hint_text(crate::scoreset::short_name(&tname)).desired_width(70.0)).lost_focus() && short != e.short {
                        edit(app, "Short Name", &mut |v| v[k].entries[i].short = short.clone());
                    }
                    let mut b = e.bracket;
                    if ui.checkbox(&mut b, "").changed() {
                        edit(app, "Bracket", &mut |v| v[k].entries[i].bracket = b);
                    }
                    let mut l = e.barline;
                    if ui.checkbox(&mut l, "").changed() {
                        edit(app, "Bar Line", &mut |v| v[k].entries[i].barline = l);
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(i > 0, egui::Button::new("▲").small()).clicked() {
                            edit(app, "Move Instrument", &mut |v| v[k].entries.swap(i, i - 1));
                        }
                        if ui.add_enabled(i + 1 < set.entries.len(), egui::Button::new("▼").small()).clicked() {
                            edit(app, "Move Instrument", &mut |v| v[k].entries.swap(i, i + 1));
                        }
                        if ui.small_button("✕").on_hover_text("Leave out of the set").clicked() {
                            edit(app, "Remove Instrument", &mut |v| {
                                v[k].entries.remove(i);
                            });
                        }
                    });
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                ui.menu_button("Add Instrument", |ui| {
                    for (id, n) in tracks.iter().filter(|t| set.entry(t.0).is_none()) {
                        if ui.button(n).clicked() {
                            let id = *id;
                            edit(app, "Add Instrument", &mut |v| v[k].entries.push(lg_core::score::SetEntry { track: id, ..Default::default() }));
                            ui.close();
                        }
                    }
                });
                ui.label("Scale:");
                let mut sc = set.scale as f32;
                if ui.add(egui::DragValue::new(&mut sc).range(25.0..=250.0).suffix(" %")).changed() {
                    edit(app, "Scale", &mut |v| v[k].scale = sc as u16);
                }
            });
            if ui.button("Show This Set").clicked() {
                score::with_state(&ui.ctx().clone(), |s| {
                    s.set = Some(k);
                    s.score_set = None;
                });
            }
        });
    });
}

/// The note heads by name (for the Functions ▸ Note Attributes menu).
pub fn head_names() -> &'static [&'static str] {
    &NOTE_HEADS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_toggle_and_move() {
        let mut g = vec![0];
        toggle_group(&mut g, 4);
        toggle_group(&mut g, 9);
        assert_eq!(g, vec![0, 4, 9]);
        toggle_group(&mut g, 4);
        assert_eq!(g, vec![0, 9]);
        move_group(&mut g, 9, 0);
        assert_eq!(g, vec![9, 0]);
    }

    #[test]
    fn sets_round_trip() {
        let sets = vec![("Lead sheet".to_string(), vec![0, 12, 9]), ("Drums".to_string(), vec![4])];
        assert_eq!(parse_sets(&sets_text(&sets)), sets);
    }

    #[test]
    fn pedal_alternates() {
        let ev = |pos, value| lg_core::events::Control { kind: 0xB0, channel: 0, pos, number: 64, value };
        let evs = vec![ev(0, 127), ev(960, 0), ev(1920, 127)];
        assert!(pedal_down_before(&evs, 500));
        assert!(!pedal_down_before(&evs, 1000));
        assert!(pedal_down_before(&evs, 3000));
        assert!(!pedal_down_before(&evs, 0));
    }

    #[test]
    fn bar_signs_snap() {
        assert_eq!(bar_snap(3800.0, 3840, 240, BarKind::Double), 3840);
        assert_eq!(bar_snap(2000.0, 3840, 240, BarKind::Double), 1920);
        assert_eq!(bar_snap(5000.0, 3840, 240, BarKind::Ending1), 3840);
        assert_eq!(bar_snap(1000.0, 3840, 240, BarKind::Segno), 960);
    }

    #[test]
    fn part_groups_cover_every_symbol() {
        for g in 0..Part::GROUPS.len() {
            for i in 0..Part::GROUPS[g] {
                assert_eq!(Part::from_index(g, i).index(), (g, i));
                assert!(!Part::from_index(g, i).name().is_empty());
            }
        }
    }
}
