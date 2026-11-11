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
    let mut v: Vec<(u8, Vec<(u32, u8)>)> = lanes.iter().filter(|l| l.plugin.is_none() && !l.points.is_empty()).map(|l| (l.param, l.points.clone())).collect();
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
