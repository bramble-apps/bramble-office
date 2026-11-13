//! The Step Editor (Window ▸ Open Step Editor): the selected MIDI region's notes as lanes, one
//! per note (Logic's lane sets: GM Drums, the region's current notes, or lanes made by hand),
//! each step a beam whose height is the velocity. Click an empty step to add a note (the lane's
//! fixed value with Fix Value on, else the height clicked), drag a beam to change its velocity,
//! Option-click to remove it. Lanes are made, deleted, copied, pasted, converted and selected by
//! Logic's "Step Editor" key commands. Note edits are one undo step each; lane sets are the
//! view's (kept for the session).

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::events::Note;

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Create Lane Set", "Create Lane Set for GM Drums", "Create Lane Set for Current Events", "Clear Lane Set", "Create Lane", "Delete Lane", "Create Multiple Lanes…", "Convert Lane…", "Copy Lane", "Paste Lane", "Select All Lanes", "Toggle Fix Value", "Toggle Auto Define", "Select Next Lane", "Select Previous Lane", "Delete Lane Set",
    "Open Step Editor", "Open Step Editor…", "Show/Hide Step Editor",
];

/// A lane: its event type (Status: Note, Control, C-Press, P-Press, Program, PitchBd), its
/// first data byte (`pitch`: the note or controller number; off: every one), its name, the
/// value Fix Value enters, and the Lane inspector's Delay, Length and Channel.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub pitch: u8,
    pub name: String,
    pub fixed: u8,
    pub status: u8,
    pub use_number: bool,
    /// Ticks the lane's steps sit early (negative) or late.
    pub delay: i32,
    /// New steps' length in ticks (0: one grid step).
    pub len: u32,
    /// Only events on this MIDI channel (0-15); None: every channel.
    pub channel: Option<u8>,
}

/// The Lane inspector's Status values.
pub const STATUS: &[(&str, u8)] = &[("Note", 0x90), ("Control", 0xB0), ("C-Press", 0xD0), ("P-Press", 0xA0), ("Program", 0xC0), ("PitchBd", 0xE0)];

/// A step's event: a note or a region event (index).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ev {
    Note(usize),
    Ctl(usize),
}

/// The lane's event in the step at `pos` (song ticks), and its beam value.
fn hit(l: &Lane, rg: &lg_core::Region, pos: u32, grid: u32) -> Option<(Ev, u8)> {
    let from = (pos as i64 + l.delay as i64).max(0) as u32;
    let ch = |c: u8| l.channel.is_none_or(|x| x == c);
    if l.status == 0x90 {
        return rg.notes.iter().enumerate().find(|(_, n)| (!l.use_number || n.pitch == l.pitch) && ch(n.channel) && n.pos >= from && n.pos < from + grid).map(|(i, n)| (Ev::Note(i), n.velocity));
    }
    // (C-Press, Program and PitchBd have no first data byte to match)
    let by_number = l.use_number && matches!(l.status, 0xB0 | 0xA0);
    let rel = from.saturating_sub(rg.start);
    rg.ext.events.iter().enumerate().find(|(_, e)| e.kind == l.status && (!by_number || e.number == l.pitch) && ch(e.channel) && e.pos >= rel && e.pos < rel + grid).map(|(i, e)| (Ev::Ctl(i), e.value))
}

/// Renames a lane, changes its Status, Delay, Length, Channel or first data byte: the Lane
/// inspector's edits, on every selected lane.
pub fn edit_lanes(ctx: &egui::Context, f: impl Fn(&mut Lane)) {
    lanes_mut(ctx, |l, sel| {
        for &k in sel.iter() {
            if let Some(x) = l.get_mut(k) {
                f(x);
            }
        }
    });
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub open: bool,
    pub sets: Vec<(String, Vec<Lane>)>,
    pub set: usize,
    pub selected: Vec<usize>,
    pub fix_value: bool,
    pub auto_define: bool,
    /// Copy Lane's notes (positions from the region start, velocities, lengths).
    clip: Vec<(u32, u8, u32)>,
    /// Create Multiple Lanes… / Convert Lane… being typed.
    asking: Option<(bool, String)>,
    rect: Option<Rect>,
    /// Ticks per step.
    pub grid: u32,
    /// The Step Editor's tool (its own tool menu).
    pub tool: StepTool,
    /// Selected steps: (lane, step position in song ticks).
    pub sel_steps: Vec<(usize, u32)>,
    /// A drag of one of several selected steps: every selected step's value when it started,
    /// and the pressed step's.
    group: Option<(Vec<((usize, u32), u8)>, u8)>,
    /// The lane a Pencil or Eraser stroke (or a Shift-drag selection) runs on, and where it
    /// started.
    stroke: Option<(usize, u32)>,
    /// The Line tool's start (lane, position, value) while dragging; where the last line ended.
    line: Option<(usize, u32, u8)>,
    line_end: Option<(usize, u32, u8)>,
    /// Where the line being drawn ends now.
    line_cur: Option<(usize, u32, u8)>,
}

/// The Step Editor's tools (Logic: Pointer, Pencil, Eraser, Line).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum StepTool {
    #[default]
    Pointer,
    Pencil,
    Eraser,
    Line,
}

impl StepTool {
    pub const ALL: [StepTool; 4] = [StepTool::Pointer, StepTool::Pencil, StepTool::Eraser, StepTool::Line];
    pub fn name(self) -> &'static str {
        match self {
            StepTool::Pointer => "Pointer Tool",
            StepTool::Pencil => "Pencil Tool",
            StepTool::Eraser => "Eraser Tool",
            StepTool::Line => "Line Tool",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            StepTool::Pointer => crate::glyphs::ph::CURSOR,
            StepTool::Pencil => crate::glyphs::ph::PENCIL_SIMPLE,
            StepTool::Eraser => crate::glyphs::ph::ERASER,
            StepTool::Line => crate::glyphs::ph::LINE_SEGMENT,
        }
    }
}

/// A step edit: (lane, position, Some(value) / None removes, create the step if empty).
type Op = (usize, u32, Option<u8>, bool);

/// A group of step values moved by `d`: their differences kept, stopping when one reaches
/// `lo` or `hi`.
pub fn group_values(vals: &[u8], d: i32, lo: u8, hi: u8) -> Vec<u8> {
    let min = vals.iter().map(|&v| lo as i32 - v as i32).max().unwrap_or(0).min(0);
    let max = vals.iter().map(|&v| hi as i32 - v as i32).min().unwrap_or(0).max(0);
    let d = d.clamp(min, max);
    vals.iter().map(|&v| (v as i32 + d) as u8).collect()
}

/// The values along a line from (a, va) to (b, vb), one per step of `grid`.
pub fn line_values(a: u32, va: u8, b: u32, vb: u8, grid: u32) -> Vec<(u32, u8)> {
    let (a, va, b, vb) = if a <= b { (a, va, b, vb) } else { (b, vb, a, va) };
    let n = ((b - a) / grid.max(1)).max(1) as f32;
    (0..=((b - a) / grid.max(1))).map(|i| (a + i * grid, (va as f32 + (vb as f32 - va as f32) * i as f32 / n).round().clamp(0.0, 127.0) as u8)).collect()
}

fn id() -> egui::Id {
    egui::Id::unique("lg-step-editor")
}

pub fn state(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp::<State>(id())).unwrap_or_default()
}

fn with<R>(ctx: &egui::Context, f: impl FnOnce(&mut State) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<State>(id())))
}

fn lane(pitch: u8, drums: bool) -> Lane {
    let name = drums.then(|| crate::stepseq::drum_name(pitch)).flatten().map(str::to_string).unwrap_or_else(|| crate::app::note_name(pitch));
    Lane { pitch, name, fixed: 100, status: 0x90, use_number: true, delay: 0, len: 0, channel: None }
}

/// GM drum lanes (Logic's "Create Lane Set for GM Drums": the kit from the kick up).
pub fn gm_drums() -> Vec<Lane> {
    (35u8..=81).filter(|&p| crate::stepseq::drum_name(p).is_some()).map(|p| lane(p, true)).collect()
}

/// Lanes for the notes a region has, highest first.
pub fn current_lanes(notes: &[Note], drums: bool) -> Vec<Lane> {
    let mut p: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
    p.sort_unstable_by(|a, b| b.cmp(a));
    p.dedup();
    p.into_iter().map(|x| lane(x, drums)).collect()
}

fn region(app: &App) -> Option<(usize, usize, lg_core::Region)> {
    let (t, r) = app.selected_region?;
    let rg = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?.clone();
    rg.audio.is_none().then_some((t, r, rg))
}

fn lanes_mut<R>(ctx: &egui::Context, f: impl FnOnce(&mut Vec<Lane>, &mut Vec<usize>) -> R) -> R {
    with(ctx, |s| {
        if s.sets.is_empty() {
            s.sets.push(("Lane Set 1".into(), Vec::new()));
        }
        let k = s.set.min(s.sets.len() - 1);
        let (sets, sel) = (&mut s.sets, &mut s.selected);
        f(&mut sets[k].1, sel)
    })
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
    }
    match name {
        "Open Step Editor" | "Open Step Editor…" => {
            with(ctx, |s| s.open = true);
            return true;
        }
        "Show/Hide Step Editor" => {
            with(ctx, |s| s.open = !s.open);
            return true;
        }
        _ => {}
    }
    if !state(ctx).open {
        return false;
    }
    let drums = region(app).is_some_and(|(t, ..)| app.project.as_ref().is_some_and(|p| p.tracks[t].kind == lg_core::TrackKind::Drummer || p.tracks[t].name.to_lowercase().contains("drum")));
    match name {
        "Create Lane Set" | "Create Lane Set for GM Drums" | "Create Lane Set for Current Events" => {
            let lanes = match name {
                "Create Lane Set for GM Drums" => gm_drums(),
                "Create Lane Set for Current Events" => region(app).map(|(_, _, rg)| current_lanes(&rg.notes, drums)).unwrap_or_default(),
                _ => Vec::new(),
            };
            with(ctx, |s| {
                let n = s.sets.len() + 1;
                s.sets.push((format!("Lane Set {n}"), lanes));
                s.set = s.sets.len() - 1;
                s.selected.clear();
            });
        }
        "Clear Lane Set" => lanes_mut(ctx, |l, sel| {
            l.clear();
            sel.clear();
        }),
        "Delete Lane Set" => with(ctx, |s| {
            if s.set < s.sets.len() {
                s.sets.remove(s.set);
            }
            s.set = s.set.saturating_sub(1);
            s.selected.clear();
        }),
        "Create Lane" => {
            // the selected note's pitch with Auto Define, else the note below the selected lane
            let auto = state(ctx).auto_define;
            let from_note = region(app).and_then(|(_, _, rg)| app.selected_notes.first().and_then(|&i| rg.notes.get(i)).map(|n| n.pitch)).filter(|_| auto);
            lanes_mut(ctx, |l, sel| {
                let at = sel.first().copied().map_or(l.len(), |k| k + 1);
                let pitch = from_note.unwrap_or_else(|| sel.first().and_then(|&k| l.get(k)).map_or(60, |x| x.pitch.saturating_sub(1)));
                l.insert(at.min(l.len()), lane(pitch, drums));
                *sel = vec![at.min(l.len() - 1)];
            });
        }
        "Delete Lane" => lanes_mut(ctx, |l, sel| {
            let mut v = sel.clone();
            v.sort_unstable_by(|a, b| b.cmp(a));
            for k in v {
                if k < l.len() {
                    l.remove(k);
                }
            }
            sel.clear();
        }),
        "Create Multiple Lanes…" => with(ctx, |s| s.asking = Some((true, "4".into()))),
        "Convert Lane…" => {
            let cur = lanes_mut(ctx, |l, sel| sel.first().and_then(|&k| l.get(k)).map(|x| x.pitch));
            let Some(p) = cur else { return true };
            with(ctx, |s| s.asking = Some((false, p.to_string())));
        }
        "Copy Lane" => {
            let Some((_, _, rg)) = region(app) else { return true };
            let pitch = lanes_mut(ctx, |l, sel| sel.first().and_then(|&k| l.get(k)).map(|x| x.pitch));
            if let Some(p) = pitch {
                let clip: Vec<(u32, u8, u32)> = rg.notes.iter().filter(|n| n.pitch == p).map(|n| (n.pos - rg.start.min(n.pos), n.velocity, n.len)).collect();
                with(ctx, |s| s.clip = clip);
            }
        }
        "Paste Lane" => {
            let clip = state(ctx).clip;
            let pitch = lanes_mut(ctx, |l, sel| sel.first().and_then(|&k| l.get(k)).map(|x| x.pitch));
            let (Some(p), Some((t, r, rg))) = (pitch, region(app)) else { return true };
            app.checkpoint("Paste Lane");
            if let Some(region) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)?.regions.get_mut(r)) {
                region.notes.retain(|n| n.pitch != p);
                for &(pos, vel, len) in &clip {
                    region.notes.push(Note { channel: 0, pitch: p, velocity: vel, pos: rg.start + pos, len, muted: false });
                }
                region.notes.sort_by_key(|n| (n.pos, n.pitch));
            }
            app.selected_notes.clear();
            app.after_edit();
        }
        "Select All Lanes" => lanes_mut(ctx, |l, sel| *sel = (0..l.len()).collect()),
        "Select Next Lane" | "Select Previous Lane" => lanes_mut(ctx, |l, sel| {
            if l.is_empty() {
                return;
            }
            let cur = sel.first().copied().unwrap_or(0);
            let next = if name == "Select Next Lane" { (cur + 1).min(l.len() - 1) } else { cur.saturating_sub(1) };
            *sel = vec![next];
        }),
        "Toggle Fix Value" => with(ctx, |s| s.fix_value = !s.fix_value),
        "Toggle Auto Define" => with(ctx, |s| s.auto_define = !s.auto_define),
        _ => return false,
    }
    true
}

/// The Step Editor window's rect while it's open (key focus: focus.rs).
pub fn rect(ctx: &egui::Context) -> Option<Rect> {
    let st = state(ctx);
    st.rect.filter(|_| st.open)
}


pub fn window(app: &mut App, ctx: &egui::Context) {
    let st = state(ctx);
    if !st.open {
        return;
    }
    let mut open = true;
    let reg = region(app);
    // Delete: the selected steps (the window has key focus)
    if !st.sel_steps.is_empty() && crate::focus::clicked_in(app, st.rect) && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete) | i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)) {
        if let Some((t, r, _)) = reg.clone() {
            let lanes = st.sets.get(st.set).map(|s| s.1.clone()).unwrap_or_default();
            let grid = if st.grid == 0 { 240 } else { st.grid };
            // (latest first, so earlier notes keep their places)
            let mut ops: Vec<Op> = st.sel_steps.iter().map(|&(k, q)| (k, q, None, false)).collect();
            ops.sort_by(|a, b| b.1.cmp(&a.1));
            app.checkpoint("Delete events");
            if let Some(region) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                apply_ops(region, &lanes, &ops, grid);
            }
            app.selected_notes.clear();
            with(ctx, |s| s.sel_steps.clear());
            app.after_edit();
        }
    }
    let mut edit: Option<(usize, u32, Option<u8>)> = None; // (lane, pos, Some(value) set / None remove)
    let mut drag_vel: Option<(Ev, u8, bool)> = None;
    let mut lane_steps: Option<usize> = None;
    let mut convert = false;
    let mut reorder: Option<(usize, usize)> = None;
    let mut select_lane: Option<(usize, bool)> = None;
    let mut set_pick: Option<usize> = None;
    let mut ops: Vec<Op> = Vec::new();
    let mut begin: Option<&'static str> = None;
    let mut new_sel: Option<Vec<(usize, u32)>> = None;
    let mut set_fixed: Option<(usize, u8)> = None;
    let mut menu_cmd: Option<&'static str> = None;
    let mut stroke_start: Option<(usize, u32)> = None;
    let mut line_start: Option<(usize, u32, u8)> = None;
    let mut line_now: Option<(usize, u32, u8)> = None;
    let mut group_start: Option<(Vec<((usize, u32), u8)>, u8)> = None;
    let resp = egui::Window::new("Step Editor").id(egui::Id::unique("lg-step-editor-window")).open(&mut open).default_size(vec2(760.0, 360.0)).resizable(true).show(ctx, |ui| {
        let st = state(ui.ctx());
        ui.horizontal(|ui| {
            let names: Vec<String> = st.sets.iter().map(|s| s.0.clone()).collect();
            egui::ComboBox::from_id_salt("lg-se-set").selected_text(names.get(st.set).cloned().unwrap_or_else(|| "No Lane Set".into())).show_ui(ui, |ui| {
                for (k, n) in names.iter().enumerate() {
                    if ui.selectable_label(k == st.set, n).clicked() {
                        set_pick = Some(k);
                    }
                }
            });
            if ui.button("GM Drums").on_hover_text("Create Lane Set for GM Drums").clicked() {
                command(app, ui.ctx(), "Create Lane Set for GM Drums");
            }
            if ui.button("Current Events").on_hover_text("Create Lane Set for Current Events").clicked() {
                command(app, ui.ctx(), "Create Lane Set for Current Events");
            }
            ui.separator();
            let mut fix = st.fix_value;
            if ui.checkbox(&mut fix, "Fix Value").changed() {
                with(ui.ctx(), |s| s.fix_value = fix);
            }
            let mut auto = st.auto_define;
            if ui.checkbox(&mut auto, "Auto Define").changed() {
                with(ui.ctx(), |s| s.auto_define = auto);
            }
            ui.separator();
            for t in StepTool::ALL {
                let on = st.tool == t;
                let b = ui.add(egui::Button::selectable(on, egui::RichText::new(t.icon()).family(crate::glyphs::family()).size(14.0))).on_hover_text(t.name());
                if b.clicked() {
                    with(ui.ctx(), |s| s.tool = t);
                }
            }
            ui.menu_button("Delete MIDI Events", |ui| {
                for n in ["Duplicates", "Inside Locators", "Outside Locators", "Outside Region Boundaries", "Unselected Within Selection"] {
                    if ui.button(n).clicked() {
                        menu_cmd = Some(n);
                        ui.close();
                    }
                }
            });
            ui.separator();
            let grids = [("1/8", 480u32), ("1/16", 240), ("1/32", 120)];
            let g = if st.grid == 0 { 240 } else { st.grid };
            for (n, v) in grids {
                if ui.selectable_label(g == v, n).clicked() {
                    with(ui.ctx(), |s| s.grid = v);
                }
            }
        });
        let Some((_, _, rg)) = reg.clone() else {
            ui.label("Select a MIDI region");
            return;
        };
        let lanes = st.sets.get(st.set).map(|s| s.1.clone()).unwrap_or_default();
        if lanes.is_empty() {
            ui.label("Make a lane set: GM Drums, Current Events, or Create Lane (⌥⌘N)");
            return;
        }
        let grid = if st.grid == 0 { 240 } else { st.grid };
        let steps = rg.length.div_ceil(grid).max(1) as usize;
        let lane_h = 34.0;
        ui.horizontal_top(|ui| {
        lane_inspector(ui, &lanes, &st.selected);
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            let cell_w = 18.0;
            let (rect, _) = ui.allocate_exact_size(vec2(130.0 + steps as f32 * cell_w, lanes.len() as f32 * lane_h), Sense::hover());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 0.0, Color32::from_rgb(0x1b, 0x1b, 0x1e));
            let alt = ui.input(|i| i.modifiers.alt);
            for (k, l) in lanes.iter().enumerate() {
                let y = rect.min.y + k as f32 * lane_h;
                let head = Rect::from_min_size(pos2(rect.min.x, y), vec2(126.0, lane_h - 2.0));
                let sel = st.selected.contains(&k);
                p.rect_filled(head, 3.0, if sel { look::accent().gamma_multiply(0.6) } else { look::header_bg() });
                p.text(pos2(head.min.x + 8.0, head.center().y), Align2::LEFT_CENTER, &l.name, look::regular(12.0), look::text());
                let hr = ui.interact(head, ui.scope_id().with(("se-lane", k)), Sense::click_and_drag());
                if hr.clicked() {
                    select_lane = Some((k, ui.input(|i| i.modifiers.shift || i.modifiers.command)));
                    lane_steps = Some(k);
                }
                // double-click: Convert; drag up or down: the lane moves
                if hr.double_clicked() {
                    select_lane = Some((k, false));
                    convert = true;
                }
                if hr.dragged() {
                    if let Some(pp) = hr.interact_pointer_pos() {
                        let to = (((pp.y - rect.min.y) / lane_h).floor().max(0.0) as usize).min(lanes.len() - 1);
                        p.hline(head.x_range(), rect.min.y + to as f32 * lane_h, Stroke::new(2.0, look::accent()));
                    }
                }
                if hr.drag_stopped() {
                    if let Some(pp) = hr.interact_pointer_pos().or(ui.ctx().pointer_latest_pos()) {
                        let to = (((pp.y - rect.min.y) / lane_h).floor().max(0.0) as usize).min(lanes.len() - 1);
                        if to != k {
                            reorder = Some((k, to));
                        }
                    }
                }
                for s in 0..steps {
                    let x = rect.min.x + 130.0 + s as f32 * cell_w;
                    let cr = Rect::from_min_size(pos2(x, y + 2.0), vec2(cell_w - 2.0, lane_h - 6.0));
                    p.rect_filled(cr, 1.0, if (s * grid as usize / 960) % 2 == 0 { Color32::from_gray(0x2a) } else { Color32::from_gray(0x24) });
                    let pos = rg.start + s as u32 * grid;
                    let ev = hit(l, &rg, pos, grid);
                    if let Some((_, v)) = ev {
                        let f = v as f32 / 127.0;
                        p.rect_filled(Rect::from_min_max(pos2(cr.min.x + 2.0, cr.max.y - cr.height() * f), pos2(cr.max.x - 2.0, cr.max.y)), 1.0, look::mix(Color32::from_rgb(0x3a, 0x9a, 0xf0), Color32::from_rgb(0xf0, 0x50, 0x3a), f));
                    }
                    let r = ui.interact(cr, ui.scope_id().with(("se-cell", k, s)), Sense::click_and_drag());
                    let vel_at = |y: f32| (((cr.max.y - y) / cr.height()).clamp(0.0, 1.0) * 127.0).round() as u8;
                    let here = (k, pos);
                    if ev.is_some() && st.sel_steps.contains(&here) {
                        p.rect_stroke(cr, 1.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
                    }
                    let (shift, down) = ui.input(|i| (i.modifiers.shift, i.pointer.primary_down()));
                    let y_now = ui.input(|i| i.pointer.latest_pos()).map(|p| p.y);
                    match st.tool {
                        StepTool::Eraser => {
                            // a click or a drag over steps removes them
                            if r.drag_started() || r.clicked() {
                                begin = Some("Delete events");
                                stroke_start = Some((k, pos));
                            }
                            if ev.is_some() && (r.clicked() || r.drag_started() || (down && st.stroke.is_some() && r.contains_pointer())) {
                                ops.push((k, pos, None, false));
                            }
                        }
                        StepTool::Pencil => {
                            if r.clicked() {
                                match ev {
                                    // Fix Value: an existing step's value becomes the preset
                                    Some((_, v)) if st.fix_value => set_fixed = Some((k, v)),
                                    Some(_) => {
                                        begin = Some("Change Velocity");
                                        ops.push((k, pos, r.interact_pointer_pos().map(|pp| vel_at(pp.y)), false));
                                    }
                                    None => {
                                        begin = Some("Insert events");
                                        let v = if st.fix_value { l.fixed } else { r.interact_pointer_pos().map_or(100, |pp| vel_at(pp.y)) };
                                        ops.push((k, pos, Some(v), true));
                                    }
                                }
                            }
                            if r.drag_started() {
                                begin = Some("Insert events");
                                stroke_start = Some((k, pos));
                            }
                            // a horizontal drag draws steps following the pointer
                            if (r.drag_started() || (down && st.stroke.is_some_and(|s| s.0 == k) && r.contains_pointer())) && !(st.fix_value && ev.is_some()) {
                                let v = if st.fix_value { l.fixed } else { y_now.map_or(100, vel_at) };
                                ops.push((k, pos, Some(v), true));
                            }
                        }
                        StepTool::Line => {
                            if r.drag_started() {
                                let v = match st.line_end {
                                    // pressed where the last line ended: it goes on from there
                                    Some((lk, lp, lv)) if lk == k && lp == pos => lv,
                                    _ if st.fix_value => l.fixed,
                                    _ => r.interact_pointer_pos().map_or(100, |pp| vel_at(pp.y)),
                                };
                                line_start = Some((k, pos, v));
                            }
                        }
                        StepTool::Pointer => {
                            if r.clicked() {
                                match ev {
                                    Some(_) if alt => edit = Some((k, pos, None)),
                                    Some(_) if shift => {
                                        let mut v = st.sel_steps.clone();
                                        if let Some(i) = v.iter().position(|x| *x == here) {
                                            v.remove(i);
                                        } else {
                                            v.push(here);
                                        }
                                        new_sel = Some(v);
                                    }
                                    Some((i, _)) => {
                                        new_sel = Some(vec![here]);
                                        if let Some(pp) = r.interact_pointer_pos() {
                                            drag_vel = Some((i, vel_at(pp.y).max(1), true));
                                        }
                                    }
                                    None => {
                                        let v = if st.fix_value { l.fixed } else { r.interact_pointer_pos().map_or(100, |pp| vel_at(pp.y)) };
                                        edit = Some((k, pos, Some(v)));
                                    }
                                }
                            } else if r.drag_started() && shift {
                                // Shift-drag: the steps along the lane are selected
                                stroke_start = Some((k, pos));
                            } else if r.drag_started() && ev.is_some() && st.sel_steps.contains(&here) && st.sel_steps.len() > 1 && !st.fix_value {
                                app.checkpoint("Change Velocity");
                                let vals: Vec<((usize, u32), u8)> = st.sel_steps.iter().filter_map(|&(lk, lp)| Some(((lk, lp), hit(lanes.get(lk)?, &rg, lp, grid)?.1))).collect();
                                group_start = Some((vals, ev.map_or(0, |e| e.1)));
                            } else if r.dragged() && st.group.is_none() && st.stroke.is_none() && !st.fix_value {
                                if let (Some((i, _)), Some(pp)) = (ev, r.interact_pointer_pos()) {
                                    drag_vel = Some((i, vel_at(pp.y).max(1), false));
                                }
                            }
                            if r.drag_started() && ev.is_some() && !shift && !(st.sel_steps.contains(&here) && st.sel_steps.len() > 1) {
                                app.checkpoint("Change Velocity");
                            }
                            if down && shift && st.stroke.is_some_and(|s| s.0 == k) && r.contains_pointer() {
                                if let Some((_, from)) = st.stroke {
                                    let (a, b) = (from.min(pos), from.max(pos));
                                    new_sel = Some((0..steps).map(|s2| rg.start + s2 as u32 * grid).filter(|&q| q >= a && q <= b && hit(l, &rg, q, grid).is_some()).map(|q| (k, q)).collect());
                                }
                            }
                            if let (Some((vals, v0)), true) = (&st.group, r.dragged()) {
                                if let Some(pp) = r.interact_pointer_pos() {
                                    let d = vel_at(pp.y) as i32 - *v0 as i32;
                                    let olds: Vec<u8> = vals.iter().map(|x| x.1).collect();
                                    let lo = if vals.iter().any(|x| lanes.get(x.0 .0).is_some_and(|l| l.status == 0x90)) { 1 } else { 0 };
                                    for (x, nv) in vals.iter().zip(group_values(&olds, d, lo, 127)) {
                                        ops.push((x.0 .0, x.0 .1, Some(nv), false));
                                    }
                                    p.text(pp + vec2(12.0, -12.0), Align2::LEFT_BOTTOM, format!("{}", (*v0 as i32 + d).clamp(0, 127)), look::semibold(11.0), Color32::WHITE);
                                }
                            }
                        }
                    }
                    if r.dragged() && matches!(st.tool, StepTool::Pointer | StepTool::Pencil) && st.group.is_none() {
                        if let (Some(pp), Some(_)) = (r.interact_pointer_pos(), ev) {
                            p.text(pp + vec2(12.0, -12.0), Align2::LEFT_BOTTOM, format!("{}", vel_at(pp.y)), look::semibold(11.0), Color32::WHITE);
                        }
                    }
                }
                // the Line tool's line, with the position and value under the pointer
                if let (Some((lk, lpos, lv)), Some(pp)) = (st.line.or(line_start), ui.input(|i| i.pointer.latest_pos())) {
                    if lk == k && ui.input(|i| i.pointer.primary_down()) {
                        let x0 = rect.min.x + 130.0 + ((lpos - rg.start) / grid) as f32 * cell_w + cell_w / 2.0;
                        let y0 = y + 2.0 + (lane_h - 6.0) * (1.0 - lv as f32 / 127.0);
                        p.line_segment([pos2(x0, y0), pp], Stroke::new(1.5, Color32::WHITE));
                        let s2 = (((pp.x - rect.min.x - 130.0) / cell_w).floor().max(0.0) as u32).min(steps as u32 - 1);
                        let v = (((y + lane_h - 4.0 - pp.y) / (lane_h - 6.0)).clamp(0.0, 1.0) * 127.0).round() as u8;
                        let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
                        p.text(pp + vec2(12.0, -12.0), Align2::LEFT_BOTTOM, format!("Position: {}   Value: {v}", crate::app::bbdt(rg.start + s2 * grid, sig)), look::semibold(11.0), Color32::WHITE);
                        line_now = Some((k, rg.start + s2 * grid, v));
                    }
                }
                p.hline(rect.x_range(), y + lane_h - 1.0, Stroke::new(1.0, Color32::from_gray(0x10)));
            }
        });
        });
    });
    if let Some(r) = resp.as_ref().map(|r| r.response.rect) {
        with(ctx, |s| s.rect = Some(r));
    }
    step_gestures(app, ctx, reg.clone(), StepInput { ops, begin, new_sel, set_fixed, menu_cmd, stroke_start, line_start, line_now, group_start });
    if let Some(k) = set_pick {
        with(ctx, |s| {
            s.set = k;
            s.selected.clear();
        });
    }
    if let Some((k, add)) = select_lane {
        with(ctx, |s| {
            if add {
                if let Some(i) = s.selected.iter().position(|x| *x == k) {
                    s.selected.remove(i);
                } else {
                    s.selected.push(k);
                }
            } else {
                s.selected = vec![k];
            }
        });
    }
    let lanes_now = state(ctx).sets.get(state(ctx).set).map(|s| s.1.clone()).unwrap_or_default();
    if let (Some((t, r, rg)), Some((k, pos, vel)), Some(l)) = (reg.clone(), edit, edit.and_then(|e| lanes_now.get(e.0).cloned())) {
        app.checkpoint(if vel.is_some() { "Insert events" } else { "Delete events" });
        let grid = state(ctx).grid.max(1);
        let grid = if grid == 1 { 240 } else { grid };
        let found = hit(&l, &rg, pos, grid);
        let _ = k;
        if let Some(region) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            set_step(region, &l, pos, grid, vel, found.map(|x| x.0));
        }
        app.selected_notes.clear();
        app.after_edit();
    }
    if let (Some(k), Some((_, _, rg))) = (lane_steps, reg.clone()) {
        // a click on a lane name selects all its steps (notes)
        if let Some(l) = lanes_now.get(k) {
            if l.status == 0x90 {
                app.selected_notes = rg.notes.iter().enumerate().filter(|(_, n)| (!l.use_number || n.pitch == l.pitch) && l.channel.is_none_or(|c| c == n.channel)).map(|(i, _)| i).collect();
            }
        }
    }
    if let Some((a, b)) = reorder {
        lanes_mut(ctx, |l, sel| {
            let x = l.remove(a);
            l.insert(b.min(l.len()), x);
            *sel = vec![b.min(l.len() - 1)];
        });
    }
    if convert {
        command(app, ctx, "Convert Lane…");
    }
    if let (Some((t, r, _)), Some((ev, v, click))) = (reg, drag_vel) {
        // a click is its own undo step (a drag took one when it started)
        if click {
            app.checkpoint("Change Velocity");
        }
        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            match ev {
                Ev::Note(i) => {
                    if let Some(n) = rg.notes.get_mut(i) {
                        n.velocity = v.max(1);
                    }
                }
                Ev::Ctl(i) => {
                    if let Some(e) = rg.ext.events.get_mut(i) {
                        e.value = v.min(127);
                    }
                }
            }
        }
        if click || !ctx.input(|x| x.pointer.primary_down()) {
            app.after_edit();
        }
    }
    // Create Multiple Lanes… / Convert Lane…
    if let Some((multi, mut text)) = state(ctx).asking {
        let mut done = None;
        egui::Window::new(if multi { "Create Multiple Lanes" } else { "Convert Lane" }).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(if multi { "Lanes (one per semitone down from the selected lane):" } else { "Convert the selected lane (and its notes) to note number:" });
            ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0)).request_focus();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        with(ctx, |s| s.asking = if done.is_some() { None } else { Some((multi, text.clone())) });
        if done == Some(true) {
            if let Ok(n) = text.trim().parse::<u32>() {
                if multi {
                    lanes_mut(ctx, |l, sel| {
                        let at = sel.first().copied().map_or(l.len(), |k| k + 1).min(l.len());
                        let from = sel.first().and_then(|&k| l.get(k)).map_or(61, |x| x.pitch);
                        for j in 0..n.min(64) {
                            l.insert(at + j as usize, lane(from.saturating_sub(1 + j as u8), false));
                        }
                    });
                } else if n <= 127 {
                    let old = lanes_mut(ctx, |l, sel| sel.first().and_then(|&k| l.get_mut(k)).map(|x| {
                        let o = x.pitch;
                        *x = lane(n as u8, crate::stepseq::drum_name(o).is_some_and(|d| d == x.name));
                        o
                    }));
                    if let (Some(o), Some((t, r, _))) = (old, region(app)) {
                        app.checkpoint("Convert Lane");
                        if let Some(region) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                            region.notes.iter_mut().filter(|x| x.pitch == o).for_each(|x| x.pitch = n as u8);
                        }
                        app.after_edit();
                    }
                }
            }
        }
    }
    if !open {
        with(ctx, |s| s.open = false);
    }
}

/// What the steps' gestures did this frame.
struct StepInput {
    ops: Vec<Op>,
    begin: Option<&'static str>,
    new_sel: Option<Vec<(usize, u32)>>,
    set_fixed: Option<(usize, u8)>,
    menu_cmd: Option<&'static str>,
    stroke_start: Option<(usize, u32)>,
    line_start: Option<(usize, u32, u8)>,
    line_now: Option<(usize, u32, u8)>,
    group_start: Option<(Vec<((usize, u32), u8)>, u8)>,
}

/// Applies the Step Editor's tool gestures: Eraser and Pencil strokes, the Line tool (Option
/// on release: every empty grid position gets a step too, but no notes), group value drags,
/// step selection (the note lanes' steps are the selected notes, so Delete removes them).
fn step_gestures(app: &mut App, ctx: &egui::Context, reg: Option<(usize, usize, lg_core::Region)>, inp: StepInput) {
    let st = state(ctx);
    let down = ctx.input(|i| i.pointer.primary_down());
    let alt = ctx.input(|i| i.modifiers.alt);
    let lanes = st.sets.get(st.set).map(|s| s.1.clone()).unwrap_or_default();
    let grid = if st.grid == 0 { 240 } else { st.grid };
    if let Some(n) = inp.menu_cmd {
        app.key_command(ctx, n);
    }
    if let Some((k, v)) = inp.set_fixed {
        lanes_mut(ctx, |l, _| {
            if let Some(x) = l.get_mut(k) {
                x.fixed = v;
            }
        });
    }
    with(ctx, |s| {
        if inp.stroke_start.is_some() {
            s.stroke = inp.stroke_start;
        }
        if inp.line_start.is_some() {
            s.line = inp.line_start;
        }
        if inp.group_start.is_some() {
            s.group = inp.group_start.clone();
        }
        if inp.line_now.is_some() {
            s.line_cur = inp.line_now;
        }
    });
    let mut ops = inp.ops;
    // the line is drawn on release
    if let (Some((k, a, va)), false) = (st.line.or(inp.line_start), down) {
        if let (Some((lk, b, vb)), Some(l)) = (inp.line_now.or(st.line_cur), lanes.get(k)) {
            if lk == k {
                let create = alt && l.status != 0x90;
                for (q, v) in line_values(a, va, b, vb, grid) {
                    ops.push((k, q, Some(v), create));
                }
                with(ctx, |s| s.line_end = Some((k, b, vb)));
                if !ops.is_empty() {
                    app.checkpoint("Line Tool");
                }
            }
        }
        with(ctx, |s| {
            s.line = None;
            s.line_cur = None;
        });
    }
    if !down {
        with(ctx, |s| {
            s.stroke = None;
            s.group = None;
        });
    }
    if let Some(v) = inp.new_sel {
        // the selected steps; their notes are the selected notes
        if let Some((_, _, rg)) = &reg {
            app.selected_notes = v.iter().filter_map(|&(k, q)| match hit(lanes.get(k)?, rg, q, grid)? {
                (Ev::Note(i), _) => Some(i),
                _ => None,
            }).collect();
        }
        with(ctx, |s| s.sel_steps = v);
    }
    let Some((t, r, _)) = reg else { return };
    if ops.is_empty() {
        return;
    }
    if let Some(what) = inp.begin {
        app.checkpoint(what);
    }
    let removed = ops.iter().any(|o| o.2.is_none());
    if let Some(region) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        apply_ops(region, &lanes, &ops, grid);
    }
    if removed {
        app.selected_notes.clear();
        with(ctx, |s| s.sel_steps.clear());
    }
    if !down || removed {
        app.after_edit();
    }
}

/// Step edits on a region: a value set (or a step made when `create`, and allowed: the Line
/// tool makes no notes), or a step removed.
pub fn apply_ops(region: &mut lg_core::Region, lanes: &[Lane], ops: &[Op], grid: u32) {
    for &(k, q, val, create) in ops {
        let Some(l) = lanes.get(k) else { continue };
        let found = hit(l, region, q, grid);
        match (val, found) {
            (Some(v), Some((Ev::Note(i), _))) => {
                if let Some(n) = region.notes.get_mut(i) {
                    n.velocity = v.max(1);
                }
            }
            (Some(v), Some((Ev::Ctl(i), _))) => {
                if let Some(e) = region.ext.events.get_mut(i) {
                    e.value = v.min(127);
                }
            }
            (Some(v), None) if create => set_step(region, l, q, grid, Some(v), None),
            (None, Some((ev, _))) => set_step(region, l, q, grid, None, Some(ev)),
            _ => {}
        }
    }
}

/// A step set (Some(value)) or removed (None) on lane `l` at `pos` (song ticks; the lane's
/// Delay added; a new note `l.len` long, or one step).
fn set_step(region: &mut lg_core::Region, l: &Lane, pos: u32, grid: u32, value: Option<u8>, found: Option<Ev>) {
    let at = (pos as i64 + l.delay as i64).max(0) as u32;
    let ch = l.channel.unwrap_or(0);
    match (value, found) {
        (Some(v), _) if l.status == 0x90 => {
            region.notes.push(Note { channel: ch, pitch: l.pitch, velocity: v.max(1), pos: at, len: if l.len > 0 { l.len } else { grid }, muted: false });
            region.notes.sort_by_key(|n| (n.pos, n.pitch));
        }
        (Some(v), _) => {
            region.ext.events.push(lg_core::events::Control { kind: l.status, channel: ch, pos: at.saturating_sub(region.start), number: if matches!(l.status, 0xB0 | 0xA0) { l.pitch } else { 0 }, value: v.min(127) });
            region.ext.events.sort_by_key(|e| e.pos);
        }
        (None, Some(Ev::Note(i))) => {
            region.notes.remove(i);
        }
        (None, Some(Ev::Ctl(i))) => {
            region.ext.events.remove(i);
        }
        (None, None) => {}
    }
}

/// The Lane inspector (left of the steps): the selected lane's name (click to rename), Status,
/// Delay, Length, Channel and First Data Byte; edits go to every selected lane.
fn lane_inspector(ui: &mut egui::Ui, lanes: &[Lane], sel: &[usize]) {
    let w = 190.0;
    ui.allocate_ui_with_layout(vec2(w, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(w);
        let Some(l) = sel.first().and_then(|&k| lanes.get(k)).cloned() else {
            ui.label(egui::RichText::new("Lane:").strong());
            return;
        };
        let ctx = ui.ctx().clone();
        let nid = ui.scope_id().with("se-rename");
        let mut naming: Option<String> = ui.data(|d| d.get_temp(nid));
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Lane:").strong());
            match naming.as_mut() {
                Some(t) => {
                    let te = ui.add(egui::TextEdit::singleline(t).desired_width(120.0));
                    crate::widgets::focus_once(ui, &te);
                    if te.lost_focus() {
                        let n = t.clone();
                        edit_lanes(&ctx, |x| x.name = n.clone());
                        ui.data_mut(|d| d.remove::<String>(nid));
                    } else {
                        ui.data_mut(|d| d.insert_temp(nid, t.clone()));
                    }
                }
                None => {
                    if ui.add(egui::Label::new(&l.name).sense(Sense::click())).on_hover_text("Click to rename").clicked() {
                        ui.data_mut(|d| d.insert_temp(nid, l.name.clone()));
                    }
                }
            }
        });
        egui::Grid::new("se-lane-insp").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
            ui.label("Status");
            let cur = STATUS.iter().position(|s| s.1 == l.status).unwrap_or(0);
            egui::ComboBox::from_id_salt("se-status").selected_text(STATUS[cur].0).width(90.0).show_ui(ui, |ui| {
                for (n, v) in STATUS {
                    if ui.selectable_label(*v == l.status, *n).clicked() {
                        edit_lanes(&ctx, |x| x.status = *v);
                    }
                }
            });
            ui.end_row();
            ui.label("Delay");
            let mut d = l.delay;
            if ui.add(egui::DragValue::new(&mut d).range(-960..=960).suffix(" tk")).changed() {
                edit_lanes(&ctx, |x| x.delay = d);
            }
            ui.end_row();
            ui.label("Length");
            ui.horizontal(|ui| {
                let (mut dv, mut tk) = (l.len / 240, l.len % 240);
                let a = ui.add(egui::DragValue::new(&mut dv).range(0..=64)).changed();
                let b = ui.add(egui::DragValue::new(&mut tk).range(0..=239)).changed();
                if a || b {
                    edit_lanes(&ctx, |x| x.len = dv * 240 + tk);
                }
            });
            ui.end_row();
            let mut on = l.channel.is_some();
            if ui.checkbox(&mut on, "Channel").changed() {
                edit_lanes(&ctx, |x| x.channel = if on { Some(x.channel.unwrap_or(0)) } else { None });
            }
            ui.add_enabled_ui(on, |ui| {
                let c = l.channel.unwrap_or(0);
                egui::ComboBox::from_id_salt("se-ch").selected_text(format!("{}", c + 1)).width(50.0).show_ui(ui, |ui| {
                    for k in 0..16u8 {
                        if ui.selectable_label(k == c, format!("{}", k + 1)).clicked() {
                            edit_lanes(&ctx, |x| x.channel = Some(k));
                        }
                    }
                });
            });
            ui.end_row();
            // First Data Byte: a pitch for Note and P-Press, a number for Control and Program;
            // dimmed for C-Press and PitchBd (no first byte)
            let used = matches!(l.status, 0x90 | 0xB0 | 0xA0 | 0xC0);
            let mut on = l.use_number;
            ui.add_enabled_ui(used, |ui| {
                if ui.checkbox(&mut on, if l.status == 0xB0 { "Number" } else { "Data 1" }).changed() {
                    edit_lanes(&ctx, |x| x.use_number = on);
                }
            });
            ui.add_enabled_ui(used && on, |ui| {
                let mut v = l.pitch as i32;
                let pitch = matches!(l.status, 0x90 | 0xA0);
                if ui.add(egui::DragValue::new(&mut v).range(0..=127).custom_formatter(move |x, _| if pitch { crate::app::note_name(x as u8) } else { format!("{x:.0}") })).changed() {
                    edit_lanes(&ctx, |x| {
                        x.pitch = v as u8;
                        if x.status == 0x90 && crate::stepseq::drum_name(v as u8).is_some() {
                            x.name = crate::stepseq::drum_name(v as u8).unwrap_or_default().to_string();
                        }
                    });
                }
            });
            ui.end_row();
        });
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn step_tools() {
        assert_eq!(super::group_values(&[100, 120], 20, 1, 127), [107, 127]);
        assert_eq!(super::group_values(&[10, 40], -30, 1, 127), [1, 31]);
        assert_eq!(super::line_values(0, 0, 960, 120, 240), [(0, 0), (240, 30), (480, 60), (720, 90), (960, 120)]);
        assert_eq!(super::line_values(480, 100, 0, 0, 240), [(0, 0), (240, 50), (480, 100)]);
        let l = super::Lane { pitch: 36, name: "Kick".into(), fixed: 100, status: 0x90, use_number: true, delay: 0, len: 0, channel: None };
        let c = super::Lane { status: 0xB0, pitch: 7, ..l.clone() };
        let mut rg = lg_core::Region { name: "R".into(), start: 0, length: 3840, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
        // the Line tool (create) makes controller steps, never notes
        super::apply_ops(&mut rg, &[l.clone(), c], &[(0, 0, Some(90), false), (1, 0, Some(50), true), (1, 240, Some(60), true)], 240);
        assert!(rg.notes.is_empty());
        assert_eq!(rg.ext.events.len(), 2);
        super::apply_ops(&mut rg, &[l.clone()], &[(0, 0, Some(90), true), (0, 240, Some(80), true)], 240);
        assert_eq!(rg.notes.len(), 2);
        super::apply_ops(&mut rg, &[l], &[(0, 240, None, false)], 240);
        assert_eq!(rg.notes.len(), 1);
    }

    use super::*;

    #[test]
    fn lane_sets() {
        let d = gm_drums();
        assert_eq!(d.first().map(|l| l.pitch), Some(35));
        assert!(d.iter().any(|l| l.name == "Snare 1"));
        let notes = [Note { channel: 0, pitch: 60, velocity: 90, pos: 0, len: 10, muted: false }, Note { channel: 0, pitch: 64, velocity: 90, pos: 0, len: 10, muted: false }, Note { channel: 0, pitch: 60, velocity: 90, pos: 240, len: 10, muted: false }];
        assert_eq!(current_lanes(&notes, false).iter().map(|l| l.pitch).collect::<Vec<_>>(), [64, 60]);
    }

    #[test]
    fn lane_types_delay_length_channel() {
        let mut rg = lg_core::Region { name: "R".into(), start: 3840, length: 3840, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
        let mut l = lane(60, false);
        l.delay = 10;
        l.len = 480;
        l.channel = Some(2);
        set_step(&mut rg, &l, 3840 + 240, 240, Some(90), None);
        assert_eq!((rg.notes[0].pos, rg.notes[0].len, rg.notes[0].channel), (3840 + 250, 480, 2));
        assert_eq!(hit(&l, &rg, 3840 + 240, 240).map(|x| x.1), Some(90));
        let mut other = l.clone();
        other.channel = Some(3);
        assert!(hit(&other, &rg, 3840 + 240, 240).is_none(), "another channel's lane doesn't show it");
        other.channel = None;
        assert!(hit(&other, &rg, 3840 + 240, 240).is_some());
        // a Control lane: CC 1 events in the region
        let mut cc = lane(1, false);
        cc.status = 0xB0;
        set_step(&mut rg, &cc, 3840, 240, Some(64), None);
        assert_eq!(rg.ext.events[0], lg_core::events::Control { kind: 0xB0, channel: 0, pos: 0, number: 1, value: 64 });
        let found = hit(&cc, &rg, 3840, 240);
        assert_eq!(found.map(|x| x.1), Some(64));
        set_step(&mut rg, &cc, 3840, 240, None, found.map(|x| x.0));
        assert!(rg.ext.events.is_empty());
        // Data 1 off: a note lane shows every pitch
        let mut any = lane(10, false);
        any.use_number = false;
        any.delay = 10;
        assert!(hit(&any, &rg, 3840 + 240, 240).is_some());
    }
}
