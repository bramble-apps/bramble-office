//! Region edits of the Tracks area, as Logic's: Loop by dragging a region's upper right corner
//! (the repetitions drawn as ghosts), fades and crossfades (Fade tool drags, the corner
//! handles, Ctrl-Shift or Option-drag for the curve, Apply Default Crossfade, Remove Fades…),
//! region gain (±1 / ±0.1 dB, Normalize Region Gain…), Reverse, the Marquee tool (select a
//! time range over tracks: Delete, Split, Crop, Set Locators) and Bounce in Place.
//!
//! An audio region's fades, gain and reverse are kept with its other audio settings
//! (audioedit::RegionAudio); MIDI regions loop through lg-core's RegionProps.

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Rect, Sense, Stroke};
use lg_audio::fade::{FadeType, RegionFades};
use lg_core::{Region, TrackKind};

use crate::app::App;
use crate::audioedit::{self, RegionAudio};
use crate::edit::Tool;
use crate::look;

/// Logic's default crossfade (Settings ▸ Audio ▸ Editing: 20 ms).
pub const DEFAULT_CROSSFADE_MS: u16 = 20;

#[derive(Clone, Debug, PartialEq)]
pub struct Marquee {
    /// The time range (ticks) and the tracks it covers.
    pub from: u32,
    pub to: u32,
    pub tracks: Vec<usize>,
}

#[derive(Clone, Copy, Debug)]
enum RDrag {
    Loop { t: usize, r: usize },
    Fade { t: usize, r: usize, out: bool },
    Crossfade { t: usize, a: usize, b: usize, at: u32 },
    Curve { t: usize, r: usize, out: bool, orig: i8, y0: f32 },
    Marquee { tick: u32, y: f32 },
}

#[derive(Default)]
pub struct State {
    pub marquee: Option<Marquee>,
    drag: Option<RDrag>,
    /// The last fade edited (Apply Last Edited Fade Again): which side, its settings.
    last_fade: Option<(bool, RegionFades)>,
    /// Normalize Region Gain…: the sheet open, its level (dB, peak), used once.
    pub normalize_open: bool,
    pub normalize_db: f32,
    normalized: bool,
}

// ---------------------------------------------------------------------------------------------
// helpers

fn region(app: &App, t: usize, r: usize) -> Option<&Region> {
    app.project.as_ref()?.tracks.get(t)?.regions.get(r)
}

/// `seconds` for other modules (Stem Splitter).
pub(crate) fn seconds_pub(app: &App, a: u32, b: u32) -> f64 {
    seconds(app, a, b)
}

fn seconds(app: &App, a: u32, b: u32) -> f64 {
    app.seconds_at(b as f64) - app.seconds_at(a as f64)
}

/// The selected audio regions.
fn audio_selection(app: &App) -> Vec<(usize, usize)> {
    app.region_selection().into_iter().filter(|&(t, r)| region(app, t, r).is_some_and(|x| x.audio.is_some())).collect()
}

/// An audio file's rate and length (from the audio editors' cache, else read).
fn file_info(app: &App, path: &std::path::Path) -> Option<(u32, f64)> {
    if let (Some(r), Some(s)) = (audioedit::rate_of(app, path), audioedit::loaded_seconds(app, path)) {
        return Some((r, s));
    }
    let a = lg_audio::AudioFile::read(path).ok()?;
    Some((a.rate, a.seconds()))
}

/// Where a looped region's repetitions end: the end it was dragged to, else the next region on
/// the track, else the project's end.
pub fn loop_end(app: &App, t: usize, r: usize) -> u32 {
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return 0 };
    let Some(rg) = tr.regions.get(r) else { return 0 };
    let end = rg.start + rg.length;
    let next = tr.regions.iter().map(|o| o.start).filter(|&s| s >= end).min();
    let song_end = app.project.as_ref().map_or(0, |p| p.tracks.iter().flat_map(|t| t.regions.iter().map(|r| r.start + r.length)).max().unwrap_or(0)).max(64 * app.bar_ticks());
    let limit = next.unwrap_or(song_end);
    if rg.props.loop_to > end { rg.props.loop_to.min(limit) } else { limit }
}

// ---------------------------------------------------------------------------------------------
// drawing and drags in the tracks area

/// A looped region's repetitions after it: ghost copies (lighter), a notch at each start.
pub fn draw_loops(app: &App, p: &egui::Painter, rr: Rect, t: usize, r: usize, x_of: &dyn Fn(f32) -> f32) {
    let Some(rg) = region(app, t, r) else { return };
    if !rg.props.looped || rg.length == 0 {
        return;
    }
    let until = loop_end(app, t, r);
    let kind = app.project.as_ref().map_or(TrackKind::Instrument, |p| p.tracks[t].kind);
    // (Settings ▸ View ▸ Tracks ▸ Shaded loops: the repetitions shaded, else in the region's colour)
    let col = if app.logic_settings.on(crate::tracksview::SHADED) { look::region_color(kind).gamma_multiply(0.45) } else { look::region_color(kind) };
    let mut at = rg.start + rg.length;
    let mut k = 0;
    // (skip straight to the first repetition in view: scrolled far right, the early ones are off)
    let left = p.clip_rect().min.x;
    if x_of(until as f32) > left && x_of(at as f32) < left {
        let px_per_tick = (x_of(until as f32) - x_of(at as f32)) / (until - at).max(1) as f32;
        if px_per_tick > 0.0 {
            let skip = (((left - x_of(at as f32)) / px_per_tick) as u32 / rg.length).saturating_sub(1);
            at += skip * rg.length;
        }
    }
    while at < until && k < 4096 {
        let b = (at + rg.length).min(until);
        let g = Rect::from_min_max(pos2(x_of(at as f32), rr.min.y), pos2(x_of(b as f32), rr.max.y));
        if g.min.x > p.clip_rect().max.x {
            break;
        }
        if g.max.x < left {
            at += rg.length;
            k += 1;
            continue;
        }
        p.rect_filled(g.shrink2(vec2(0.5, 0.0)), 4.0, col);
        // Logic's ▾ at each repetition's start
        p.add(egui::Shape::convex_polygon(vec![pos2(g.min.x, g.min.y), pos2(g.min.x + 6.0, g.min.y), pos2(g.min.x, g.min.y + 6.0)], look::text_dim(), Stroke::NONE));
        // the notes again, faint
        let bar_t = app.bar_ticks() as f32;
        for n in rg.notes.iter().take(400) {
            let x0 = x_of((n.pos - rg.start + at) as f32);
            if x0 > g.max.x {
                continue;
            }
            let lo = rg.notes.iter().map(|n| n.pitch).min().unwrap_or(60) as f32;
            let hi = (rg.notes.iter().map(|n| n.pitch).max().unwrap_or(72) as f32).max(lo + 12.0);
            let y = g.max.y - 4.0 - (n.pitch as f32 - lo) / (hi - lo) * (g.height() - 22.0);
            let x1 = (x0 + (n.len as f32 / bar_t * app.bar_w).max(1.5)).min(g.max.x);
            p.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(1.4, Color32::from_black_alpha(90)));
        }
        at += rg.length;
        k += 1;
    }
}

/// The loop handle (the upper half of a region's right edge, Logic's loop cursor): drag right
/// to loop the region up to there; back inside its own length: Loop off.
pub fn loop_handle(app: &mut App, ui: &egui::Ui, rr: Rect, clip: Rect, t: usize, r: usize, tick_of: &dyn Fn(f32) -> f32) {
    let zone = Rect::from_min_max(pos2(rr.max.x - 7.0, rr.min.y), pos2(rr.max.x + 2.0, rr.center().y)).intersect(clip);
    let resp = ui.interact(zone, ui.scope_id().with(("region-loop", t, r)), Sense::drag()).on_hover_text("Loop: drag to the right");
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Alias);
        ui.painter().add(egui::Shape::line(vec![pos2(rr.max.x - 6.0, rr.min.y + 9.0), pos2(rr.max.x - 3.0, rr.min.y + 4.0), pos2(rr.max.x, rr.min.y + 9.0)], Stroke::new(1.4, Color32::WHITE)));
    }
    if resp.drag_started() {
        app.checkpoint("Loop Region");
        app.ta.regions.drag = Some(RDrag::Loop { t, r });
        app.selected_region = Some((t, r));
        app.selected_regions = vec![(t, r)];
    }
    if let (Some(RDrag::Loop { t: dt, r: dr }), true, Some(pp)) = (app.ta.regions.drag, resp.dragged(), resp.interact_pointer_pos()) {
        if (dt, dr) == (t, r) {
            let Some(rg) = region(app, t, r) else { return };
            let (start, len) = (rg.start, rg.length);
            let bar = app.bar_ticks();
            let beat = bar / app.project.as_ref().map_or(4, |p| p.signature.0.max(1) as u32);
            let free = ui.input(|i| i.modifiers.shift);
            let mut to = tick_of(pp.x).max(0.0) as u32;
            if !free {
                to = (to + beat / 2) / beat * beat;
            }
            let on = to > start + len + beat / 2;
            if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                x.props.looped = on;
                x.props.loop_to = if on { to } else { 0 };
            }
            let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
            app.help_tag = Some(if on {
                let reps = (to - start) as f32 / len.max(1) as f32;
                vec![("Loop:".into(), format!("{reps:.2} ×")), ("End:".into(), crate::app::bbdt(to, sig))]
            } else {
                vec![("Loop Off".into(), String::new())]
            });
        }
    }
    if resp.drag_stopped() {
        app.ta.regions.drag = None;
        app.after_edit();
    }
}

/// An audio region's fades in the tracks area: drawn over it (darker outside the curve), and
/// edited: the Fade tool drags in from an edge (or across two touching regions: a crossfade),
/// the upper corners are fade handles for the Pointer, Ctrl-Shift- or Option-drag inside a fade
/// bends its curve.
pub fn fades(app: &mut App, ui: &egui::Ui, rr: Rect, clip: Rect, t: usize, r: usize, tool: Tool, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    let Some(rg) = region(app, t, r).cloned() else { return };
    if rg.audio.is_none() {
        return;
    }
    let x = audioedit::extras(app, t, r);
    let secs = seconds(app, rg.start, rg.start + rg.length).max(1e-6);
    let sec_x = |s: f64| rr.min.x + (s / secs) as f32 * rr.width();
    let p = ui.painter_at(clip);
    let body = Rect::from_min_max(pos2(rr.min.x, rr.min.y + 15.0), rr.max);
    // the fades drawn: a curve from the bottom corner up to full height; outside it darker
    for out in [false, true] {
        let ms = if out { x.fades.out_ms } else { x.fades.in_ms };
        if ms == 0 {
            continue;
        }
        let f = (ms as f64 / 1000.0).min(secs);
        let mut line = Vec::new();
        let mut shade = Vec::new();
        for k in 0..=24 {
            let s = if out { secs - f + f * k as f64 / 24.0 } else { f * k as f64 / 24.0 };
            let g = x.fades.gain_at(s.clamp(0.0, secs), secs);
            let pt = pos2(sec_x(s), body.max.y - g * body.height());
            line.push(pt);
            shade.push((pt, pos2(pt.x, body.min.y)));
        }
        for w in shade.windows(2) {
            let quad = vec![w[0].1, w[1].1, w[1].0, w[0].0];
            p.add(egui::Shape::convex_polygon(quad, Color32::from_black_alpha(70), Stroke::NONE));
        }
        let xfade = if out { x.fades.out_type != FadeType::Out } else { x.fades.in_type != FadeType::Out };
        p.add(egui::Shape::line(line, Stroke::new(1.3, if xfade { Color32::from_rgb(0xff, 0xd2, 0x4a) } else { Color32::WHITE })));
    }
    let shift_ctrl = ui.input(|i| (i.modifiers.ctrl && i.modifiers.shift) || i.modifiers.alt);
    // the zones: the Fade tool takes the whole region (a press near a touching neighbour's edge:
    // a crossfade); the Pointer the upper corners (Settings ▸ Editing ▸ Fade tool click zones)
    let next = app.project.as_ref().and_then(|pp| {
        pp.tracks[t].regions.iter().enumerate().find(|(k, o)| *k != r && o.audio.is_some() && o.start <= rg.start + rg.length + 2 && o.start + 2 >= rg.start + rg.length.saturating_sub(rg.length / 2) && o.start > rg.start).map(|(k, _)| k)
    });
    let zones: Vec<(Rect, &str)> = if tool == Tool::Fade {
        vec![(Rect::from_min_max(rr.min, pos2(rr.center().x, rr.max.y)), "in"), (Rect::from_min_max(pos2(rr.center().x, rr.min.y), rr.max), "out")]
    } else if app.logic_settings.on("General / Editing / Fade tool click zones") {
        vec![(Rect::from_min_size(rr.min, vec2(10.0, 12.0)), "in"), (Rect::from_min_max(pos2(rr.max.x - 18.0, rr.min.y), pos2(rr.max.x - 7.0, rr.min.y + 12.0)), "out")]
    } else {
        Vec::new()
    };
    for (zone, side) in zones {
        let zone = zone.intersect(clip);
        if zone.width() <= 0.0 {
            continue;
        }
        let resp = ui.interact(zone, ui.scope_id().with(("fade-zone", t, r, side)), Sense::click_and_drag());
        let out = side == "out";
        if resp.hovered() && tool != Tool::Fade {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            let hx = if out { sec_x(secs - x.fades.out_ms as f64 / 1000.0) } else { sec_x(x.fades.in_ms as f64 / 1000.0) };
            ui.painter().circle_filled(pos2(hx, rr.min.y + 6.0), 4.0, Color32::WHITE);
        }
        if resp.drag_started() {
            audioedit::checkpoint(app, if shift_ctrl { "Region Fade Curve" } else { "Region Fade" });
            app.selected_region = Some((t, r));
            app.selected_regions = vec![(t, r)];
            let press = ui.input(|i| i.pointer.press_origin()).unwrap_or(rr.center());
            let fade_len = if out { x.fades.out_ms } else { x.fades.in_ms };
            app.ta.regions.drag = Some(if shift_ctrl && fade_len > 0 {
                RDrag::Curve { t, r, out, orig: if out { x.fades.out_curve } else { x.fades.in_curve }, y0: press.y }
            } else if let (true, true, Some(b)) = (tool == Tool::Fade, out, next.filter(|_| (press.x - rr.max.x).abs() < 14.0)) {
                RDrag::Crossfade { t, a: r, b, at: rg.start + rg.length }
            } else {
                RDrag::Fade { t, r, out }
            });
        }
        if !resp.dragged() {
            if resp.drag_stopped() {
                app.ta.regions.drag = None;
                app.load_engine();
            }
            continue;
        }
        let Some(pp) = resp.interact_pointer_pos() else { continue };
        match app.ta.regions.drag {
            Some(RDrag::Fade { t: dt, r: dr, out: o }) if (dt, dr, o) == (t, r, out) => {
                let tick = tick_of(pp.x).max(0.0) as u32;
                let s = if out { seconds(app, tick.clamp(rg.start, rg.start + rg.length), rg.start + rg.length) } else { seconds(app, rg.start, tick.clamp(rg.start, rg.start + rg.length)) };
                let ms = (s * 1000.0).round().clamp(0.0, u16::MAX as f64) as u16;
                let mut nx = audioedit::extras(app, t, r);
                if out {
                    nx.fades.out_ms = ms;
                } else {
                    nx.fades.in_ms = ms;
                }
                app.ta.regions.last_fade = Some((out, nx.fades));
                app.help_tag = Some(vec![(if out { "Fade Out:" } else { "Fade In:" }.into(), format!("{ms} ms"))]);
                audioedit::set_extras(app, t, r, nx);
            }
            Some(RDrag::Curve { t: dt, r: dr, out: o, orig, y0 }) if (dt, dr, o) == (t, r, out) => {
                let c = (orig as f32 + (pp.y - y0) * if out { -1.0 } else { 1.0 }).round().clamp(-99.0, 99.0) as i8;
                let mut nx = audioedit::extras(app, t, r);
                if out {
                    nx.fades.out_curve = c;
                } else {
                    nx.fades.in_curve = c;
                }
                app.ta.regions.last_fade = Some((out, nx.fades));
                app.help_tag = Some(vec![("Curve:".into(), format!("{c}"))]);
                audioedit::set_extras(app, t, r, nx);
            }
            Some(RDrag::Crossfade { t: dt, a, b, at }) if dt == t && a == r => {
                let half = seconds(app, at.min(tick_of(pp.x).max(0.0) as u32), at.max(tick_of(pp.x).max(0.0) as u32));
                let ms = (half * 2000.0).round().clamp(1.0, 10_000.0) as u16;
                crossfade(app, t, a, b, ms, FadeType::X);
                app.help_tag = Some(vec![("Crossfade:".into(), format!("{ms} ms"))]);
            }
            _ => {}
        }
        let _ = x_of;
    }
}

/// Crossfades regions `a` and `b` (b after a on track t) over `ms`: where they overlap, over
/// the overlap; where they only touch, each is lengthened by half of it into the other when
/// its file has the audio (as Logic needs material from both sides).
/// Settings ▸ Audio ▸ Editing ▸ Crossfade Time and Crossfade Curve: the default crossfade.
pub fn default_crossfade(v: &crate::settings::Values) -> (u16, i8) {
    let ms = v.get("Audio / Editing / Crossfade Time:").trim().parse::<f32>().unwrap_or(DEFAULT_CROSSFADE_MS as f32).clamp(1.0, 200.0);
    let curve = v.get("Audio / Editing / Crossfade Curve:").trim().parse::<f32>().unwrap_or(0.0).clamp(-100.0, 100.0);
    (ms.round() as u16, curve.round() as i8)
}

pub fn crossfade(app: &mut App, t: usize, a: usize, b: usize, ms: u16, kind: FadeType) {
    let (Some(ra), Some(rb)) = (region(app, t, a).cloned(), region(app, t, b).cloned()) else { return };
    let (Some(fa), Some(fb)) = (ra.audio.as_ref().map(|x| x.file.clone()), rb.audio.as_ref().map(|x| x.file.clone())) else { return };
    let (xa, xb) = (audioedit::extras(app, t, a), audioedit::extras(app, t, b));
    let a_end = ra.start + ra.length;
    let tempo_ticks = |s: f64| (s * app.tempo() / 60.0 * lg_core::events::PPQ as f64) as u32;
    let mut overlap = a_end.saturating_sub(rb.start);
    if overlap == 0 {
        // touching: half the crossfade into each side, as far as the files have audio
        let half = ms as f64 / 2000.0;
        let (Some((rate_a, len_a)), Some((rate_b, _))) = (file_info(app, &fa), file_info(app, &fb)) else { return };
        let a_secs = seconds(app, ra.start, a_end);
        let room_a = (len_a - xa.offset as f64 / rate_a as f64 - a_secs).max(0.0).min(half);
        let room_b = (xb.offset as f64 / rate_b as f64).min(half);
        let (da, db) = (tempo_ticks(room_a), tempo_ticks(room_b));
        if let Some(tr) = app.project.as_mut().map(|p| &mut p.tracks[t]) {
            tr.regions[a].length += da;
            tr.regions[b].start -= db.min(tr.regions[b].start);
            tr.regions[b].length += db;
        }
        let mut nb = xb.clone();
        nb.offset = nb.offset.saturating_sub((room_b * rate_b as f64) as usize);
        audioedit::set_extras(app, t, b, nb);
        overlap = da + db;
    }
    let ra = region(app, t, a).cloned().unwrap_or(ra);
    let rb = region(app, t, b).cloned().unwrap_or(rb);
    let ms = if overlap > 0 { (seconds(app, rb.start, (rb.start + overlap).min(ra.start + ra.length)) * 1000.0).round() as u16 } else { ms };
    let (mut na, mut nb) = (audioedit::extras(app, t, a), audioedit::extras(app, t, b));
    lg_audio::fade::crossfade(&mut na.fades, &mut nb.fades, ms.max(1), kind);
    audioedit::set_extras(app, t, a, na);
    audioedit::set_extras(app, t, b, nb);
}

/// The Marquee tool over the lanes: drag a time range across tracks (snapped to the division;
/// Shift: free); a click clears it. Drawn as Logic's white band.
pub fn marquee(app: &mut App, ui: &mut egui::Ui, lanes: Rect, rows_top: f32, rows: &[crate::trackarea::Row], tick_of: &dyn Fn(f32) -> f32, x_of: &dyn Fn(f32) -> f32, active: bool) {
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    if active {
        let resp = ui.interact(lanes, ui.scope_id().with("marquee"), Sense::click_and_drag());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        let beat = app.bar_ticks() / sig.0.max(1) as u32;
        let div = (beat / 4).max(1);
        // the Snap value; Control: the division; Control-Shift: ticks (regionparams.rs)
        let m = ui.input(|i| i.modifiers);
        let px_per_tick = 1.0 / (tick_of(1.0) - tick_of(0.0)).max(1e-6);
        if resp.drag_started() {
            if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
                app.ta.regions.drag = Some(RDrag::Marquee { tick: crate::regionparams::marquee_snap(app, tick_of(o.x), div, px_per_tick, m), y: o.y });
            }
        }
        if let (true, Some(RDrag::Marquee { tick, y }), Some(pp)) = (resp.dragged(), app.ta.regions.drag, resp.interact_pointer_pos()) {
            let now = crate::regionparams::marquee_snap(app, tick_of(pp.x), div, px_per_tick, m);
            let (y0, y1) = (y.min(pp.y) - rows_top + app.scroll_y, y.max(pp.y) - rows_top + app.scroll_y);
            let tracks: Vec<usize> = rows.iter().filter(|r| r.y + r.h > y0 && r.y < y1).map(|r| r.t).collect();
            if now != tick && !tracks.is_empty() {
                app.ta.regions.marquee = Some(Marquee { from: tick.min(now), to: tick.max(now), tracks });
                app.help_tag = Some(vec![("Marquee".into(), String::new()), ("Start:".into(), crate::app::bbdt(tick.min(now), sig)), ("Length:".into(), crate::app::bbdt_len(tick.abs_diff(now), sig))]);
            }
        }
        if resp.drag_stopped() {
            app.ta.regions.drag = None;
        }
        if resp.clicked() {
            // Shift-click: the marquee's end nearest (Option-Shift: farther) moves there, or it
            // grows to the click; a click inside splits the regions at its borders
            let at = resp.interact_pointer_pos().map(|pp| (crate::regionparams::marquee_snap(app, tick_of(pp.x), div, px_per_tick, m), rows.iter().find(|r| pp.y - rows_top + app.scroll_y >= r.y && pp.y - rows_top + app.scroll_y < r.y + r.h).map(|r| r.t)));
            if !at.is_some_and(|(tick, t)| crate::regionparams::marquee_click(app, tick, t, m)) {
                app.ta.regions.marquee = None;
                if let Some((tick, _)) = at {
                    app.locate(tick as f64);
                }
            }
        }
    }
    let Some(m) = app.ta.regions.marquee.clone() else { return };
    let p = ui.painter_at(lanes);
    for r in rows.iter().filter(|r| m.tracks.contains(&r.t)) {
        let y = rows_top + r.y - app.scroll_y;
        let band = Rect::from_min_max(pos2(x_of(m.from as f32), y + 1.0), pos2(x_of(m.to as f32), y + r.h - 1.0));
        p.rect_filled(band, 0.0, Color32::from_white_alpha(60));
        p.rect_stroke(band, 0.0, Stroke::new(1.0, Color32::from_white_alpha(170)), egui::StrokeKind::Inside);
    }
}

// ---------------------------------------------------------------------------------------------
// cutting regions by a range (the marquee)

/// A region cut to `a..b` (ticks), with its audio settings: notes outside left out, an audio
/// region's file offset moved on by `skip` frames, the fades that no longer touch an edge dropped.
fn piece(rg: &Region, x: &RegionAudio, a: u32, b: u32, skip: usize) -> (Region, RegionAudio) {
    let mut n = rg.clone();
    let mut nx = x.clone();
    n.start = a;
    n.length = b - a;
    n.notes.retain(|k| k.pos >= a && k.pos < b);
    if a > rg.start {
        nx.offset += skip;
        nx.fades.in_ms = 0;
        nx.fades.in_type = FadeType::Out;
    }
    if b < rg.start + rg.length {
        nx.fades.out_ms = 0;
        nx.fades.out_type = FadeType::Out;
    }
    (n, nx)
}

/// What the marquee does to a track's regions: Delete (the range out), Crop (only the range
/// kept), Split (cut at its edges).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cut {
    Delete,
    Crop,
    Split,
}

/// A track's regions (with their audio settings) cut by `from..to`; `skip(region, tick)`: how
/// many frames of its file a region plays from its start to `tick`. The first piece of each
/// region keeps its place in the file it was read from; the others are new regions modelled on
/// it (Save writes them as copies).
pub fn cut(list: &[(Region, RegionAudio)], from: u32, to: u32, how: Cut, skip: &dyn Fn(&Region, u32) -> usize) -> Vec<(Region, RegionAudio)> {
    let mut out: Vec<(Region, RegionAudio)> = Vec::new();
    for (rg, x) in list {
        let (s, e) = (rg.start, rg.start + rg.length);
        if e <= from || s >= to {
            if how != Cut::Crop {
                out.push((rg.clone(), x.clone()));
            }
            continue;
        }
        let (a, b) = (s.max(from), e.min(to));
        if s < a && how != Cut::Crop {
            out.push(piece(rg, x, s, a, 0));
        }
        if how != Cut::Delete {
            out.push(piece(rg, x, a, b, skip(rg, a)));
        }
        if b < e && how != Cut::Crop {
            out.push(piece(rg, x, b, e, skip(rg, b)));
        }
    }
    let mut seen = std::collections::HashSet::new();
    for (rg, _) in out.iter_mut() {
        if let Some(src) = rg.source {
            if !seen.insert(src) {
                rg.origin = Some(src);
                rg.source = None;
            }
        }
    }
    out
}

/// Cuts track `t`'s regions by `from..to`; the audio settings follow the pieces.
pub fn cut_track(app: &mut App, t: usize, from: u32, to: u32, how: Cut) {
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return };
    let list: Vec<(Region, RegionAudio)> = tr.regions.iter().enumerate().map(|(r, rg)| (rg.clone(), audioedit::extras(app, t, r))).collect();
    let skip = |rg: &Region, tick: u32| -> usize {
        let rate = rg.audio.as_ref().and_then(|a| file_info(app, &a.file)).map_or(0, |i| i.0);
        (seconds(app, rg.start, tick) * rate as f64).round() as usize
    };
    let out = cut(&list, from, to, how, &skip);
    app.audio_edit.regions.retain(|k, _| k.0 != t);
    for (k, (_, x)) in out.iter().enumerate() {
        if !x.file.as_os_str().is_empty() {
            app.audio_edit.regions.insert((t, k), x.clone());
        }
    }
    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
        tr.regions = out.into_iter().map(|x| x.0).collect();
    }
}

/// The marquee's range applied to its tracks (and the automation shown on them).
pub fn apply_marquee(app: &mut App, how: Cut) -> bool {
    let Some(m) = app.ta.regions.marquee.clone() else { return false };
    audioedit::checkpoint(app, match how {
        Cut::Delete => "Delete Marquee Selection",
        Cut::Crop => "Crop",
        Cut::Split => "Split",
    });
    for &t in &m.tracks {
        cut_track(app, t, m.from, m.to, how);
        if how == Cut::Delete && app.show_automation {
            crate::automation::delete_marquee(app, t, m.from, m.to);
        }
    }
    app.selected_region = None;
    app.selected_regions.clear();
    if how != Cut::Split {
        app.ta.regions.marquee = None;
    }
    app.after_edit();
    true
}

// ---------------------------------------------------------------------------------------------
// commands

fn each_audio(app: &mut App, what: &str, mut f: impl FnMut(&mut RegionAudio)) -> bool {
    let sel = audio_selection(app);
    if sel.is_empty() {
        return false;
    }
    audioedit::checkpoint(app, what);
    for (t, r) in sel {
        let mut x = audioedit::extras(app, t, r);
        f(&mut x);
        audioedit::set_extras(app, t, r, x);
    }
    app.load_engine();
    true
}

/// The selected regions' peak (linear), as they play without their gain.
fn region_peak(app: &App, t: usize, r: usize) -> Option<f32> {
    let rg = region(app, t, r)?;
    let a = rg.audio.as_ref()?;
    let x = audioedit::extras(app, t, r);
    let (mode, flex, notes) = crate::flex::plan_for(app, t, rg, &x);
    let plan = audioedit::Plan { file: a.file.clone(), offset: x.offset, seconds: seconds(app, rg.start, rg.start + rg.length), gain_db: 0.0, fades: RegionFades::default(), flex, reverse: false, mode, notes, params: crate::flex::params_for(app, t).0, keep_unvoiced: crate::flex::params_for(app, t).1 };
    let s = plan.render()?;
    Some(s.iter().fold(0f32, |m, v| m.max(v.abs())))
}

fn normalize(app: &mut App, db: f32) -> bool {
    let sel = audio_selection(app);
    if sel.is_empty() {
        return false;
    }
    let peaks: Vec<((usize, usize), Option<f32>)> = sel.iter().map(|&(t, r)| ((t, r), region_peak(app, t, r))).collect();
    audioedit::checkpoint(app, "Normalize Region Gain");
    for ((t, r), peak) in peaks {
        let Some(peak) = peak.filter(|p| *p > 1e-6) else { continue };
        let mut x = audioedit::extras(app, t, r);
        x.gain_db = ((db - 20.0 * peak.log10()) * 10.0).round() / 10.0;
        audioedit::set_extras(app, t, r, x);
    }
    app.ta.regions.normalized = true;
    app.ta.regions.normalize_db = db;
    app.load_engine();
    true
}

/// Bounce Regions in Place…: the selected regions of each track rendered (that track alone,
/// its instrument and effects) into an audio file in the project, on a new audio track under
/// it; the originals muted (Logic's default choice).
fn bounce_in_place(app: &mut App, whole_tracks: bool) -> bool {
    let mut sel = app.region_selection();
    if whole_tracks {
        sel = crate::trackarea::selection(app).into_iter().flat_map(|t| (0..app.project.as_ref().map_or(0, |p| p.tracks[t].regions.len())).map(move |r| (t, r))).collect();
    }
    sel.retain(|&(t, r)| region(app, t, r).is_some_and(|x| x.audio.is_none()));
    if sel.is_empty() {
        return false;
    }
    let mut tracks: Vec<usize> = sel.iter().map(|x| x.0).collect();
    tracks.dedup();
    let dir = match (&app.file, app.project.as_ref().and_then(|p| p.audio_dir.clone())) {
        (_, Some(d)) => d,
        (Some((alt, _)), None) => lg_core::project::package_of(alt).join("Media/Audio Files"),
        _ => std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join(".local/share/bramble-studio/Bounces"),
    };
    let _ = std::fs::create_dir_all(&dir);
    app.checkpoint(if whole_tracks { "Bounce Tracks in Place" } else { "Bounce Regions in Place" });
    let engine_tracks = app.engine.lock().map(|e| e.tracks.clone()).unwrap_or_default();
    let sf = app.engine.lock().ok().and_then(|e| e.soundfont.clone());
    let (tempo, map) = app.project.as_ref().map_or((120.0, Vec::new()), |p| (p.tempo, p.tempo_map.clone()));
    // bottom first, so the indexes above stay right
    for &t in tracks.iter().rev() {
        let mine: Vec<usize> = sel.iter().filter(|x| x.0 == t).map(|x| x.1).collect();
        let Some(tr) = app.project.as_ref().map(|p| p.tracks[t].clone()) else { continue };
        let from = mine.iter().map(|&r| tr.regions[r].start).min().unwrap_or(0);
        let to = mine.iter().map(|&r| tr.regions[r].start + tr.regions[r].length).max().unwrap_or(from);
        let mut solo = engine_tracks.clone();
        for (i, pt) in solo.iter_mut().enumerate() {
            pt.mute = i != t;
            pt.solo = false;
            if i == t {
                // only the chosen regions
                pt.notes.retain(|n| mine.iter().any(|&r| n.0 >= tr.regions[r].start && n.0 < tr.regions[r].start + tr.regions[r].length));
                pt.auto_volume.clear();
                pt.volume = 1.0;
                pt.pan = 0.0;
                pt.mute = false;
            }
        }
        let samples = crate::audio::bounce(solo, tempo, map.clone(), sf.as_deref(), from, to);
        let base = format!("{}_bip", tr.regions[mine[0]].name.trim());
        let mut path = dir.join(format!("{base}.wav"));
        let mut k = 1;
        while path.exists() {
            path = dir.join(format!("{base} {k}.wav"));
            k += 1;
        }
        if let Err(e) = std::fs::write(&path, crate::audio::wav_bytes(&samples)) {
            app.tell(format!("Couldn't write the bounce: {e}"));
            return true;
        }
        let secs = samples.len() as f64 / 2.0 / crate::audio::RATE as f64;
        let mut nt = lg_core::Track::new(&format!("{}_bip", tr.name), TrackKind::Audio);
        nt.extra.depth = tr.extra.depth;
        nt.regions.push(Region { name: base, start: from, length: to - from, notes: Vec::new(), audio: Some(lg_core::project::AudioRef { file: path, seconds: secs }), muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
        if let Some(p) = app.project.as_mut() {
            for &r in &mine {
                p.tracks[t].regions[r].muted = true;
            }
            let at = lg_core::arrange::group(&p.tracks, t).end;
            p.tracks.insert(at, nt);
            app.programs.insert(at.min(app.programs.len()), 0);
        }
    }
    app.selected_region = None;
    app.selected_regions.clear();
    app.after_edit();
    app.tell("Bounced in place");
    true
}

/// The region commands. False: not one of these (or nothing to work on).
pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Reverse On/Off" | "Reverse" => each_audio(app, "Reverse", |x| x.reverse = !x.reverse),
        "Region Gain +1 dB" | "Region Gain -1 dB" | "Region Gain +0.1 dB" | "Region Gain -0.1 dB" => {
            let d: f32 = name.trim_start_matches("Region Gain ").trim_end_matches(" dB").parse().unwrap_or(0.0);
            each_audio(app, "Region Gain", |x| x.gain_db = ((x.gain_db + d).clamp(-96.0, 30.0) * 10.0).round() / 10.0)
        }
        "Normalize Region/Cell Gain…" | "Normalize Region Gain…" => {
            if audio_selection(app).is_empty() {
                return false;
            }
            app.ta.regions.normalize_open = true;
            true
        }
        "Apply Normalize Region/Cell Gain Again" | "Apply Normalize Region Gain Again" => {
            let db = app.ta.regions.normalize_db;
            normalize(app, db)
        }
        "Apply Default Crossfade" => {
            let sel = audio_selection(app);
            if sel.is_empty() {
                return false;
            }
            audioedit::checkpoint(app, "Apply Default Crossfade");
            let (dms, dcurve) = default_crossfade(&app.logic_settings);
            let mut done = 0;
            for &(t, a) in &sel {
                let Some(ra) = region(app, t, a).cloned() else { continue };
                let end = ra.start + ra.length;
                let b = app.project.as_ref().and_then(|p| p.tracks[t].regions.iter().enumerate().find(|(k, o)| *k != a && o.audio.is_some() && o.start > ra.start && o.start <= end + 2).map(|(k, _)| k));
                if let Some(b) = b {
                    crossfade(app, t, a, b, dms, FadeType::X);
                    let (mut na, mut nb) = (audioedit::extras(app, t, a), audioedit::extras(app, t, b));
                    na.fades.out_curve = dcurve;
                    nb.fades.in_curve = dcurve;
                    audioedit::set_extras(app, t, a, na);
                    audioedit::set_extras(app, t, b, nb);
                    done += 1;
                }
            }
            if done == 0 {
                // (no neighbours: short fades in and out, as Logic's default on single regions)
                for (t, r) in sel {
                    let mut x = audioedit::extras(app, t, r);
                    x.fades.in_ms = x.fades.in_ms.max(dms);
                    x.fades.out_ms = x.fades.out_ms.max(dms);
                    audioedit::set_extras(app, t, r, x);
                }
            }
            app.after_edit();
            true
        }
        "Apply Last Edited Fade Again" => {
            let Some((out, f)) = app.ta.regions.last_fade else { return false };
            each_audio(app, "Apply Last Edited Fade Again", |x| {
                if out {
                    x.fades.out_ms = f.out_ms;
                    x.fades.out_curve = f.out_curve;
                    x.fades.out_type = f.out_type;
                } else {
                    x.fades.in_ms = f.in_ms;
                    x.fades.in_curve = f.in_curve;
                }
            })
        }
        "Remove Fades" => each_audio(app, "Remove Fades", |x| x.fades = RegionFades::default()),
        "Remove Fade Ins" | "Remove Fade-In" => each_audio(app, "Remove Fade-In", |x| {
            x.fades.in_ms = 0;
            x.fades.in_curve = 0;
            x.fades.in_type = FadeType::Out;
        }),
        "Remove Fade Outs" | "Remove Fade-Out" => each_audio(app, "Remove Fade-Out", |x| {
            x.fades.out_ms = 0;
            x.fades.out_curve = 0;
            x.fades.out_type = FadeType::Out;
        }),
        "Remove Crossfades" | "Remove Crossfade" => each_audio(app, "Remove Crossfade", |x| {
            if x.fades.out_type != FadeType::Out {
                x.fades.out_ms = 0;
                x.fades.out_type = FadeType::Out;
            }
            if x.fades.in_type != FadeType::Out {
                x.fades.in_ms = 0;
                x.fades.in_type = FadeType::Out;
            }
        }),
        "Bounce Regions/Cells in Place…" | "Bounce In Place…" | "Bounce Regions in Place…" => bounce_in_place(app, false),
        "Bounce Tracks in Place" => bounce_in_place(app, true),
        "Set Marquee to Selection" => {
            let sel = app.region_selection();
            let p = app.project.as_ref();
            let spans: Vec<(usize, u32, u32)> = sel.iter().filter_map(|&(t, r)| p.and_then(|p| p.tracks.get(t)?.regions.get(r)).map(|x| (t, x.start, x.start + x.length))).collect();
            let (Some(a), Some(b)) = (spans.iter().map(|s| s.1).min(), spans.iter().map(|s| s.2).max()) else { return false };
            let mut tracks: Vec<usize> = spans.iter().map(|s| s.0).collect();
            tracks.dedup();
            app.ta.regions.marquee = Some(Marquee { from: a, to: b, tracks });
            true
        }
        _ => false,
    }
}

pub fn enabled(app: &App, label: &str) -> Option<bool> {
    Some(match label {
        "Reverse" | "Reverse On/Off" | "Normalize Region Gain…" | "Normalize Region/Cell Gain…" | "Remove Fades" | "Remove Fade-In" | "Remove Fade-Out" | "Remove Crossfade" | "Apply Default Crossfade" => !audio_selection(app).is_empty(),
        "Apply Normalize Region Gain Again" | "Apply Normalize Region/Cell Gain Again" => app.ta.regions.normalized && !audio_selection(app).is_empty(),
        "Apply Last Edited Fade Again" => app.ta.regions.last_fade.is_some() && !audio_selection(app).is_empty(),
        _ => return None,
    })
}

pub fn checked(app: &App, label: &str) -> Option<bool> {
    match label {
        "Reverse" => {
            let sel = audio_selection(app);
            Some(!sel.is_empty() && sel.iter().all(|&(t, r)| audioedit::extras(app, t, r).reverse))
        }
        _ => None,
    }
}

/// The Normalize Region Gain sheet.
pub fn dialogs(app: &mut App, ctx: &egui::Context) {
    if !app.ta.regions.normalize_open {
        return;
    }
    let mut open = true;
    let mut db = app.ta.regions.normalize_db;
    let mut go = false;
    egui::Window::new("Normalize Region Gain").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -80.0)).open(&mut open).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Normalize to peak level:");
            ui.add(egui::DragValue::new(&mut db).range(-60.0..=0.0).speed(0.1).suffix(" dB"));
        });
        ui.label(egui::RichText::new("Sets each selected region's gain so its loudest sample reaches this level.").color(look::text_dim()));
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Normalize").clicked() {
                go = true;
            }
            if ui.button("Cancel").clicked() {
                app.ta.regions.normalize_open = false;
            }
        });
    });
    app.ta.regions.normalize_db = db;
    if !open {
        app.ta.regions.normalize_open = false;
    }
    if go {
        app.ta.regions.normalize_open = false;
        normalize(app, db);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn midi(start: u32, len: u32) -> (Region, RegionAudio) {
        let notes = (0..len / 480).map(|k| lg_core::events::Note { channel: 0, pitch: 60, velocity: 90, pos: start + k * 480, len: 240, muted: false }).collect();
        let src = lg_core::project::Source { folder: 0x40000, pos: start, seq: Some(0x100000), audio_region: 0 };
        (Region { name: "r".into(), start, length: len, notes, audio: None, muted: false, source: Some(src), origin: None, props: Default::default(), ext: Default::default() }, RegionAudio::default())
    }

    #[test]
    fn default_crossfade_follows_settings() {
        let mut v = crate::settings::Values::default();
        assert_eq!(default_crossfade(&v), (20, 0));
        v.0.insert("Audio / Editing / Crossfade Time:".into(), "120".into());
        v.0.insert("Audio / Editing / Crossfade Curve:".into(), "-40".into());
        assert_eq!(default_crossfade(&v), (120, -40));
    }

    #[test]
    fn marquee_cuts_delete_crop_and_split() {
        let list = vec![midi(0, 3840), midi(3840, 3840)];
        let no_skip = |_: &Region, _: u32| 0usize;
        // delete bar 1 beat 3 .. bar 2 beat 2: two pieces left of the first region's start and
        // the second region from beat 2 on
        let d = cut(&list, 1920, 4800, Cut::Delete, &no_skip);
        let spans: Vec<(u32, u32, usize)> = d.iter().map(|(r, _)| (r.start, r.length, r.notes.len())).collect();
        assert_eq!(spans, vec![(0, 1920, 4), (4800, 2880, 6)]);
        // the first piece keeps its place in the file; the cut-off part of the second is new
        assert!(d[0].0.source.is_some());
        assert!(d[1].0.source.is_some());
        let c = cut(&list, 1920, 4800, Cut::Crop, &no_skip);
        assert_eq!(c.iter().map(|(r, _)| (r.start, r.length)).collect::<Vec<_>>(), vec![(1920, 1920), (3840, 960)]);
        let s = cut(&list, 1920, 4800, Cut::Split, &no_skip);
        assert_eq!(s.len(), 4);
        assert_eq!(s.iter().map(|(r, _)| r.length).sum::<u32>(), 7680);
        assert_eq!(s.iter().filter(|(r, _)| r.source.is_some()).count(), 2);
        assert_eq!(s.iter().map(|(r, _)| r.notes.len()).sum::<usize>(), 16);
    }

    #[test]
    fn cut_audio_moves_the_offset_and_drops_inner_fades() {
        let mut x = RegionAudio { offset: 1000, ..Default::default() };
        x.fades.in_ms = 50;
        x.fades.out_ms = 80;
        let rg = Region { name: "a".into(), start: 0, length: 3840, notes: Vec::new(), audio: Some(lg_core::project::AudioRef { file: "a.wav".into(), seconds: 2.0 }), muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
        // 1 tick = 10 frames here
        let out = cut(&[(rg, x)], 960, 1920, Cut::Split, &|r: &Region, t: u32| ((t - r.start) * 10) as usize);
        assert_eq!(out.iter().map(|(_, x)| x.offset).collect::<Vec<_>>(), vec![1000, 10600, 20200]);
        assert_eq!(out.iter().map(|(_, x)| (x.fades.in_ms, x.fades.out_ms)).collect::<Vec<_>>(), vec![(50, 0), (0, 0), (0, 80)]);
    }

    #[test]
    fn region_gain_names_parse() {
        for (n, d) in [("Region Gain +1 dB", 1.0f32), ("Region Gain -0.1 dB", -0.1)] {
            let v: f32 = n.trim_start_matches("Region Gain ").trim_end_matches(" dB").parse().unwrap();
            assert!((v - d).abs() < 1e-6);
        }
    }
}
