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

/// A lane: a note (pitch), its name, and the velocity Fix Value enters.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub pitch: u8,
    pub name: String,
    pub fixed: u8,
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
    Lane { pitch, name, fixed: 100 }
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
                    region.notes.push(Note { channel: 0, pitch: p, velocity: vel, pos: rg.start + pos, len });
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

pub fn hovered(ctx: &egui::Context, p: Option<egui::Pos2>) -> bool {
    let st = state(ctx);
    st.open && p.is_some_and(|p| st.rect.is_some_and(|r| r.contains(p)))
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    let st = state(ctx);
    if !st.open {
        return;
    }
    let mut open = true;
    let reg = region(app);
    let mut edit: Option<(u8, u32, Option<u8>)> = None; // (pitch, pos, Some(vel) set / None remove)
    let mut drag_vel: Option<(usize, u8, bool)> = None;
    let mut select_lane: Option<(usize, bool)> = None;
    let mut set_pick: Option<usize> = None;
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
                let hr = ui.interact(head, ui.scope_id().with(("se-lane", k)), Sense::click());
                if hr.clicked() {
                    select_lane = Some((k, ui.input(|i| i.modifiers.shift || i.modifiers.command)));
                }
                for s in 0..steps {
                    let x = rect.min.x + 130.0 + s as f32 * cell_w;
                    let cr = Rect::from_min_size(pos2(x, y + 2.0), vec2(cell_w - 2.0, lane_h - 6.0));
                    p.rect_filled(cr, 1.0, if (s * grid as usize / 960) % 2 == 0 { Color32::from_gray(0x2a) } else { Color32::from_gray(0x24) });
                    let pos = rg.start + s as u32 * grid;
                    let ev = rg.notes.iter().enumerate().find(|(_, n)| n.pitch == l.pitch && n.pos >= pos && n.pos < pos + grid);
                    if let Some((_, n)) = ev {
                        let f = n.velocity as f32 / 127.0;
                        p.rect_filled(Rect::from_min_max(pos2(cr.min.x + 2.0, cr.max.y - cr.height() * f), pos2(cr.max.x - 2.0, cr.max.y)), 1.0, look::mix(Color32::from_rgb(0x3a, 0x9a, 0xf0), Color32::from_rgb(0xf0, 0x50, 0x3a), f));
                    }
                    let r = ui.interact(cr, ui.scope_id().with(("se-cell", k, s)), Sense::click_and_drag());
                    let vel_at = |y: f32| (((cr.max.y - y) / cr.height()).clamp(0.05, 1.0) * 127.0).round() as u8;
                    if r.clicked() {
                        match ev {
                            Some(_) if alt => edit = Some((l.pitch, pos, None)),
                            Some((i, _)) => {
                                if let Some(pp) = r.interact_pointer_pos() {
                                    drag_vel = Some((i, vel_at(pp.y), true));
                                }
                            }
                            None => {
                                let v = if st.fix_value { l.fixed } else { r.interact_pointer_pos().map_or(100, |pp| vel_at(pp.y)) };
                                edit = Some((l.pitch, pos, Some(v)));
                            }
                        }
                    } else if r.dragged() {
                        if let (Some((i, _)), Some(pp)) = (ev, r.interact_pointer_pos()) {
                            drag_vel = Some((i, vel_at(pp.y), false));
                        }
                    }
                    if r.drag_started() && ev.is_some() {
                        app.checkpoint("Change Velocity");
                    }
                }
                p.hline(rect.x_range(), y + lane_h - 1.0, Stroke::new(1.0, Color32::from_gray(0x10)));
            }
        });
    });
    if let Some(r) = resp.as_ref().map(|r| r.response.rect) {
        with(ctx, |s| s.rect = Some(r));
    }
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
    if let (Some((t, r, rg)), Some((pitch, pos, vel))) = (reg.clone(), edit) {
        app.checkpoint(if vel.is_some() { "Insert events" } else { "Delete events" });
        let grid = state(ctx).grid.max(1);
        let grid = if grid == 1 { 240 } else { grid };
        if let Some(region) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            match vel {
                Some(v) => {
                    region.notes.push(Note { channel: 0, pitch, velocity: v, pos, len: grid });
                    region.notes.sort_by_key(|n| (n.pos, n.pitch));
                }
                None => region.notes.retain(|n| !(n.pitch == pitch && n.pos >= pos && n.pos < pos + grid)),
            }
        }
        let _ = rg;
        app.selected_notes.clear();
        app.after_edit();
    }
    if let (Some((t, r, _)), Some((i, v, click))) = (reg, drag_vel) {
        // a click is its own undo step (a drag took one when it started)
        if click {
            app.checkpoint("Change Velocity");
        }
        if let Some(n) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)?.notes.get_mut(i)) {
            n.velocity = v;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_sets() {
        let d = gm_drums();
        assert_eq!(d.first().map(|l| l.pitch), Some(35));
        assert!(d.iter().any(|l| l.name == "Snare 1"));
        let notes = [Note { channel: 0, pitch: 60, velocity: 90, pos: 0, len: 10 }, Note { channel: 0, pitch: 64, velocity: 90, pos: 0, len: 10 }, Note { channel: 0, pitch: 60, velocity: 90, pos: 240, len: 10 }];
        assert_eq!(current_lanes(&notes, false).iter().map(|l| l.pitch).collect::<Vec<_>>(), [64, 60]);
    }
}
