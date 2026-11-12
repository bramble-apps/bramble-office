//! Smoother waveforms in the Tracks area (a Bramble extra; users asked Logic 13 for it): each
//! audio region draws from a multi-resolution min / max overview (lg_audio::peaks, built once
//! per decoded file and kept while the file is in use), only for the columns on screen, as one
//! filled shape with its true minimum and maximum. Columns are counted from the region's own
//! left edge, so scrolling doesn't make the outline shimmer; closer than a sample per point the
//! samples themselves are joined as a line.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use egui::{pos2, Color32, Rect, Stroke};
use lg_audio::peaks::Overview;

thread_local! {
    static CACHE: RefCell<HashMap<(usize, usize), (Arc<Vec<f32>>, Arc<Overview>)>> = RefCell::new(HashMap::new());
}

/// The overview of `s` (stereo, interleaved), built on first use.
pub fn overview(s: &Arc<Vec<f32>>) -> Arc<Overview> {
    let key = (Arc::as_ptr(s) as usize, s.len());
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((_, o)) = c.get(&key) {
            return o.clone();
        }
        // forget the files no one holds any more
        c.retain(|_, (a, _)| Arc::strong_count(a) > 1);
        let o = Arc::new(Overview::new(s, 2));
        c.insert(key, (s.clone(), o.clone()));
        o
    })
}

/// Columns `k0..k1` (points from the waveform's left edge), `fpp` frames each: (min, max).
pub fn columns(o: &Overview, data: &[f32], fpp: f64, k0: usize, k1: usize) -> Vec<(f32, f32)> {
    (k0..k1).map(|k| {
        let a = k as f64 * fpp;
        if a >= o.frames as f64 {
            return (0.0, 0.0);
        }
        o.range(data, None, a, (a + fpp).max(a + 1.0))
    }).collect()
}

/// Draws the waveform of `s` stretched over `inner`, the part inside `clip` only.
pub fn draw(p: &egui::Painter, inner: Rect, clip: Rect, s: &Arc<Vec<f32>>, ink: Color32) {
    let frames = s.len() / 2;
    if frames == 0 || inner.width() < 1.0 {
        return;
    }
    let o = overview(s);
    let w = inner.width() as f64;
    let fpp = frames as f64 / w;
    let mid = inner.center().y;
    let half = inner.height() * 0.5;
    let vis = inner.intersect(clip);
    if !vis.is_positive() {
        return;
    }
    let k0 = ((vis.min.x - inner.min.x).floor().max(0.0)) as usize;
    let k1 = ((vis.max.x - inner.min.x).ceil() as usize + 1).min(w.ceil() as usize + 1);
    if k1 <= k0 {
        return;
    }
    if fpp < 1.0 {
        // closer than a sample a point: the samples (both channels averaged) as a line
        let f0 = (k0 as f64 * fpp).floor() as usize;
        let f1 = ((k1 as f64 * fpp).ceil() as usize + 1).min(frames);
        let pts: Vec<egui::Pos2> = (f0..f1).map(|f| {
            let v = (s[f * 2] + s[f * 2 + 1]) * 0.5;
            pos2(inner.min.x + (f as f64 / fpp) as f32, mid - v.clamp(-1.0, 1.0) * half)
        }).collect();
        p.line(pts, Stroke::new(1.0, ink));
        return;
    }
    let cols = columns(&o, s, fpp, k0, k1);
    // one shape: a quad between each column and the next (a column at least a hairline tall)
    let mut mesh = egui::Mesh::default();
    for (i, (mn, mx)) in cols.iter().enumerate() {
        let x = inner.min.x + (k0 + i) as f32;
        let top = mid - mx.clamp(-1.0, 1.0) * half;
        let bot = (mid - mn.clamp(-1.0, 1.0) * half).max(top + 0.6);
        mesh.colored_vertex(pos2(x, top), ink);
        mesh.colored_vertex(pos2(x, bot), ink);
        if i > 0 {
            let v = (i * 2) as u32;
            mesh.add_triangle(v - 2, v - 1, v);
            mesh.add_triangle(v - 1, v + 1, v);
        }
    }
    p.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overviews_are_built_once_and_dropped_with_the_file() {
        let s = Arc::new((0..20_000).map(|i| ((i as f32) * 0.01).sin()).collect::<Vec<f32>>());
        let a = overview(&s);
        let b = overview(&s);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.frames, 10_000);
        // columns at any zoom hold the true peaks
        for fpp in [3.0, 17.5, 400.0] {
            let cols = columns(&a, &s, fpp, 0, (10_000.0 / fpp) as usize);
            let mx = cols.iter().fold(f32::MIN, |m, c| m.max(c.1));
            let mn = cols.iter().fold(f32::MAX, |m, c| m.min(c.0));
            assert!(mx > 0.99 && mn < -0.99, "{fpp}");
        }
        // past the end: silence
        assert_eq!(columns(&a, &s, 10.0, 5000, 5001)[0], (0.0, 0.0));
        drop((a, b));
        let other = Arc::new(vec![0.0f32; 64]);
        drop(s);
        let _ = overview(&other);
        CACHE.with(|c| assert!(c.borrow().keys().all(|k| k.1 == 64)));
    }
}
