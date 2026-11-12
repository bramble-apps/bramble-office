//! Tempo points and signatures on the clipboard (Copy / Paste on the Tempo and Signature
//! tracks: pasted at the playhead, the first copied item landing there, the rest at their
//! distances), Select Inside Locators for tempo points, and Set Time For Last Selected Tempo
//! (a video hit point: the selected points before the last get a tempo scaled so the last one
//! falls at the given time).

use crate::project::Project;
use crate::timeline::{self, Map};

/// Copied tempo points: (ticks after the first, BPM).
pub fn copy_points(p: &Project, sel: &[u32]) -> Vec<(u32, f64)> {
    let mut v: Vec<(u32, f64)> = p.tempo_map.iter().filter(|x| sel.contains(&x.0)).copied().collect();
    v.sort_by_key(|x| x.0);
    let first = v.first().map_or(0, |x| x.0);
    v.into_iter().map(|(t, b)| (t - first, b)).collect()
}

/// Pastes tempo points at `at`; returns their ticks (to select).
pub fn paste_points(p: &mut Project, clip: &[(u32, f64)], at: u32) -> Vec<u32> {
    clip.iter()
        .map(|&(d, b)| {
            timeline::set_point(p, at + d, b);
            at + d
        })
        .collect()
}

/// The tempo points in [a, b).
pub fn points_inside(p: &Project, a: u32, b: u32) -> Vec<u32> {
    p.tempo_map.iter().map(|x| x.0).filter(|&t| t >= a && t < b).collect()
}

/// Set Time For Last Selected Tempo: the points in `sel` before the last get their tempo
/// scaled so the last selected point falls at `secs` (seconds from bar 1). False when it
/// can't (fewer than two points, or the time is before the first one).
pub fn set_time_for_last(p: &mut Project, sel: &[u32], secs: f64) -> bool {
    let mut s: Vec<u32> = sel.iter().copied().filter(|t| p.tempo_map.iter().any(|x| x.0 == *t)).collect();
    s.sort();
    if s.len() < 2 {
        return false;
    }
    let (first, last) = (s[0], *s.last().unwrap());
    let m = Map::of(p);
    let (t0, t1) = (m.secs_at(first as f64), m.secs_at(last as f64));
    let want = secs - t0;
    if want <= 0.01 || t1 - t0 <= 0.0 {
        return false;
    }
    let k = (t1 - t0) / want;
    for x in p.tempo_map.iter_mut() {
        if x.0 >= first && x.0 < last && s.contains(&x.0) {
            x.1 = (x.1 * k).clamp(5.0, 990.0);
        }
    }
    // a point in the span that isn't selected keeps its own tempo; the selected ones carry it
    true
}

/// Copied signatures: (bars after the first, numerator, denominator).
pub fn copy_signatures(p: &Project, bars: &[u32]) -> Vec<(u32, u8, u8)> {
    let mut v: Vec<(u32, u8, u8)> = p.global.signatures.iter().filter(|s| bars.contains(&s.0)).copied().collect();
    v.sort_by_key(|s| s.0);
    let first = v.first().map_or(0, |s| s.0);
    v.into_iter().map(|(b, n, d)| (b - first, n, d)).collect()
}

/// Pastes signatures from bar `at` (bar 0 is the project's own: at least bar 1).
pub fn paste_signatures(p: &mut Project, clip: &[(u32, u8, u8)], at: u32) -> Vec<u32> {
    let at = at.max(1);
    clip.iter()
        .map(|&(d, n, den)| {
            timeline::set_signature(p, at + d, (n, den));
            at + d
        })
        .collect()
}

/// The Scissors on the Signature track: the bar at `bar` holding the signature (n/d) is cut
/// after `beats` beats: n/d becomes beats/d at that bar, then (n-beats)/d for one bar, then
/// the original resumes (a cut inside a run of n/d bars inserts the two short ones).
pub fn split_signature(p: &mut Project, bar: u32, beats: u8) -> bool {
    let (n, d) = Map::of(p).sig_of_bar(bar);
    if beats == 0 || beats >= n {
        return false;
    }
    let had_next = p.global.signatures.iter().any(|s| s.0 == bar + 1);
    let next = p.global.signatures.iter().find(|s| s.0 == bar + 1).map(|s| (s.1, s.2));
    timeline::set_signature(p, bar, (beats, d));
    timeline::set_signature(p, bar + 1, (n - beats, d));
    if !had_next {
        timeline::set_signature(p, bar + 2, (n, d));
    } else if let Some(sig) = next {
        timeline::set_signature(p, bar + 2, sig);
    }
    true
}

/// The Join tool: signatures `bars` (selected) merged into the first: its bar gets the
/// summed numerator (same denominator), the others removed.
pub fn join_signatures(p: &mut Project, bars: &[u32]) -> bool {
    let mut b: Vec<u32> = bars.to_vec();
    b.sort();
    b.dedup();
    if b.len() < 2 {
        return false;
    }
    let m = Map::of(p);
    let sigs: Vec<(u8, u8)> = b.iter().map(|&x| m.sig_of_bar(x)).collect();
    if sigs.iter().any(|s| s.1 != sigs[0].1) {
        return false;
    }
    let total: u32 = sigs.iter().map(|s| s.0 as u32).sum();
    if total > 99 {
        return false;
    }
    let after = m.sig_of_bar(b[b.len() - 1] + 1);
    for &x in &b[1..] {
        timeline::delete_signature(p, x);
    }
    timeline::set_signature(p, b[0], (total as u8, sigs[0].1));
    // what followed the last joined bar carries on
    if !p.global.signatures.iter().any(|s| s.0 == b[0] + 1) {
        timeline::set_signature(p, b[0] + 1, after);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        let mut p = Project { tempo: 120.0, tempo_map: vec![(0, 120.0)], signature: (4, 4), ..Default::default() };
        timeline::set_point(&mut p, 3840, 100.0);
        timeline::set_point(&mut p, 7680, 140.0);
        p
    }

    #[test]
    fn copy_paste_and_inside() {
        let mut p = project();
        let clip = copy_points(&p, &[3840, 7680]);
        assert_eq!(clip, vec![(0, 100.0), (3840, 140.0)]);
        let got = paste_points(&mut p, &clip, 15360);
        assert_eq!(got, vec![15360, 19200]);
        assert!(p.tempo_map.contains(&(19200, 140.0)));
        assert_eq!(points_inside(&p, 1, 15360), vec![3840, 7680]);
    }

    #[test]
    fn set_time_moves_the_last_point() {
        let mut p = project();
        let m = Map::of(&p);
        let t1 = m.secs_at(7680.0);
        assert!(set_time_for_last(&mut p, &[3840, 7680], t1 + 0.4));
        let m2 = Map::of(&p);
        assert!((m2.secs_at(7680.0) - (t1 + 0.4)).abs() < 1e-6);
        // the point before the first selected one is unchanged
        assert!((m2.secs_at(3840.0) - m.secs_at(3840.0)).abs() < 1e-9);
        assert!(!set_time_for_last(&mut p, &[3840], 10.0));
    }

    #[test]
    fn signatures_copy_split_join() {
        let mut p = project();
        timeline::set_signature(&mut p, 2, (3, 4));
        let clip = copy_signatures(&p, &[2]);
        assert_eq!(clip, vec![(0, 3, 4)]);
        paste_signatures(&mut p, &clip, 6);
        assert!(p.global.signatures.iter().any(|s| *s == (6, 3, 4)));
        // 5/4 cut after 3 beats: 3/4, 2/4, then 5/4 again
        let mut q = project();
        timeline::set_signature(&mut q, 4, (5, 4));
        assert!(split_signature(&mut q, 4, 3));
        let m = Map::of(&q);
        assert_eq!((m.sig_of_bar(4), m.sig_of_bar(5), m.sig_of_bar(6)), ((3, 4), (2, 4), (5, 4)));
        // joining them back: 5/4
        assert!(join_signatures(&mut q, &[4, 5]));
        let m = Map::of(&q);
        assert_eq!((m.sig_of_bar(4), m.sig_of_bar(5)), ((5, 4), (5, 4)));
    }
}
