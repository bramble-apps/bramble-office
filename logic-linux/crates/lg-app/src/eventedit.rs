//! The Event Editor's key commands (Logic's "Event Editor" group) for the List Editors' Events
//! list in the right pane: ↑ ↓ select the previous / next event, Relative Position (positions
//! from the region's start) and Length as Absolute Position (the end instead of the length),
//! Numerical Edit of Event Position (type the position), Duplicate Event and Numerical Edit,
//! Copy Value to All Following Events (the selected event's velocity), Create Event (a note at
//! the playhead). Edits are one undo step each.

use lg_core::events::Note;

use crate::app::App;

pub const COMMANDS: &[&str] = &[
    "Select Previous Event", "Select Next Event", "Length as Absolute Position", "Relative Position", "Numerical Edit of Event Position", "Duplicate Event and Numerical Edit", "Copy Value to All Following Events", "Create Event",
    "Open Event List", "Open Event List…", "Show/Hide Event List",
];

#[derive(Clone, Debug, Default)]
pub struct State {
    pub relative: bool,
    pub abs_length: bool,
    /// The position being typed for an event (index, text).
    pub typing: Option<(usize, String)>,
    pub rect: Option<egui::Rect>,
}

fn id() -> egui::Id {
    egui::Id::unique("lg-event-editor")
}

pub fn state(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp::<State>(id())).unwrap_or_default()
}

pub fn with<R>(ctx: &egui::Context, f: impl FnOnce(&mut State) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<State>(id())))
}

fn open(app: &App) -> bool {
    app.right_pane == Some(crate::app::RightPane::Lists) && app.list_tab == 0
}

pub fn hovered(app: &App, ctx: &egui::Context, p: Option<egui::Pos2>) -> bool {
    open(app) && p.is_some_and(|p| state(ctx).rect.is_some_and(|r| r.contains(p)))
}

/// "1 2 3 4" (bar beat division tick, from 1) into ticks.
pub fn parse_position(text: &str, sig: (u8, u8)) -> Option<u32> {
    let bar = lg_core::project::bar_ticks(sig);
    let beat = bar / sig.0.max(1) as u32;
    let div = (beat / 4).max(1);
    let n: Vec<u32> = text.split_whitespace().map(|x| x.parse::<u32>()).collect::<Result<_, _>>().ok()?;
    let at = |k: usize, d: u32| n.get(k).map_or(d, |&v| v);
    let (b, bt, dv, tk) = (at(0, 1), at(1, 1), at(2, 1), at(3, 1));
    if b == 0 || bt == 0 || dv == 0 || tk == 0 {
        return None;
    }
    Some((b - 1) * bar + (bt - 1) * beat + (dv - 1) * div + (tk - 1))
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
    }
    if matches!(name, "Open Event List" | "Open Event List…" | "Show/Hide Event List") {
        let show = name != "Show/Hide Event List" || !open(app);
        app.right_pane = if show { Some(crate::app::RightPane::Lists) } else { None };
        app.list_tab = 0;
        return true;
    }
    if !open(app) {
        return false;
    }
    let Some((t, r)) = app.selected_region else { return false };
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return false };
    let cur = app.selected_notes.first().copied();
    match name {
        "Select Previous Event" | "Select Next Event" => {
            if rg.notes.is_empty() {
                return true;
            }
            let next = match cur {
                None => 0,
                Some(i) if name == "Select Next Event" => (i + 1).min(rg.notes.len() - 1),
                Some(i) => i.saturating_sub(1),
            };
            app.selected_notes = vec![next];
            if let Some(n) = rg.notes.get(next) {
                app.preview_note(t, n.pitch, n.velocity);
            }
        }
        "Length as Absolute Position" => with(ctx, |s| s.abs_length = !s.abs_length),
        "Relative Position" => with(ctx, |s| s.relative = !s.relative),
        "Numerical Edit of Event Position" => {
            let Some(i) = cur else { return true };
            let text = crate::rightpane::bbdt(app, rg.notes[i].pos);
            with(ctx, |s| s.typing = Some((i, text)));
        }
        "Duplicate Event and Numerical Edit" => {
            let Some(i) = cur else { return true };
            let n = rg.notes[i];
            app.checkpoint("Duplicate Event");
            let mut idx = i;
            if let Some(region) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                region.notes.insert(i + 1, n);
                idx = i + 1;
            }
            app.selected_notes = vec![idx];
            app.after_edit();
            let text = crate::rightpane::bbdt(app, n.pos);
            with(ctx, |s| s.typing = Some((idx, text)));
        }
        "Copy Value to All Following Events" => {
            let Some(i) = cur else { return true };
            let v = rg.notes[i].velocity;
            app.checkpoint("Copy Value to All Following Events");
            if let Some(region) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                region.notes.iter_mut().skip(i + 1).for_each(|n| n.velocity = v);
            }
            app.after_edit();
        }
        "Create Event" => {
            let at = (app.position().max(0.0) as u32).clamp(rg.start, rg.start + rg.length.saturating_sub(1));
            let like = cur.and_then(|i| rg.notes.get(i)).copied().unwrap_or(Note { channel: 0, pitch: 60, velocity: 80, pos: 0, len: 240 });
            app.checkpoint("Create Event");
            let mut idx = 0;
            if let Some(region) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                region.notes.push(Note { pos: at, ..like });
                region.notes.sort_by_key(|n| (n.pos, n.pitch));
                idx = region.notes.iter().position(|n| n.pos == at && n.pitch == like.pitch).unwrap_or(0);
            }
            app.selected_notes = vec![idx];
            app.after_edit();
        }
        _ => return false,
    }
    true
}

/// Applies a typed position (Numerical Edit) to an event.
pub fn set_position(app: &mut App, i: usize, text: &str) {
    let Some((t, r)) = app.selected_region else { return };
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let start = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map_or(0, |rg| rg.start);
    let Some(mut pos) = parse_position(text, sig) else {
        app.tell("Type a position as bar beat division tick");
        return;
    };
    if state(&app.ctx).relative {
        pos += start;
    }
    app.checkpoint("Event Position");
    let mut idx = i;
    if let Some(region) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        if let Some(n) = region.notes.get_mut(i) {
            n.pos = pos;
            let keep = *n;
            region.notes.sort_by_key(|n| (n.pos, n.pitch));
            idx = region.notes.iter().position(|n| *n == keep).unwrap_or(i);
        }
    }
    app.selected_notes = vec![idx];
    app.after_edit();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_parse() {
        assert_eq!(parse_position("1 1 1 1", (4, 4)), Some(0));
        assert_eq!(parse_position("2 3 2 11", (4, 4)), Some(3840 + 1920 + 240 + 10));
        assert_eq!(parse_position("3", (3, 4)), Some(2 * 2880));
        assert_eq!(parse_position("0 1", (4, 4)), None);
        assert_eq!(parse_position("x", (4, 4)), None);
    }
}
