//! Track automation lanes (Logic's "*Automation" sequences): the line's value with curved
//! segments, the edits the Tracks area makes (segment moves, marquee ranges, points at region
//! borders), and the event list written back into Logic's file.
//!
//! Records (research DIFFS E08): 16-byte units, kind 0x5n, +4 position (the event origin, as
//! notes), +11 value (0-127, Logic's fader scale: 90 = 0 dB), +12 parameter (7 volume, 10 pan),
//! +15 flags (0x80 selected).

use crate::events::{self, BAR_ONE};
use crate::project::AutoLane;

/// A curved segment's shape at `t` (0..1): `curve` -99..99, 0 straight; positive starts slow
/// and ends fast (as the fades' curve), negative the other way.
pub fn shape(t: f32, curve: i8) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let c = curve.clamp(-99, 99) as f32 / 99.0;
    let e = 1.0 + 4.0 * c.abs();
    if c >= 0.0 { t.powf(e) } else { 1.0 - (1.0 - t).powf(e) }
}

/// The curve of the segment that starts at `tick`.
pub fn curve_of(lane: &AutoLane, tick: u32) -> i8 {
    lane.curves.iter().find(|c| c.0 == tick).map_or(0, |c| c.1)
}

/// The lane's value at `tick` (0-127): straight or curved between points, the first point's
/// value before it, the last's after it.
pub fn value_at(lane: &AutoLane, tick: f64) -> Option<f32> {
    let pts = &lane.points;
    let first = pts.first()?;
    if tick <= first.0 as f64 {
        return Some(first.1 as f32);
    }
    for w in pts.windows(2) {
        let ((a, va), (b, vb)) = (w[0], w[1]);
        if tick >= a as f64 && tick <= b as f64 {
            if lane.steps.contains(&a) && tick < b as f64 {
                return Some(va as f32);
            }
            let t = if b > a { ((tick - a as f64) / (b - a) as f64) as f32 } else { 1.0 };
            let s = shape(t, curve_of(lane, a));
            return Some(va as f32 + (vb as f32 - va as f32) * s);
        }
    }
    pts.last().map(|p| p.1 as f32)
}

/// Keeps the points in time order and the curves on segments that still exist.
pub fn tidy(lane: &mut AutoLane) {
    lane.points.sort_by_key(|p| p.0);
    lane.points.dedup_by_key(|p| p.0);
    let starts: Vec<u32> = lane.points.iter().rev().skip(1).map(|p| p.0).collect();
    lane.curves.retain(|c| c.1 != 0 && starts.contains(&c.0));
    lane.steps.retain(|t| starts.contains(t));
    lane.steps.sort_unstable();
    lane.steps.dedup();
}

/// Moves the segment that starts at point `k` (both its points) up or down by `dv`.
pub fn move_segment(lane: &mut AutoLane, k: usize, dv: f32) {
    for i in [k, k + 1] {
        if let Some(p) = lane.points.get_mut(i) {
            p.1 = (p.1 as f32 + dv).round().clamp(0.0, 127.0) as u8;
        }
    }
}

/// Bends the segment that starts at point `k`.
pub fn set_curve(lane: &mut AutoLane, k: usize, curve: i8) {
    let Some(&(tick, _)) = lane.points.get(k) else { return };
    lane.curves.retain(|c| c.0 != tick);
    if curve != 0 {
        lane.curves.push((tick, curve.clamp(-99, 99)));
    }
}

/// The points inside `from..=to`.
pub fn points_in(lane: &AutoLane, from: u32, to: u32) -> Vec<usize> {
    lane.points.iter().enumerate().filter(|(_, p)| p.0 >= from && p.0 <= to).map(|(i, _)| i).collect()
}

/// A point at `tick` on the line (the value there), unless there is one.
pub fn add_on_line(lane: &mut AutoLane, tick: u32, fallback: f32) {
    if lane.points.iter().any(|p| p.0 == tick) {
        return;
    }
    let v = value_at(lane, tick as f64).unwrap_or(fallback);
    lane.points.push((tick, v.round().clamp(0.0, 127.0) as u8));
    tidy(lane);
}

/// Deletes the points inside a marquee range.
pub fn delete_range(lane: &mut AutoLane, from: u32, to: u32) {
    lane.points.retain(|p| p.0 < from || p.0 > to);
    tidy(lane);
}

/// A marquee range moved up or down (Logic: drag inside the selection): points at its borders
/// keep the line outside it, everything inside moves by `dv`.
pub fn shift_range(lane: &mut AutoLane, from: u32, to: u32, dv: f32, fallback: f32) {
    if to <= from {
        return;
    }
    let outside_l = from.saturating_sub(1);
    let outside_r = to.saturating_add(1);
    for t in [outside_l, from, to, outside_r] {
        add_on_line(lane, t, fallback);
    }
    for p in &mut lane.points {
        if p.0 >= from && p.0 <= to {
            p.1 = (p.1 as f32 + dv).round().clamp(0.0, 127.0) as u8;
        }
    }
    tidy(lane);
}

/// Create 1 (or 2) Track Automation Points at Region Borders: one point on the line at each
/// border; two: one just outside and one just inside (so the region's part can move alone).
pub fn points_at_borders(lane: &mut AutoLane, start: u32, end: u32, two: bool, fallback: f32) {
    let ticks: Vec<u32> = if two { vec![start.saturating_sub(1), start, end.saturating_sub(1), end] } else { vec![start, end] };
    for t in ticks {
        add_on_line(lane, t, fallback);
    }
}

/// Delete Redundant Automation Points: points that sit on the straight line between their
/// neighbours.
pub fn delete_redundant(lane: &mut AutoLane) {
    let pts = lane.points.clone();
    if pts.len() < 3 {
        return;
    }
    // (each point against the last one kept and the next)
    let mut kept = vec![pts[0]];
    for k in 1..pts.len() - 1 {
        let (a, b, c) = (*kept.last().unwrap(), pts[k], pts[k + 1]);
        let straight = curve_of(lane, a.0) == 0 && curve_of(lane, b.0) == 0 && c.0 > a.0;
        let on = a.1 as f32 + (c.1 as f32 - a.1 as f32) * (b.0 - a.0) as f32 / (c.0 - a.0).max(1) as f32;
        if !(straight && (on - b.1 as f32).abs() < 0.5) {
            kept.push(b);
        }
    }
    kept.push(pts[pts.len() - 1]);
    lane.points = kept;
    tidy(lane);
}

/// The lanes as one comparable list (param -> points), plug-in lanes left out.
fn channel_lanes(lanes: &[AutoLane]) -> Vec<(u8, Vec<(u32, u8)>)> {
    let mut v: Vec<(u8, Vec<(u32, u8)>)> = lanes.iter().filter(|l| l.plugin.is_none() && !l.relative && !l.points.is_empty()).map(|l| (l.param, l.points.clone())).collect();
    v.sort_by_key(|x| x.0);
    v
}

/// The lanes differ (as Logic stores them: channel parameters' points).
pub fn changed(before: &[AutoLane], now: &[AutoLane]) -> bool {
    channel_lanes(before) != channel_lanes(now)
}

/// An automation sequence's event list with `lanes` in place of its points: records that
/// aren't automation kept, each point written into a copy of one of Logic's own records for
/// that parameter (or a plain one-unit record), in time order, the end marker last.
pub fn write_lanes(payload: &[u8], lanes: &[AutoLane]) -> Vec<u8> {
    let records = events::records(payload);
    let end: Vec<u8> = payload.chunks_exact(16).rev().find(|u| u[0] == 0xF1).map(|u| u.to_vec()).unwrap_or_else(|| {
        let mut u = vec![0u8; 16];
        u[0] = 0xF1;
        u[4..8].copy_from_slice(&events::END.to_le_bytes());
        u
    });
    let mut out: Vec<(u32, u8, Vec<u8>)> = records.iter().filter(|r| events::automation(r).is_none()).map(|r| (r.pos, 0, r.bytes.clone())).collect();
    let any = records.iter().find(|r| events::automation(r).is_some());
    for (param, pts) in channel_lanes(lanes) {
        let own: Vec<&events::Record> = records.iter().filter(|r| events::automation(r).is_some_and(|a| a.0 == param)).collect();
        for (i, (tick, value)) in pts.iter().enumerate() {
            let mut b = match own.get(i).or(own.first()).copied().or(any) {
                Some(t) => t.bytes.clone(),
                None => {
                    let mut b = vec![0u8; 16];
                    b[0] = 0x50;
                    b
                }
            };
            let pos = (tick + BAR_ONE).to_le_bytes();
            b[4..7].copy_from_slice(&pos[..3]);
            b[7] = pos[3] & 0x7F;
            b[11] = *value;
            b[12] = param;
            // (not selected)
            b[15] &= 0x7F;
            out.push((tick + BAR_ONE, param, b));
        }
    }
    out.sort_by_key(|x| (x.0, x.1));
    let mut bytes: Vec<u8> = out.into_iter().flat_map(|x| x.2).collect();
    bytes.extend_from_slice(&end);
    bytes
}

/// A tick on the automation snap grid (`grid` ticks, 0: off), moved by the snap offset.
pub fn snap(tick: u32, grid: u32, offset: i32) -> u32 {
    if grid == 0 {
        return tick;
    }
    let g = grid as i64;
    let at = ((tick as i64 - offset as i64) as f64 / g as f64).round() as i64 * g + offset as i64;
    at.max(0) as u32
}

/// Toggles the segment that starts at point `k` between continuous and stepped.
pub fn toggle_step(lane: &mut AutoLane, k: usize) {
    let Some(&(tick, _)) = lane.points.get(k) else { return };
    if lane.points.len() < k + 2 {
        return;
    }
    if let Some(i) = lane.steps.iter().position(|t| *t == tick) {
        lane.steps.remove(i);
    } else {
        lane.steps.push(tick);
        lane.steps.sort_unstable();
    }
}

/// Trim: the whole curve (or the points `only`) up or down by `dv`, keeping the differences
/// between points. Stops where a point reaches 0 or 127 unless `force` (Option), which clips.
/// Returns the offset applied.
pub fn trim(lane: &mut AutoLane, only: Option<&[usize]>, dv: f32, force: bool) -> f32 {
    let picked: Vec<usize> = match only {
        Some(w) => w.to_vec(),
        None => (0..lane.points.len()).collect(),
    };
    if picked.is_empty() {
        return 0.0;
    }
    let mut d = dv.round();
    if !force {
        let hi = picked.iter().filter_map(|&k| lane.points.get(k)).map(|p| p.1).max().unwrap_or(0) as f32;
        let lo = picked.iter().filter_map(|&k| lane.points.get(k)).map(|p| p.1).min().unwrap_or(0) as f32;
        d = d.clamp(-lo, 127.0 - hi);
    }
    for &k in &picked {
        if let Some(p) = lane.points.get_mut(k) {
            p.1 = (p.1 as f32 + d).clamp(0.0, 127.0) as u8;
        }
    }
    d
}

/// The value an absolute lane and its relative curve play together at `tick`.
pub fn combined_at(abs: Option<&AutoLane>, rel: Option<&AutoLane>, tick: f64, fallback: f32) -> f32 {
    let a = abs.and_then(|l| value_at(l, tick)).unwrap_or(fallback);
    let r = rel.and_then(|l| value_at(l, tick)).map_or(0.0, |v| v - 64.0);
    (a + r).clamp(0.0, 127.0)
}

/// The absolute and relative curves as one absolute lane (Consolidate Relative and Absolute):
/// a point wherever either has one, valued as they play together. Playback is unchanged.
pub fn consolidate(abs: Option<&AutoLane>, rel: &AutoLane, fallback: f32) -> AutoLane {
    let mut ticks: Vec<u32> = rel.points.iter().map(|p| p.0).chain(abs.into_iter().flat_map(|l| l.points.iter().map(|p| p.0))).collect();
    // (curved segments sampled so the sum keeps their shape)
    for l in abs.into_iter().chain([rel]) {
        for w in l.points.windows(2) {
            if curve_of(l, w[0].0) != 0 && w[1].0 > w[0].0 + 16 {
                ticks.extend((1..16).map(|s| w[0].0 + (w[1].0 - w[0].0) * s / 16));
            }
            if l.steps.contains(&w[0].0) && w[1].0 > 0 {
                ticks.push(w[1].0 - 1);
            }
        }
    }
    ticks.sort_unstable();
    ticks.dedup();
    let mut out = abs.cloned().unwrap_or(AutoLane { param: rel.param, plugin: rel.plugin, ..Default::default() });
    out.curves.clear();
    out.steps.clear();
    out.relative = false;
    out.points = ticks.iter().map(|&t| (t, combined_at(abs, Some(rel), t as f64, fallback).round() as u8)).collect();
    out
}

/// The points of `from..=to` as a clipboard: ticks from `from`, with the line's values at both
/// ends.
pub fn copy_range(lane: &AutoLane, from: u32, to: u32) -> Vec<(u32, u8)> {
    let mut l = lane.clone();
    if !l.points.is_empty() {
        add_on_line(&mut l, from, 0.0);
        add_on_line(&mut l, to, 0.0);
    }
    l.points.iter().filter(|p| p.0 >= from && p.0 <= to).map(|p| (p.0 - from, p.1)).collect()
}

/// Pasted (or moved) automation at `at`: the points already in its span go first.
pub fn paste(lane: &mut AutoLane, at: u32, pts: &[(u32, u8)]) {
    let Some(len) = pts.iter().map(|p| p.0).max() else { return };
    lane.points.retain(|p| p.0 < at || p.0 > at + len);
    lane.points.extend(pts.iter().map(|p| (p.0 + at, p.1)));
    tidy(lane);
}

/// A lane's part in `start..=end` as region automation (ticks from `start`, the line's value
/// at both borders), and the track lane without it (Convert Track Automation to Region).
pub fn take_span(lane: &mut AutoLane, start: u32, end: u32) -> AutoLane {
    let mut part = AutoLane { param: lane.param, plugin: lane.plugin, ..Default::default() };
    if lane.points.is_empty() {
        return part;
    }
    part.points = copy_range(lane, start, end);
    part.curves = lane.curves.iter().filter(|c| c.0 >= start && c.0 < end).map(|c| (c.0 - start, c.1)).collect();
    part.steps = lane.steps.iter().filter(|t| **t >= start && **t < end).map(|t| t - start).collect();
    lane.points.retain(|p| p.0 < start || p.0 > end);
    tidy(lane);
    tidy(&mut part);
    part
}

/// Region automation written back onto the track lane at the region's `start` (Convert Region
/// Automation to Track): it replaces the track's points in that span.
pub fn put_span(lane: &mut AutoLane, part: &AutoLane, start: u32) {
    if part.points.is_empty() {
        return;
    }
    paste(lane, start, &part.points);
    lane.curves.extend(part.curves.iter().map(|c| (c.0 + start, c.1)));
    lane.steps.extend(part.steps.iter().map(|t| t + start));
    tidy(lane);
}

/// What plays: the track's lane with each region's own automation for the parameter over its
/// span (`spans`: region start, end, its lane in region ticks).
pub fn merge_regions(track: Option<&AutoLane>, spans: &[(u32, u32, &AutoLane)]) -> Option<AutoLane> {
    let used: Vec<&(u32, u32, &AutoLane)> = spans.iter().filter(|s| !s.2.points.is_empty() && !s.2.muted).collect();
    if used.is_empty() {
        return track.cloned();
    }
    let mut out = track.cloned().unwrap_or(AutoLane { param: used[0].2.param, plugin: used[0].2.plugin, ..Default::default() });
    for (start, end, part) in used {
        let before = value_at(&out, start.saturating_sub(1) as f64);
        let after = value_at(&out, *end as f64 + 1.0);
        let len = part.points.iter().map(|p| p.0).max().unwrap_or(0).min(end - start);
        let mut clipped = (*part).clone();
        clipped.points.retain(|p| p.0 <= len);
        out.points.retain(|p| p.0 < *start || p.0 > *end);
        if let Some(v) = before {
            out.points.push((start.saturating_sub(1), v.round() as u8));
        }
        if let Some(v) = after {
            out.points.push((end + 1, v.round() as u8));
        }
        put_span(&mut out, &clipped, *start);
    }
    tidy(&mut out);
    Some(out)
}

/// A region's automation split `at` ticks from its start: both pieces get a point at the
/// split (the line's value there), the second piece's ticks from its own start.
pub fn split_lane(lane: &mut AutoLane, at: u32) -> AutoLane {
    let mut whole = lane.clone();
    let mut b = AutoLane { param: lane.param, plugin: lane.plugin, muted: lane.muted, ..Default::default() };
    if whole.points.is_empty() {
        return b;
    }
    add_on_line(&mut whole, at, 0.0);
    lane.points = whole.points.iter().copied().filter(|p| p.0 <= at).collect();
    b.points = whole.points.iter().filter(|p| p.0 >= at).map(|p| (p.0 - at, p.1)).collect();
    b.curves = whole.curves.iter().filter(|c| c.0 >= at).map(|c| (c.0 - at, c.1)).collect();
    b.steps = whole.steps.iter().filter(|t| **t >= at).map(|t| t - at).collect();
    tidy(lane);
    tidy(&mut b);
    b
}

/// Move Track Automation with Regions: the automation over a region's span `from..=to` goes
/// with it to `to_start`; the moved part replaces what was there, and after its end the line
/// ramps back (over `ramp` ticks) to the value that was there.
pub fn move_span(lane: &mut AutoLane, from: u32, to: u32, to_start: u32, ramp: u32) {
    if lane.points.is_empty() || to <= from {
        return;
    }
    let part = take_span(lane, from, to);
    let len = to - from;
    let end = to_start + len;
    let back = value_at(lane, (end + ramp) as f64);
    let before = value_at(lane, to_start.saturating_sub(1) as f64);
    lane.points.retain(|p| p.0 < to_start || p.0 > end + ramp);
    if let Some(v) = before.filter(|_| to_start > 0) {
        lane.points.push((to_start - 1, v.round() as u8));
    }
    if let Some(v) = back {
        lane.points.push((end + ramp.max(1), v.round() as u8));
    }
    put_span(lane, &part, to_start);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane(points: &[(u32, u8)]) -> AutoLane {
        AutoLane { param: 7, points: points.to_vec(), ..Default::default() }
    }

    #[test]
    fn curves_bend_segments_and_keep_their_ends() {
        let mut l = lane(&[(0, 0), (1000, 100)]);
        assert_eq!(value_at(&l, 500.0), Some(50.0));
        set_curve(&mut l, 0, 60);
        let v = value_at(&l, 500.0).unwrap();
        assert!(v < 20.0, "a positive curve starts slow: {v}");
        assert_eq!(value_at(&l, 0.0), Some(0.0));
        assert_eq!(value_at(&l, 1000.0), Some(100.0));
        set_curve(&mut l, 0, -60);
        assert!(value_at(&l, 500.0).unwrap() > 80.0);
        // the curve goes with its segment
        l.points.remove(1);
        tidy(&mut l);
        assert!(l.curves.is_empty());
    }

    #[test]
    fn segments_and_ranges_move() {
        let mut l = lane(&[(0, 90), (960, 90), (1920, 90)]);
        move_segment(&mut l, 0, -20.0);
        assert_eq!(l.points, vec![(0, 70), (960, 70), (1920, 90)]);
        // a marquee range inside a flat line: points at its borders, the inside lowered
        let mut l = lane(&[(0, 90), (3840, 90)]);
        shift_range(&mut l, 960, 1920, -30.0, 90.0);
        assert_eq!(l.points, vec![(0, 90), (959, 90), (960, 60), (1920, 60), (1921, 90), (3840, 90)]);
        delete_range(&mut l, 950, 1930);
        assert_eq!(l.points, vec![(0, 90), (3840, 90)]);
    }

    #[test]
    fn border_points_and_redundant_ones() {
        let mut l = lane(&[(0, 40), (2000, 80)]);
        points_at_borders(&mut l, 500, 1500, true, 90.0);
        assert_eq!(l.points.len(), 6);
        assert!(l.points.iter().any(|p| p.0 == 499) && l.points.iter().any(|p| p.0 == 1500));
        delete_redundant(&mut l);
        assert_eq!(l.points, vec![(0, 40), (2000, 80)]);
        let mut empty = lane(&[]);
        points_at_borders(&mut empty, 100, 200, false, 90.0);
        assert_eq!(empty.points, vec![(100, 90), (200, 90)]);
    }

    #[test]
    fn steps_trim_and_relative() {
        let mut l = lane(&[(0, 20), (1000, 100), (2000, 60)]);
        toggle_step(&mut l, 0);
        assert_eq!(value_at(&l, 999.0), Some(20.0));
        assert_eq!(value_at(&l, 1000.0), Some(100.0));
        toggle_step(&mut l, 0);
        assert_eq!(value_at(&l, 500.0), Some(60.0));
        // trim stops at the top unless forced
        assert_eq!(trim(&mut l, None, 50.0, false), 27.0);
        assert_eq!(l.points, vec![(0, 47), (1000, 127), (2000, 87)]);
        trim(&mut l, None, 30.0, true);
        assert_eq!(l.points[0].1, 77);
        assert_eq!(l.points[1].1, 127);
        // relative: 64 is no change; consolidating keeps what plays
        let abs = lane(&[(0, 90), (1000, 90)]);
        let rel = AutoLane { param: 7, relative: true, points: vec![(0, 64), (1000, 44)], ..Default::default() };
        assert_eq!(combined_at(Some(&abs), Some(&rel), 1000.0, 90.0), 70.0);
        let c = consolidate(Some(&abs), &rel, 90.0);
        for t in [0.0, 500.0, 1000.0] {
            assert!((value_at(&c, t).unwrap() - combined_at(Some(&abs), Some(&rel), t, 90.0)).abs() < 1.0);
        }
        assert!(!c.relative);
        assert_eq!(snap(1000, 960, 0), 960);
        assert_eq!(snap(1000, 960, -5), 955);
        assert_eq!(snap(1000, 0, 0), 1000);
    }

    #[test]
    fn region_automation_converts_moves_and_splits() {
        let mut l = lane(&[(0, 90), (4000, 30)]);
        let part = take_span(&mut l, 1000, 2000);
        assert_eq!(part.points.first().map(|p| p.0), Some(0));
        assert_eq!(part.points.last().map(|p| p.0), Some(1000));
        assert!(l.points.iter().all(|p| p.0 < 1000 || p.0 > 2000));
        put_span(&mut l, &part, 1000);
        assert!((value_at(&l, 1500.0).unwrap() - 67.5).abs() < 1.0);
        // region automation plays over the track's in its span
        let reg = lane(&[(0, 10), (500, 10)]);
        let m = merge_regions(Some(&l), &[(1000, 1500, &reg)]).unwrap();
        assert_eq!(value_at(&m, 1200.0), Some(10.0));
        assert!((value_at(&m, 3000.0).unwrap() - value_at(&l, 3000.0).unwrap()).abs() < 0.5);
        // split: a point at the split on both pieces
        let mut a = lane(&[(0, 0), (1000, 100)]);
        let b = split_lane(&mut a, 400);
        assert_eq!(a.points.last(), Some(&(400, 40)));
        assert_eq!(b.points.first(), Some(&(0, 40)));
        assert_eq!(b.points.last(), Some(&(600, 100)));
        // a region's automation moving with it: the moved part replaces what was there
        let mut t = lane(&[(0, 90), (960, 30), (1920, 90), (5000, 90)]);
        move_span(&mut t, 0, 1920, 3000, 100);
        assert_eq!(value_at(&t, 3960.0), Some(30.0));
        assert_eq!(value_at(&t, 1000.0), Some(90.0));
        // copy and paste: the span's points replaced
        let mut d = lane(&[(0, 50), (100, 60), (200, 70), (3000, 50)]);
        let clip = copy_range(&d, 0, 200);
        paste(&mut d, 1000, &clip);
        assert_eq!(value_at(&d, 1100.0), Some(60.0));
    }

    #[test]
    fn written_records_read_back() {
        // Logic's E08 list: two volume points and the end marker
        let mut payload = Vec::new();
        for (pos, flags) in [(38400u32, 0u8), (59440, 0x80)] {
            let mut u = [0u8; 16];
            u[0] = 0x50;
            u[4..8].copy_from_slice(&pos.to_le_bytes());
            u[11] = 0x5A;
            u[12] = 7;
            u[15] = flags;
            payload.extend(u);
        }
        let mut end = [0u8; 16];
        end[0] = 0xF1;
        end[4..8].copy_from_slice(&events::END.to_le_bytes());
        payload.extend(end);
        let read = |p: &[u8]| -> Vec<(u8, u32, u8)> { events::records(p).iter().filter_map(|r| events::automation(r).map(|(pa, v)| (pa, r.pos - BAR_ONE, v))).collect() };
        let lanes = vec![lane(&[(3840, 90), (24880, 40)]), AutoLane { param: 10, points: vec![(7680, 64)], ..Default::default() }];
        let out = write_lanes(&payload, &lanes);
        assert_eq!(read(&out), vec![(7, 3840, 90), (10, 7680, 64), (7, 24880, 40)]);
        assert_eq!(&out[out.len() - 16..], &end[..]);
        // the same points give Logic's bytes back but for the selection flag
        let same = write_lanes(&payload, &[lane(&[(3840, 90), (24880, 90)])]);
        assert_eq!(&same[..31], &payload[..31]);
        assert_eq!(same[31], 0);
        assert!(!changed(&[lane(&[(1, 2)])], &[lane(&[(1, 2)]), AutoLane { param: 3, plugin: Some((3, 1)), points: vec![(5, 5)], ..Default::default() }]));
    }
}
