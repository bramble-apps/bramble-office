//! Bramble's Session Player drummer: our own pattern generator, responding to Logic's controls the
//! way the Logic research measured them (session-players grid: Pop Rock at complexity ×
//! intensity 3×3, ghost notes, feel, re-rolls):
//! - Intensity sets the velocities (about 57 soft, 80 medium, 104 loud) and, soft, the snare
//!   becomes a side-stick (37 for 38).
//! - Complexity adds notes (about 11.6 → 13 per bar from simple to complex): kick variations,
//!   snare ghost notes, sixteenth hi-hats; simple grooves lean on tom fills and a crash.
//! - Fills at the ends of phrases, swing on the off sixteenths, ghost notes, and a seed for
//!   Regenerate.
//! The patterns are made here, beat by beat; none are copied from Logic's.
//!
//! The editor (the bottom pane for a drummer region, as Logic's Session Player): the region's
//! hits, the player and style, Regenerate, the Complexity and Intensity sliders, and the Main /
//! Details / Manual tabs (Details: fills, swing, ghost notes; Manual: the Piano Roll).

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::events::{Note, PPQ};

use crate::app::App;
use crate::look;

/// General MIDI drum notes (Logic's Drum Kit Designer kits follow them).
pub const KICK: u8 = 36;
pub const SIDE_STICK: u8 = 37;
pub const SNARE: u8 = 38;
pub const HAT_CLOSED: u8 = 42;
pub const HAT_OPEN: u8 = 46;
pub const CRASH: u8 = 49;
pub const RIDE: u8 = 51;
pub const TOMS: [u8; 4] = [50, 48, 47, 45];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// 0 simple .. 1 complex
    pub complexity: f32,
    /// 0 soft .. 1 loud
    pub intensity: f32,
    /// 0 none .. 1 many
    pub fills: f32,
    /// 0 straight .. 1 heavy swing (of the sixteenths)
    pub swing: f32,
    /// 0 .. 1
    pub ghosts: f32,
    pub seed: u64,
}

impl Default for Params {
    fn default() -> Self {
        Params { complexity: 0.45, intensity: 0.6, fills: 0.5, swing: 0.0, ghosts: 0.5, seed: 1 }
    }
}

/// A small deterministic random source (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn chance(&mut self, p: f32) -> bool {
        self.next() < p
    }
    fn around(&mut self, v: f32, by: f32) -> f32 {
        v + (self.next() * 2.0 - 1.0) * by
    }
}

/// The drummer's performance from `start` (song ticks) for `bars` bars of `beats` beats.
pub fn generate(p: &Params, start: u32, bars: u32, beats: u32) -> Vec<Note> {
    let mut rng = Rng(p.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut out = Vec::new();
    let six = PPQ / 4;
    let bar_t = PPQ * beats;
    // velocity: intensity 0 → ~57, .5 → ~80, 1 → ~104 (the measured grid)
    let base = 57.0 + p.intensity.clamp(0.0, 1.0) * 47.0;
    let snare = if p.intensity < 0.3 { SIDE_STICK } else { SNARE };
    let c = p.complexity.clamp(0.0, 1.0);
    let phrase = if p.fills > 0.66 { 2 } else if p.fills > 0.25 { 4 } else { 8 };
    let mut push = |rng: &mut Rng, pitch: u8, tick: u32, vel: f32, len: u32| {
        // swing: the off sixteenths later
        let off = (tick % (2 * six)) == six;
        let t = tick + if off { (p.swing * six as f32 * 0.5) as u32 } else { 0 };
        let v = rng.around(vel, 6.0).clamp(1.0, 127.0) as u8;
        out.push(Note { channel: 9, pitch, velocity: v, pos: t, len });
    };
    for bar in 0..bars {
        let b0 = start + bar * bar_t;
        let fill_bar = p.fills > 0.05 && (bar + 1) % phrase == 0;
        // how many beats at the bar's end the fill takes
        let fill_beats = if fill_bar { (1.0 + p.fills * 2.0).round() as u32 } else { 0 };
        let groove_beats = beats.saturating_sub(fill_beats);
        for beat in 0..groove_beats {
            let t = b0 + beat * PPQ;
            let strong = beat % 2 == 0;
            // kick: on 1 and 3; complexity adds the "and" of 3, the "a" of 2, pickups
            if strong {
                push(&mut rng, KICK, t, base + 8.0, 240);
            }
            if beat == 2 && rng.chance(0.25 + c * 0.6) {
                push(&mut rng, KICK, t + 2 * six, base - 4.0, 240);
            }
            if beat == 1 && rng.chance(c * 0.45) {
                push(&mut rng, KICK, t + 3 * six, base - 8.0, 240);
            }
            // snare: 2 and 4; ghost notes around it with complexity and the ghost control
            if !strong {
                push(&mut rng, snare, t, base + 4.0, 240);
            }
            for k in 1..4 {
                if rng.chance(c * p.ghosts * 0.32) {
                    push(&mut rng, SNARE, t + k * six, (base * 0.35).max(18.0), 120);
                }
            }
            // hats: eighths, sixteenths when complex; loud and complex: the ride
            let cymbal = if p.intensity > 0.85 && c > 0.6 { RIDE } else { HAT_CLOSED };
            for k in 0..4 {
                let on_eighth = k % 2 == 0;
                if on_eighth || rng.chance((c - 0.45).max(0.0) * 1.2) {
                    let accent = if k == 0 { 6.0 } else if on_eighth { -6.0 } else { -16.0 };
                    push(&mut rng, cymbal, t + k * six, base - 10.0 + accent, 120);
                }
            }
        }
        // an open hat into the next bar now and then
        if !fill_bar && groove_beats == beats && rng.chance(0.08 + c * 0.2) {
            push(&mut rng, HAT_OPEN, b0 + bar_t - 2 * six, base - 6.0, 240);
        }
        // the fill: toms down the kit in sixteenths (simple grooves: in eighths), snare to start
        if fill_beats > 0 {
            let step = if c < 0.35 { 2 * six } else { six };
            let from = b0 + groove_beats * PPQ;
            let n = (fill_beats * PPQ) / step;
            for k in 0..n {
                let tom = if k == 0 { snare } else { TOMS[((k * TOMS.len() as u32) / n.max(1)).min(3) as usize] };
                push(&mut rng, tom, from + k * step, base + 2.0 + (k as f32 / n as f32) * 10.0, 120);
                if k == 0 {
                    push(&mut rng, KICK, from, base, 240);
                }
            }
        }
        // a crash on the bar after a fill
        if bar > 0 && (bar % phrase == 0) && p.fills > 0.05 {
            push(&mut rng, CRASH, b0, base + 10.0, 480);
            push(&mut rng, KICK, b0, base + 8.0, 240);
        }
    }
    out.sort_by_key(|n| (n.pos, n.pitch));
    out.dedup_by(|a, b| a.pos == b.pos && a.pitch == b.pitch);
    out
}

/// The drummer's region: its notes made again from its parameters.
pub fn regenerate(app: &mut App, t: usize, r: usize) {
    let p = app.drum_params.get(&(t, r)).copied().unwrap_or_default();
    let beats = app.project.as_ref().map_or(4, |pr| pr.signature.0 as u32);
    let bar = app.bar_ticks();
    if let Some(rg) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)).and_then(|tr| tr.regions.get_mut(r)) {
        let bars = (rg.length / bar).max(1);
        rg.notes = generate(&p, rg.start, bars, beats);
    }
    app.load_engine();
}

/// The Session Player editor (the bottom pane for a drummer region).
pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect, t: usize, r: usize) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    p.text(pos2(area.center().x, area.min.y + 20.0), Align2::CENTER_CENTER, "Session Player", look::semibold(13.0), look::text());
    let Some(region) = app.project.as_ref().and_then(|pr| pr.tracks.get(t)).and_then(|tr| tr.regions.get(r)).cloned() else { return };
    let track_name = app.project.as_ref().and_then(|pr| pr.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    // the region strip with its hits (kick and snare low, cymbals high)
    let strip = Rect::from_min_max(pos2(area.min.x + 12.0, area.min.y + 44.0), pos2(area.max.x - 12.0, area.min.y + 120.0));
    let yellow = look::region_color(lg_core::TrackKind::Drummer);
    p.rect_filled(strip, 4.0, yellow.gamma_multiply(0.85));
    let head = Rect::from_min_size(strip.min, vec2(strip.width(), 18.0));
    p.rect_filled(head, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, look::mix(yellow, Color32::WHITE, 0.55));
    p.text(pos2(head.min.x + 8.0, head.center().y), Align2::LEFT_CENTER, &region.name, look::regular(11.5), Color32::from_rgb(0x30, 0x24, 0x04));
    let span = region.length.max(1) as f32;
    let ink = look::mix(yellow, Color32::BLACK, 0.6);
    for n in &region.notes {
        let x = strip.min.x + (n.pos.saturating_sub(region.start)) as f32 / span * strip.width();
        let row = match n.pitch {
            KICK => 0.85,
            SNARE | SIDE_STICK => 0.6,
            HAT_CLOSED | HAT_OPEN | RIDE | CRASH => 0.3,
            _ => 0.45,
        };
        let y = head.max.y + row * (strip.max.y - head.max.y);
        let h = 3.0 + n.velocity as f32 / 127.0 * 9.0;
        p.add(egui::Shape::convex_polygon(vec![pos2(x, y - h), pos2(x + 4.0, y), pos2(x, y + h)], ink, Stroke::NONE));
    }
    let mut params = app.drum_params.get(&(t, r)).copied().unwrap_or_default();
    let before = params;
    // the player, Regenerate, the two sliders
    let mid = area.center().x;
    let top = strip.max.y + 22.0;
    p.text(pos2(mid - 110.0, top), Align2::CENTER_CENTER, format!("Acoustic Drummer — {track_name}"), look::regular(13.0), look::text_dim());
    p.text(pos2(mid - 110.0, top + 20.0), Align2::CENTER_CENTER, "Pop Rock", look::regular(14.0), yellow);
    let regen = Rect::from_center_size(pos2(mid - 110.0, top + 80.0), vec2(110.0, 26.0));
    if ui.put(regen, egui::Button::new("Regenerate").corner_radius(6.0)).clicked() {
        app.checkpoint("Regenerate");
        params.seed = params.seed.wrapping_add(0x5851_F42D);
    }
    let tab = app.drum_tab;
    if tab == 0 {
        for (i, (label, v)) in [("Complexity", &mut params.complexity), ("Intensity", &mut params.intensity)].into_iter().enumerate() {
            let x = mid + 30.0 + i as f32 * 110.0;
            p.text(pos2(x, top), Align2::CENTER_CENTER, label, look::regular(13.0), look::text_dim());
            if vertical_slider(app, ui, Rect::from_center_size(pos2(x, top + 62.0), vec2(28.0, 84.0)), ("drum-slider", i), v, yellow) {
                // (each drag is one undo step)
            }
        }
    } else if tab == 1 {
        for (i, (label, v)) in [("Fills", &mut params.fills), ("Swing", &mut params.swing), ("Ghost Notes", &mut params.ghosts)].into_iter().enumerate() {
            let x = mid - 10.0 + i as f32 * 100.0;
            p.text(pos2(x, top), Align2::CENTER_CENTER, label, look::regular(13.0), look::text_dim());
            vertical_slider(app, ui, Rect::from_center_size(pos2(x, top + 62.0), vec2(28.0, 84.0)), ("drum-detail", i), v, yellow);
        }
    }
    // the tabs: Main, Details, Manual (Manual: the Piano Roll)
    let names = ["Main", "Details", "Manual"];
    let tw = 86.0;
    let ty = (top + 136.0).min(area.max.y - 18.0);
    for (i, n) in names.iter().enumerate() {
        let tr = Rect::from_center_size(pos2(mid - tw + i as f32 * tw, ty), vec2(tw - 4.0, 24.0));
        let on = app.drum_tab == i;
        p.rect_filled(tr, 6.0, if on { yellow } else { look::group_bg() });
        p.text(tr.center(), Align2::CENTER_CENTER, *n, look::regular(13.0), if on { Color32::BLACK } else { look::text() });
        if ui.interact(tr, ui.scope_id().with(("drumtab", i)), Sense::click()).clicked() {
            app.drum_tab = i;
        }
    }
    if params != before {
        app.drum_params.insert((t, r), params);
        regenerate(app, t, r);
        app.dirty = true;
    }
}

/// A Logic-style vertical slider (yellow fill, cap); true while it changes.
fn vertical_slider(app: &mut App, ui: &mut egui::Ui, r: Rect, id: (&str, usize), v: &mut f32, color: Color32) -> bool {
    let resp = ui.interact(r.expand(6.0), ui.scope_id().with(id), Sense::click_and_drag());
    let p = ui.painter();
    let track = Rect::from_center_size(r.center(), vec2(4.0, r.height()));
    p.rect_filled(track, 2.0, look::group_bg());
    let y = r.max.y - *v * r.height();
    p.rect_filled(Rect::from_min_max(pos2(track.min.x, y), track.max), 2.0, color);
    p.rect_filled(Rect::from_center_size(pos2(r.center().x, y), vec2(24.0, 10.0)), 3.0, Color32::from_gray(0x9a));
    p.hline(r.center().x - 6.0..=r.center().x + 6.0, y, Stroke::new(1.5, color));
    if resp.drag_started() {
        app.checkpoint("Change Session Player");
    }
    if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
        let nv = ((r.max.y - pp.y) / r.height()).clamp(0.0, 1.0);
        if (nv - *v).abs() > 0.001 {
            *v = nv;
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(p: &Params) -> (f32, f32, usize) {
        let n = generate(p, 0, 8, 4);
        let per_bar = n.len() as f32 / 8.0;
        let vel = n.iter().map(|x| x.velocity as f32).sum::<f32>() / n.len() as f32;
        let side = n.iter().filter(|x| x.pitch == SIDE_STICK).count();
        (per_bar, vel, side)
    }

    #[test]
    fn responds_like_the_research_measured() {
        let soft = stats(&Params { intensity: 0.0, ..Default::default() });
        let loud = stats(&Params { intensity: 1.0, ..Default::default() });
        assert!(soft.1 < 70.0 && loud.1 > 90.0, "velocity follows intensity: {soft:?} {loud:?}");
        assert!(soft.2 > 0 && loud.2 == 0, "soft: side-stick");
        let simple = stats(&Params { complexity: 0.0, seed: 7, ..Default::default() });
        let complex = stats(&Params { complexity: 1.0, seed: 7, ..Default::default() });
        assert!(complex.0 > simple.0, "complexity adds notes: {simple:?} {complex:?}");
        // same settings, same seed: the same performance; another seed: a variation
        assert_eq!(generate(&Params::default(), 0, 4, 4), generate(&Params::default(), 0, 4, 4));
        assert_ne!(generate(&Params::default(), 0, 4, 4), generate(&Params { seed: 99, ..Default::default() }, 0, 4, 4));
    }
}
