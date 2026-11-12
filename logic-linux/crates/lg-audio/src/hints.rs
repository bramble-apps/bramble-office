//! Smart Tempo hints, locked ranges and the beat markers' handles (Logic's Smart Tempo
//! Editor). Hints (downbeat, beat, and time signature hints on downbeats, plus one general
//! signature for the file) steer a new analysis: the beats pass through every hint, the beats
//! between two hints spaced evenly (as many as the analysed tempo puts there; whole bars
//! between two downbeat hints), the tempo carried on before the first and after the last.
//! Locked ranges keep their beats through edits and new analyses. The handles move or scale
//! beat markers without a new analysis.

use crate::tempo::BeatMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hint {
    /// Seconds in the file.
    pub sec: f64,
    pub downbeat: bool,
    /// A time signature hint (on a downbeat hint).
    pub sig: Option<(u8, u8)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hints {
    pub list: Vec<Hint>,
    /// The general time signature hint (the Signature display in Hint mode).
    pub general_sig: Option<(u8, u8)>,
    /// Locked ranges (seconds): edits and analyses leave their beats alone.
    pub locks: Vec<(f64, f64)>,
    /// Hint edits not analysed yet (the Apply Changes button turns orange).
    pub pending: bool,
}

impl Hints {
    pub fn add(&mut self, sec: f64, downbeat: bool) {
        // a hint close to another replaces it
        self.list.retain(|h| (h.sec - sec).abs() > 0.03);
        self.list.push(Hint { sec, downbeat, sig: None });
        self.list.sort_by(|a, b| a.sec.total_cmp(&b.sec));
        self.pending = true;
    }

    pub fn nearest(&self, sec: f64, within: f64) -> Option<usize> {
        (0..self.list.len()).filter(|&i| (self.list[i].sec - sec).abs() <= within).min_by(|&a, &b| (self.list[a].sec - sec).abs().total_cmp(&(self.list[b].sec - sec).abs()))
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.list.len() {
            self.list.remove(i);
            self.pending = true;
        }
    }

    pub fn move_to(&mut self, i: usize, sec: f64) -> usize {
        let Some(mut h) = self.list.get(i).copied() else { return i };
        self.list.remove(i);
        h.sec = sec.max(0.0);
        let k = self.list.partition_point(|x| x.sec < h.sec);
        self.list.insert(k, h);
        self.pending = true;
        k
    }

    /// Removes the hints inside (a, b) (tapping hints in a selection replaces those there).
    pub fn clear_range(&mut self, a: f64, b: f64) {
        let n = self.list.len();
        self.list.retain(|h| h.sec < a || h.sec > b);
        self.pending |= n != self.list.len();
    }

    pub fn convert(&mut self, to_downbeat: bool) {
        for h in &mut self.list {
            if h.downbeat != to_downbeat {
                h.downbeat = to_downbeat;
                if !to_downbeat {
                    h.sig = None;
                }
                self.pending = true;
            }
        }
    }

    pub fn locked(&self, sec: f64) -> bool {
        self.locks.iter().any(|&(a, b)| sec >= a - 1e-9 && sec <= b + 1e-9)
    }

    pub fn lock(&mut self, a: f64, b: f64) {
        let (a, b) = (a.min(b), a.max(b));
        self.locks.retain(|&(x, y)| y < a || x > b);
        self.locks.push((a, b));
        self.locks.sort_by(|p, q| p.0.total_cmp(&q.0));
    }

    /// Unlocks the locked ranges touching (a, b).
    pub fn unlock(&mut self, a: f64, b: f64) -> bool {
        let n = self.locks.len();
        self.locks.retain(|&(x, y)| y < a.min(b) || x > a.max(b));
        n != self.locks.len()
    }
}

/// A new analysis of `base` (the file's analysis without hints) steered by `hints`, over
/// `seconds` of file. `prev` (the map as it is) keeps its beats inside locked ranges.
pub fn reanalyze(base: &BeatMap, prev: &BeatMap, hints: &Hints, seconds: f64) -> BeatMap {
    let mut out = base.clone();
    if let Some((n, d)) = hints.general_sig {
        out.beats_per_bar = n.max(1);
        out.beat_unit = d;
    }
    if let Some((n, d)) = hints.list.iter().find_map(|h| h.sig.filter(|_| h.downbeat)) {
        out.beats_per_bar = n.max(1);
        out.beat_unit = d;
    }
    let bpb = out.beats_per_bar.max(1) as usize;
    let anchors = &hints.list;
    let period_at = |s: f64| 60.0 / base.tempo_at(s).max(1.0);
    if !anchors.is_empty() {
        let mut beats: Vec<f64> = Vec::new();
        let mut periods = Vec::new();
        for w in anchors.windows(2) {
            let g = w[1].sec - w[0].sec;
            if g <= 0.02 {
                continue;
            }
            let p = period_at((w[0].sec + w[1].sec) / 2.0);
            let n = if w[0].downbeat && w[1].downbeat { ((g / (p * bpb as f64)).round().max(1.0) as usize) * bpb } else { (g / p).round().max(1.0) as usize };
            let step = g / n as f64;
            periods.push(step);
            for k in 0..n {
                beats.push(w[0].sec + step * k as f64);
            }
        }
        let last = anchors[anchors.len() - 1].sec;
        beats.push(last);
        let first_p = periods.first().copied().unwrap_or_else(|| period_at(anchors[0].sec));
        let last_p = periods.last().copied().unwrap_or_else(|| period_at(last));
        let mut before = Vec::new();
        let mut s = anchors[0].sec - first_p;
        while s >= -1e-9 {
            before.push(s);
            s -= first_p;
        }
        before.reverse();
        let mut s = last + last_p;
        while s <= seconds + last_p * 0.5 {
            beats.push(s);
            s += last_p;
        }
        let lead = before.len();
        before.extend(beats);
        out.beats = before;
        // the first downbeat hint is a downbeat; else the analysed downbeat's place
        out.downbeat = match anchors.iter().position(|h| h.downbeat) {
            Some(_) => {
                let s = anchors.iter().find(|h| h.downbeat).unwrap().sec;
                out.nearest_beat(s).unwrap_or(lead)
            }
            None => base.beats.get(base.downbeat).and_then(|&s| out.nearest_beat(s)).unwrap_or(0),
        };
        out.downbeat = out.downbeat.min(out.beats.len().saturating_sub(1));
        let distinct = periods.windows(2).any(|w| (w[0] - w[1]).abs() > 0.002);
        out.variable = base.variable || distinct;
    }
    keep_locked(&mut out, prev, hints);
    out.hints = hints.clone();
    out.hints.pending = false;
    out
}

/// Puts back `prev`'s beats inside locked ranges (the downbeat's time kept).
pub fn keep_locked(map: &mut BeatMap, prev: &BeatMap, hints: &Hints) {
    if hints.locks.is_empty() {
        return;
    }
    let down = map.beats.get(map.downbeat).copied();
    let mut beats: Vec<f64> = map.beats.iter().copied().filter(|&s| !hints.locked(s)).collect();
    beats.extend(prev.beats.iter().copied().filter(|&s| hints.locked(s)));
    beats.sort_by(|a, b| a.total_cmp(b));
    beats.dedup_by(|a, b| (*a - *b).abs() < 0.02);
    map.beats = beats;
    if let Some(d) = down {
        map.downbeat = map.nearest_beat(d).unwrap_or(0);
    }
}

/// The beat markers' handles (Beat Marker mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    SetDownbeat,
    ScaleSelection,
    ScaleLeftMoveRight,
    ScaleAll,
    MoveAll,
}

impl Handle {
    pub const ALL: [Handle; 5] = [Handle::SetDownbeat, Handle::ScaleSelection, Handle::ScaleLeftMoveRight, Handle::ScaleAll, Handle::MoveAll];

    /// The tag shown on hover.
    pub fn tag(self) -> &'static str {
        match self {
            Handle::SetDownbeat => "Set Downbeat",
            Handle::ScaleSelection => "Scale Selection",
            Handle::ScaleLeftMoveRight => "Scale Left, Move Right",
            Handle::ScaleAll => "Scale All",
            Handle::MoveAll => "Move All",
        }
    }
}

fn is_down(map: &BeatMap, i: usize) -> bool {
    map.bar_beat(i).1 == 1
}

/// The beats a handle works on when nothing is selected: the marker's bar (a downbeat: the
/// bars before and after); Scale Left, Move Right on a downbeat: the bar before and the rest.
pub fn default_range(map: &BeatMap, i: usize, h: Handle) -> (usize, usize) {
    let n = map.beats.len();
    let bpb = map.beats_per_bar.max(1) as usize;
    let last = n.saturating_sub(1);
    let bar_start = |k: usize| (0..=k).rev().find(|&j| is_down(map, j)).unwrap_or(0);
    match h {
        Handle::ScaleAll | Handle::MoveAll => (0, last),
        Handle::ScaleLeftMoveRight if is_down(map, i) => (i.saturating_sub(bpb), last),
        _ if is_down(map, i) => (i.saturating_sub(bpb), (i + bpb).min(last)),
        _ => {
            let a = bar_start(i);
            (a, (a + bpb).min(last))
        }
    }
}

/// Whether a handle shows on beat `i` (Set Downbeat: not on a downbeat).
pub fn shows(map: &BeatMap, i: usize, h: Handle) -> bool {
    !(h == Handle::SetDownbeat && is_down(map, i))
}

fn lin(beats: &mut [f64], a: usize, k: usize, new: f64) {
    // beats a..=k scaled about a so k lands on `new`
    let (sa, sk) = (beats[a], beats[k]);
    if (sk - sa).abs() < 1e-9 {
        return;
    }
    let f = (new - sa) / (sk - sa);
    for b in beats.iter_mut().take(k + 1).skip(a) {
        *b = sa + (*b - sa) * f;
    }
}

/// Drags handle `h` of beat `i` to `sec`, over beats `range` (inclusive). Returns false when
/// nothing could change (it would cross a neighbour). Locked beats stay.
pub fn drag(map: &mut BeatMap, i: usize, h: Handle, range: (usize, usize), sec: f64, hints: &Hints) -> bool {
    let n = map.beats.len();
    if i >= n {
        return false;
    }
    let (a, b) = (range.0.min(range.1), range.0.max(range.1).min(n - 1));
    let before = map.clone();
    let d = sec - map.beats[i];
    match h {
        Handle::SetDownbeat => {
            map.downbeat = i;
            return true;
        }
        Handle::MoveAll => {
            for s in &mut map.beats[a..=b] {
                *s += d;
            }
        }
        Handle::ScaleAll => {
            if i <= a {
                return false;
            }
            let (sa, si) = (map.beats[a], map.beats[i]);
            let f = (sec - sa) / (si - sa);
            for s in &mut map.beats[a..=b] {
                *s = sa + (*s - sa) * f;
            }
        }
        Handle::ScaleLeftMoveRight => {
            if i <= a {
                return false;
            }
            lin(&mut map.beats, a, i, sec);
            for s in &mut map.beats[i + 1..] {
                *s += d;
            }
        }
        Handle::ScaleSelection => {
            if i <= a || i >= b {
                return false;
            }
            lin(&mut map.beats, a, i, sec);
            // i..=b scaled about b
            let (sb, si) = (map.beats[b], before.beats[i]);
            let f = (sb - sec) / (sb - si);
            for s in &mut map.beats[i + 1..b] {
                *s = sb - (sb - *s) * f;
            }
        }
    }
    let ok = map.beats.windows(2).all(|w| w[1] - w[0] > 0.02) && map.beats[0] > -0.5;
    if !ok {
        *map = before;
        return false;
    }
    map.variable = true;
    keep_locked(map, &before, hints);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> BeatMap {
        // 120 BPM, 4/4, beats every 0.5 s for 8 s
        BeatMap::constant(120.0, 0.0, 8.0, 4)
    }

    #[test]
    fn hints_steer_the_beats() {
        let base = grid();
        let mut h = Hints::default();
        // downbeat hints 2.2 s apart (one bar a bit slower), and a beat hint later
        h.add(1.0, true);
        h.add(3.2, true);
        h.add(4.2, false);
        let m = reanalyze(&base, &base, &h, 8.0);
        assert!(m.beats.iter().any(|&b| (b - 1.0).abs() < 1e-9));
        assert!(m.beats.iter().any(|&b| (b - 3.2).abs() < 1e-9));
        assert!(m.beats.iter().any(|&b| (b - 4.2).abs() < 1e-9));
        // four beats between the downbeat hints, each 0.55 s
        let i = m.nearest_beat(1.0).unwrap();
        assert!((m.beats[i + 1] - 1.55).abs() < 1e-9);
        assert_eq!(m.bar_beat(i), (1, 1));
        assert!(m.variable);
        // general signature hint
        h.general_sig = Some((3, 4));
        h.list.clear();
        let m = reanalyze(&base, &base, &h, 8.0);
        assert_eq!(m.beats_per_bar, 3);
    }

    #[test]
    fn hint_list_edits() {
        let mut h = Hints::default();
        h.add(1.0, true);
        h.add(2.0, false);
        h.add(1.01, false);
        assert_eq!(h.list.len(), 2);
        assert!(!h.list[0].downbeat);
        let k = h.move_to(0, 3.0);
        assert_eq!(k, 1);
        h.convert(true);
        assert!(h.list.iter().all(|x| x.downbeat));
        h.clear_range(1.5, 2.5);
        assert_eq!(h.list.len(), 1);
        assert_eq!(h.nearest(3.02, 0.05), Some(0));
        h.remove(0);
        assert!(h.list.is_empty());
    }

    #[test]
    fn locked_ranges_survive() {
        let base = grid();
        let mut prev = base.clone();
        prev.beats[4] = 2.1; // an edit inside the range to keep
        let mut h = Hints::default();
        h.lock(1.9, 2.2);
        h.add(0.0, true);
        h.add(4.4, true);
        let m = reanalyze(&base, &prev, &h, 8.0);
        assert!(m.beats.iter().any(|&b| (b - 2.1).abs() < 1e-9));
        assert!(h.unlock(2.0, 2.0));
        assert!(h.locks.is_empty());
    }

    #[test]
    fn handles() {
        let mut m = grid();
        let h = Hints::default();
        // Move All shifts everything
        assert!({ let n = m.beats.len(); drag(&mut m, 2, Handle::MoveAll, (0, n - 1), 1.1, &h) });
        assert!((m.beats[0] - 0.1).abs() < 1e-9);
        // Scale Left, Move Right: beats 0..=4 scaled, the rest shifted
        let mut m = grid();
        assert!(drag(&mut m, 4, Handle::ScaleLeftMoveRight, (0, 4), 2.4, &h));
        assert!((m.beats[2] - 1.2).abs() < 1e-9);
        assert!((m.beats[5] - 2.9).abs() < 1e-9);
        // Scale Selection: both sides of beat 2 within 0..=4
        let mut m = grid();
        assert!(drag(&mut m, 2, Handle::ScaleSelection, (0, 4), 1.2, &h));
        assert!((m.beats[1] - 0.6).abs() < 1e-9);
        assert!((m.beats[3] - 1.6).abs() < 1e-9);
        assert!((m.beats[4] - 2.0).abs() < 1e-9);
        // Scale All about the first beat
        let mut m = grid();
        assert!({ let n = m.beats.len(); drag(&mut m, 2, Handle::ScaleAll, (0, n - 1), 2.0, &h) });
        assert!((m.beats[4] - 4.0).abs() < 1e-9);
        // crossing a neighbour is refused
        let mut m = grid();
        assert!(!drag(&mut m, 2, Handle::ScaleSelection, (0, 4), 2.5, &h));
        // defaults: a beat's bar; a downbeat: the bars around it
        let m = grid();
        assert_eq!(default_range(&m, 5, Handle::ScaleSelection), (4, 8));
        assert_eq!(default_range(&m, 4, Handle::ScaleSelection), (0, 8));
        assert_eq!(default_range(&m, 4, Handle::ScaleLeftMoveRight), (0, m.beats.len() - 1));
        assert!(!shows(&m, 4, Handle::SetDownbeat));
        assert!(shows(&m, 5, Handle::SetDownbeat));
        // a locked range keeps its beats through a handle drag
        let mut m = grid();
        let mut lh = Hints::default();
        lh.lock(3.9, 4.1);
        assert!({ let n = m.beats.len(); drag(&mut m, 2, Handle::MoveAll, (0, n - 1), 1.1, &lh) });
        assert!(m.beats.iter().any(|&b| (b - 4.0).abs() < 1e-9));
    }
}
