//! Resizing regions in the Tracks area as Logic does: the right edge trims the end and the left
//! edge the start, for every kind of region (audio, MIDI, Session Player, pattern, alias). The
//! contents stay where they are on the timeline: an audio region's file offset moves with its
//! start, so the same audio plays at the same time, and its waveform is drawn at the timeline's
//! own scale from that offset (never squeezed into the region). Audio can't reach before the
//! file's start or past its end. Option-drag on the right edge time-stretches (stretch.rs).
//! Each resize is one undo step, with Position and Length in the help tag while dragging.

use egui::{pos2, Color32, CursorIcon, Rect, Sense};

use crate::app::App;

/// A region's left edge being dragged: (track, region, start, length, file offset) at the press.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Left {
    t: usize,
    r: usize,
    start: u32,
    length: u32,
    offset: usize,
}

thread_local! {
    static UNREADABLE: std::cell::RefCell<std::collections::HashSet<std::path::PathBuf>> = Default::default();
}

fn id() -> egui::Id {
    egui::Id::unique("lg-region-left-trim")
}

/// The left edge trimmed to `to` (ticks): start, length and file offset (frames). `secs` maps a
/// tick to seconds and `tick` back (the tempo map); an audio region (`rate` > 0) can't start
/// before its file does, and every region keeps at least a tick.
pub fn trim_start(start: u32, length: u32, offset: usize, rate: f64, to: f64, secs: &dyn Fn(f64) -> f64, tick: &dyn Fn(f64) -> f64) -> (u32, u32, usize) {
    let end = (start + length) as f64;
    let min = if rate > 0.0 { tick((secs(start as f64) - offset as f64 / rate).max(0.0)).max(0.0) } else { 0.0 };
    let nt = to.clamp(min.min(end - 1.0), end - 1.0).round();
    let off = if rate > 0.0 { (offset as f64 + (secs(nt) - secs(start as f64)) * rate).round().max(0.0) as usize } else { offset };
    (nt as u32, (end - nt) as u32, off)
}

/// The furthest an audio region's end can go: where its file ends (ticks).
pub fn max_end(start: u32, offset: usize, file_frames: usize, rate: f64, secs: &dyn Fn(f64) -> f64, tick: &dyn Fn(f64) -> f64) -> f64 {
    tick(secs(start as f64) + file_frames.saturating_sub(offset) as f64 / rate)
}

/// The region's audio file decoded (for its waveform and the trims), once.
pub fn ensure_loaded(app: &mut App, t: usize, r: usize) -> Option<(f64, usize)> {
    let file = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?.audio.as_ref()?.file.clone();
    // (a file that can't be read isn't tried again every frame)
    if UNREADABLE.with(|u| u.borrow().contains(&file)) {
        return None;
    }
    let Some(l) = crate::audioedit::loaded(app, &file) else {
        UNREADABLE.with(|u| u.borrow_mut().insert(file));
        return None;
    };
    Some((l.audio.rate as f64, l.audio.frames()))
}

/// The right edge's limit for an audio region (its file's end), in ticks; None for others.
pub fn end_limit(app: &mut App, t: usize, r: usize) -> Option<f64> {
    let (rate, frames) = ensure_loaded(app, t, r)?;
    let start = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?.start;
    let offset = crate::audioedit::extras(app, t, r).offset;
    Some(max_end(start, offset, frames, rate, &|k| app.seconds_at(k), &|s| crate::smarttempo::tick_at(app, s)))
}

/// The left edge of region (t, r) at `rr`: drag to trim the start (snapped; Shift: free).
pub fn left_edge(app: &mut App, ui: &egui::Ui, rr: Rect, lanes: Rect, t: usize, r: usize, tick_of: &dyn Fn(f32) -> f32, px_per_tick: f32) {
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let edge = Rect::from_min_max(pos2(rr.min.x - 2.0, rr.center().y), pos2(rr.min.x + 6.0, rr.max.y)).intersect(lanes);
    if !edge.is_positive() {
        return;
    }
    let resp = ui.interact(edge, ui.scope_id().with(("region-start", t, r)), Sense::drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    let ctx = ui.ctx().clone();
    if resp.drag_started() {
        let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)) else { return };
        let (start, length, audio) = (rg.start, rg.length, rg.audio.is_some());
        if audio {
            ensure_loaded(app, t, r);
        }
        let offset = if audio { crate::audioedit::extras(app, t, r).offset } else { 0 };
        crate::audioedit::checkpoint(app, "Resize Region");
        app.selected_region = Some((t, r));
        if !app.selected_regions.contains(&(t, r)) {
            app.selected_regions = vec![(t, r)];
        }
        ctx.data_mut(|d| d.insert_temp(id(), Some(Left { t, r, start, length, offset })));
    }
    let Some(d) = ctx.data(|d| d.get_temp::<Option<Left>>(id())).flatten().filter(|d| d.t == t && d.r == r) else { return };
    if let (true, Some(pp)) = (resp.dragged(), resp.interact_pointer_pos()) {
        let free = ui.input(|i| i.modifiers.shift);
        let raw = tick_of(pp.x).max(0.0);
        let to = if free { raw } else { crate::arranging::snap_tick(app, raw, px_per_tick) };
        let audio = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).is_some_and(|x| x.audio.is_some());
        let rate = if audio { ensure_loaded(app, t, r).map_or(0.0, |x| x.0) } else { 0.0 };
        let (ns, nl, off) = trim_start(d.start, d.length, d.offset, rate, to as f64, &|k| app.seconds_at(k), &|s| crate::smarttempo::tick_at(app, s));
        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            rg.start = ns;
            rg.length = nl;
        }
        if audio {
            let mut x = crate::audioedit::extras(app, t, r);
            if x.offset != off {
                // (a fade-in belongs to the old start)
                x.offset = off;
            }
            crate::audioedit::set_extras(app, t, r, x);
        }
        app.help_tag = Some(vec![("Length Change:".into(), String::new()), ("Position:".into(), crate::app::bbdt(ns, sig)), ("Length:".into(), crate::app::bbdt_len(nl, sig))]);
    }
    if resp.drag_stopped() {
        ctx.data_mut(|d| d.insert_temp::<Option<Left>>(id(), None));
        let (mode, mpt) = (app.arr.drag, crate::arranging::ms_per_tick(app));
        let len = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map_or(d.length, |x| x.length);
        let k = match app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
            Some(tr) => lg_core::regions::after_resize(&mut tr.regions, r, false, len as i64 - d.length as i64, mode, mpt),
            None => r,
        };
        if k != r {
            // (the region moved in its track's order: its audio extras go with it)
            if let Some(x) = app.audio_edit.regions.remove(&(t, r)) {
                app.audio_edit.regions.insert((t, k), x);
            }
        }
        if app.selected_region == Some((t, r)) {
            app.selected_region = Some((t, k));
            app.selected_regions = vec![(t, k)];
        }
        app.after_edit();
    }
}

/// An audio region's waveform from its file, at the timeline's scale: the file offset at the
/// region's start, flex, gain and fades as played. False when it can't be drawn this way (a
/// reversed region, or the file not decoded yet): the caller draws the engine's render.
pub fn draw_wave(app: &App, p: &egui::Painter, r: Rect, inner: Rect, clip: Rect, t: usize, ri: usize, ink: Color32) -> bool {
    let Some(rg) = app.project.as_ref().and_then(|pp| pp.tracks.get(t)?.regions.get(ri)) else { return false };
    let Some(a) = &rg.audio else { return false };
    let Some(l) = crate::audioedit::loaded_ref(app, &a.file) else { return false };
    let x = crate::audioedit::extras(app, t, ri);
    if x.reverse || r.width() < 1.0 || rg.length == 0 {
        return false;
    }
    let rate = l.audio.rate as f64;
    let start_sec = app.seconds_at(rg.start as f64);
    let region_secs = app.seconds_at((rg.start + rg.length) as f64) - start_sec;
    let tick_of = |px: f32| rg.start as f64 + ((px - r.min.x) / r.width()) as f64 * rg.length as f64;
    let g = 10f32.powf(x.gain_db / 20.0);
    let zr = Rect::from_center_size(inner.center(), egui::vec2(inner.width(), inner.height() * app.keyx.wave_zoom));
    let (mid, half) = (zr.center().y, zr.height() * 0.5);
    let vis = inner.intersect(clip);
    if !vis.is_positive() {
        return true;
    }
    let frames = l.audio.frames() as f64;
    let mut mesh = egui::Mesh::default();
    let mut k = 0u32;
    let mut px = vis.min.x.floor();
    while px < vis.max.x {
        let s0 = app.seconds_at(tick_of(px)) - start_sec;
        let s1 = app.seconds_at(tick_of(px + 1.0)) - start_sec;
        let f0 = x.offset as f64 + crate::audioedit::flex_src(&x.flex, s0.max(0.0) * rate);
        let f1 = x.offset as f64 + crate::audioedit::flex_src(&x.flex, s1.min(region_secs) * rate);
        // (past the file's end: silence)
        let (mn, mx) = if f0 >= frames { (0.0, 0.0) } else { l.overview.range(&l.audio.data, None, f0, f1.max(f0 + 1.0)) };
        let fg = x.fades.gain_at(s0.max(0.0), region_secs) * g;
        let top = mid - (mx * fg).clamp(-1.0, 1.0) * half;
        let bot = (mid - (mn * fg).clamp(-1.0, 1.0) * half).max(top + 0.6);
        mesh.colored_vertex(pos2(px, top), ink);
        mesh.colored_vertex(pos2(px, bot), ink);
        if k > 0 {
            let v = k * 2;
            mesh.add_triangle(v - 2, v - 1, v);
            mesh.add_triangle(v - 1, v + 1, v);
        }
        k += 1;
        px += 1.0;
    }
    p.with_clip_rect(vis).add(egui::Shape::mesh(mesh));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // 120 bpm, 960 ticks a beat: 1920 ticks a second
    fn secs(k: f64) -> f64 {
        k / 1920.0
    }
    fn tick(s: f64) -> f64 {
        s * 1920.0
    }

    #[test]
    fn the_left_edge_keeps_the_audio_where_it_is() {
        // a region at bar 2 (7680), two seconds long, from the file's start, at 48 kHz
        let (s, l, off) = trim_start(7680, 3840, 0, 48000.0, 7680.0 + 1920.0, &secs, &tick);
        assert_eq!((s, l, off), (9600, 1920, 48000));
        // dragged back out: the audio comes back, never before the file's start
        let (s, l, off) = trim_start(9600, 1920, 48000, 48000.0, 0.0, &secs, &tick);
        assert_eq!((s, l, off), (7680, 3840, 0));
    }

    #[test]
    fn the_left_edge_keeps_a_tick() {
        let (s, l, _) = trim_start(0, 960, 0, 0.0, 5000.0, &secs, &tick);
        assert_eq!((s, l), (959, 1));
        // MIDI: anywhere back to the project's start, no offset
        let (s, l, off) = trim_start(3840, 960, 0, 0.0, 0.0, &secs, &tick);
        assert_eq!((s, l, off), (0, 4800, 0));
    }

    #[test]
    fn the_right_edge_stops_at_the_files_end() {
        // three seconds of file, half a second skipped: two and a half seconds from the start
        let e = max_end(1920, 24000, 144000, 48000.0, &secs, &tick);
        assert_eq!(e.round() as u32, 1920 + 4800);
    }
}
