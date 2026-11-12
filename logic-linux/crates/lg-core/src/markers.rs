//! Markers as Logic keeps them: a position, a name (the first paragraph of the marker's text)
//! and the rest of the text, a length (a marker reaches to the next one or the project's end
//! unless it is given a shorter one; markers never overlap), a colour, an SMPTE lock (the
//! marker keeps its time when the tempo changes), and marker sets (only the current set's
//! markers show). Also the markers kept in audio files (WAV `cue ` points with `LIST adtl`
//! labels), which Logic imports and exports.

use crate::events::PPQ;
use crate::timeline::Map;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarkerInfo {
    /// Length in ticks; 0 or 1: up to the next marker (or the project's end).
    pub len: u32,
    /// A colour of the palette (`trackmix::palette`); none: the default marker colour.
    pub color: Option<u8>,
    /// SMPTE position lock: the time (seconds from bar 1) the marker keeps.
    pub lock: Option<f64>,
    /// The text after the name (the Marker Text's further paragraphs).
    pub text: String,
}

/// A marker: (position in ticks from bar 1, name, the rest).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Marker(pub u32, pub String, pub MarkerInfo);

impl Marker {
    pub fn new(pos: u32, name: impl Into<String>) -> Self {
        Marker(pos, name.into(), MarkerInfo::default())
    }

    /// The whole marker text: the name, then the rest as further paragraphs.
    pub fn full_text(&self) -> String {
        if self.2.text.is_empty() {
            self.1.clone()
        } else {
            format!("{}\n{}", self.1, self.2.text)
        }
    }

    /// Sets the whole text: its first paragraph becomes the name.
    pub fn set_full_text(&mut self, s: &str) {
        let (name, rest) = split_text(s);
        self.1 = name;
        self.2.text = rest;
    }

    /// Scene markers (movie cuts) are always SMPTE-locked.
    pub fn is_scene(&self) -> bool {
        self.1.starts_with("Scene ") && self.2.lock.is_some()
    }
}

/// A marker text split into its name (first paragraph) and the rest.
pub fn split_text(s: &str) -> (String, String) {
    match s.split_once('\n') {
        Some((a, b)) => (a.trim_end_matches('\r').to_string(), b.to_string()),
        None => (s.to_string(), String::new()),
    }
}

/// A marker set (Logic: Marker List ▸ Marker Set pop-up).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarkerSet {
    pub name: String,
    pub markers: Vec<Marker>,
}

/// Where a new marker goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rounding {
    None,
    Quarter,
    Bar,
}

pub fn round(map: &Map, tick: u32, r: Rounding) -> u32 {
    match r {
        Rounding::None => tick,
        Rounding::Quarter => ((tick as f64 / PPQ as f64).round() * PPQ as f64) as u32,
        Rounding::Bar => {
            // the playhead's bar, or the next bar when nearer
            let (bar, _, _) = map.bar_of(tick);
            let (a, b) = (map.bar_start(bar), map.bar_start(bar + 1));
            if tick - a <= b - tick {
                a
            } else {
                b
            }
        }
    }
}

/// Where marker `k` ends: its length, cut at the next marker; to the next marker (or `end`)
/// when it has none.
pub fn end_of(ms: &[Marker], k: usize, end: u32) -> u32 {
    let Some(m) = ms.get(k) else { return end };
    let next = ms.get(k + 1).map_or(end.max(m.0 + 1), |n| n.0);
    if m.2.len <= 1 {
        next
    } else {
        (m.0 + m.2.len).min(next)
    }
}

fn is_default_name(n: &str) -> bool {
    n.strip_prefix("Marker ").is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
}

/// Default names ("Marker n") follow the ruler order, counting every marker.
pub fn renumber(ms: &mut [Marker]) {
    for (k, m) in ms.iter_mut().enumerate() {
        if is_default_name(&m.1) {
            m.1 = format!("Marker {}", k + 1);
        }
    }
}

fn sort(ms: &mut [Marker]) {
    ms.sort_by_key(|m| m.0);
}

/// A new marker at `at` (named "Marker n" unless a name is given). None when one is already
/// within a quarter note of it.
pub fn create(ms: &mut Vec<Marker>, at: u32, name: Option<&str>) -> Option<usize> {
    if ms.iter().any(|m| m.0.abs_diff(at) < PPQ) {
        return None;
    }
    ms.push(Marker::new(at, name.unwrap_or("Marker 0")));
    sort(ms);
    renumber(ms);
    ms.iter().position(|m| m.0 == at)
}

/// A marker with its length and colour (Create Marker for Region Selection, cycle drags).
pub fn create_span(ms: &mut Vec<Marker>, at: u32, len: u32, name: Option<&str>, color: Option<u8>) -> Option<usize> {
    let k = create(ms, at, name)?;
    ms[k].2.len = len;
    ms[k].2.color = color;
    let next = ms.get(k + 1).map(|n| n.0);
    if let Some(n) = next {
        if at + len > n {
            ms[k].2.len = n - at;
        }
    }
    Some(k)
}

/// The earliest start marker `k` may have: after the previous marker (and its own length).
fn min_start(ms: &[Marker], k: usize) -> u32 {
    match k.checked_sub(1).and_then(|j| ms.get(j)) {
        Some(p) if p.2.len > 1 => p.0 + p.2.len,
        Some(p) => p.0 + 1,
        None => 0,
    }
}

/// Moves marker `k` to `at` (Marker track drags, the Marker List's Position). Returns its new
/// index (markers stay in ruler order).
pub fn move_to(ms: &mut [Marker], k: usize, at: u32) -> usize {
    let Some(m) = ms.get_mut(k) else { return k };
    m.0 = at;
    let moved = m.clone();
    sort(ms);
    let i = ms.iter().position(|x| *x == moved).unwrap_or(k);
    // a length reaching into the next marker is cut there
    if let Some(n) = ms.get(i + 1).map(|n| n.0) {
        if ms[i].2.len > 1 && ms[i].0 + ms[i].2.len > n {
            ms[i].2.len = n - ms[i].0;
        }
    }
    renumber(ms);
    i
}

/// Option-drag: a copy of marker `k` at `at` (none when another is there).
pub fn copy_to(ms: &mut Vec<Marker>, k: usize, at: u32) -> Option<usize> {
    let m = ms.get(k)?.clone();
    paste(ms, &[m], at).first().copied()
}

/// Pastes markers (as copied: their positions relative to the first) at `at`.
pub fn paste(ms: &mut Vec<Marker>, clip: &[Marker], at: u32) -> Vec<usize> {
    let Some(first) = clip.iter().map(|m| m.0).min() else { return Vec::new() };
    let mut placed = Vec::new();
    for m in clip {
        let pos = at + (m.0 - first);
        if ms.iter().any(|x| x.0.abs_diff(pos) < PPQ) {
            continue;
        }
        let mut c = m.clone();
        c.0 = pos;
        c.2.lock = None;
        ms.push(c);
        placed.push(pos);
    }
    sort(ms);
    renumber(ms);
    placed.iter().filter_map(|p| ms.iter().position(|m| m.0 == *p)).collect()
}

/// Gives marker `k` the range `start..end`, kept off the markers around it (edge drags, the
/// Marker List's Length, a cycle dropped on it). A length of one tick or less reaches to the
/// next marker.
pub fn set_range(ms: &mut [Marker], k: usize, start: u32, end: u32) {
    if k >= ms.len() {
        return;
    }
    let lo = min_start(ms, k);
    let next = ms.get(k + 1).map(|n| n.0);
    let start = start.max(lo).min(next.map_or(u32::MAX, |n| n.saturating_sub(1)));
    let mut end = end.max(start + 1);
    if let Some(n) = next {
        end = end.min(n);
    }
    ms[k].0 = start;
    // reaching the next marker: kept as "up to the next one"
    ms[k].2.len = if Some(end) == next { 0 } else { end - start };
}

/// The Marker List's Length: 1 tick (or 0) means up to the next marker.
pub fn set_len(ms: &mut [Marker], k: usize, len: u32) {
    let Some(start) = ms.get(k).map(|m| m.0) else { return };
    if len <= 1 {
        ms[k].2.len = 0;
    } else {
        set_range(ms, k, start, start + len);
    }
}

/// Shift-Command-click: the marker's start trimmed to `at` (its end kept).
pub fn trim_start(ms: &mut [Marker], k: usize, at: u32, end: u32) {
    let e = end_of(ms, k, end);
    if at < e {
        set_range(ms, k, at, e);
    }
}

/// Command-click: the marker's end trimmed to `at`.
pub fn trim_end(ms: &mut [Marker], k: usize, at: u32) {
    if let Some(s) = ms.get(k).map(|m| m.0) {
        if at > s {
            set_range(ms, k, s, at);
        }
    }
}

/// The Junction tool: the boundary between markers `k` and `k + 1` moved to `at`, no gap.
pub fn junction(ms: &mut [Marker], k: usize, at: u32, end: u32) {
    if k + 1 >= ms.len() {
        return;
    }
    let next_end = end_of(ms, k + 1, end);
    let at = at.max(ms[k].0 + 1).min(next_end.saturating_sub(1));
    ms[k].2.len = 0;
    let explicit = ms[k + 1].2.len > 1;
    ms[k + 1].0 = at;
    if explicit {
        ms[k + 1].2.len = next_end - at;
    }
}

/// SMPTE lock: the marker keeps its time from now on (Lock SMPTE Position).
pub fn lock(ms: &mut [Marker], k: usize, map: &Map) {
    if let Some(m) = ms.get_mut(k) {
        m.2.lock = Some(map.secs_at(m.0 as f64));
    }
}

/// Unlock SMPTE Position (scene markers stay locked). Returns whether it was unlocked.
pub fn unlock(ms: &mut [Marker], k: usize) -> bool {
    match ms.get_mut(k) {
        Some(m) if !m.is_scene() => {
            m.2.lock = None;
            true
        }
        _ => false,
    }
}

/// After a tempo change: locked markers back at their times (new bar positions). Returns
/// whether any moved.
pub fn relock(ms: &mut [Marker], map: &Map) -> bool {
    let mut moved = false;
    for m in ms.iter_mut().filter(|m| !m.is_scene()) {
        // (scene markers follow the movie: movie.rs places them)
        if let Some(s) = m.2.lock {
            let t = map.tick_at(s).round().max(0.0) as u32;
            if t != m.0 {
                m.0 = t;
                moved = true;
            }
        }
    }
    if moved {
        sort(ms);
    }
    moved
}

// ---- marker sets ----------------------------------------------------------------------------

/// The sets, with the current one's markers being `current` (the first set made from them when
/// there were none).
fn ensure(sets: &mut Vec<MarkerSet>, current: &[Marker]) {
    if sets.is_empty() {
        sets.push(MarkerSet { name: "Marker Set 1".into(), markers: current.to_vec() });
    }
}

/// New Set: the current markers kept in their set, a new empty set chosen.
pub fn new_set(sets: &mut Vec<MarkerSet>, cur: &mut usize, current: &mut Vec<Marker>, name: Option<&str>) {
    ensure(sets, current);
    if let Some(s) = sets.get_mut(*cur) {
        s.markers = current.clone();
    }
    let name = name.map_or_else(|| format!("Marker Set {}", sets.len() + 1), str::to_string);
    sets.push(MarkerSet { name, markers: Vec::new() });
    *cur = sets.len() - 1;
    current.clear();
}

/// Chooses set `k`: its markers become the current ones.
pub fn choose_set(sets: &mut [MarkerSet], cur: &mut usize, current: &mut Vec<Marker>, k: usize) {
    if k >= sets.len() || k == *cur {
        return;
    }
    if let Some(s) = sets.get_mut(*cur) {
        s.markers = current.clone();
    }
    *cur = k;
    *current = sets[k].markers.clone();
}

pub fn rename_set(sets: &mut Vec<MarkerSet>, cur: usize, current: &[Marker], name: &str) {
    ensure(sets, current);
    if let Some(s) = sets.get_mut(cur) {
        s.name = name.to_string();
    }
}

/// Delete Set: the current set goes, the one before it is chosen (the last set can't go).
pub fn delete_set(sets: &mut Vec<MarkerSet>, cur: &mut usize, current: &mut Vec<Marker>) -> bool {
    if sets.len() <= 1 {
        return false;
    }
    sets.remove(*cur);
    *cur = cur.saturating_sub(1).min(sets.len() - 1);
    *current = sets[*cur].markers.clone();
    true
}

// ---- markers in audio files (WAV cue points) ------------------------------------------------

fn chunks(b: &[u8]) -> Vec<(usize, [u8; 4], usize)> {
    let mut out = Vec::new();
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return out;
    }
    let mut o = 12;
    while o + 8 <= b.len() {
        let id: [u8; 4] = b[o..o + 4].try_into().unwrap();
        let n = u32::from_le_bytes(b[o + 4..o + 8].try_into().unwrap()) as usize;
        out.push((o, id, n));
        o += 8 + n + (n & 1);
    }
    out
}

/// The markers in a WAV file: (sample frame, name).
pub fn wav_markers(b: &[u8]) -> Vec<(u32, String)> {
    let cs = chunks(b);
    let mut cues: Vec<(u32, u32)> = Vec::new();
    let mut labels: std::collections::HashMap<u32, String> = Default::default();
    for &(o, id, n) in &cs {
        let body = &b[o + 8..(o + 8 + n).min(b.len())];
        if &id == b"cue " && body.len() >= 4 {
            let count = u32::from_le_bytes(body[0..4].try_into().unwrap()) as usize;
            for i in 0..count {
                let r = 4 + i * 24;
                if r + 24 > body.len() {
                    break;
                }
                let cid = u32::from_le_bytes(body[r..r + 4].try_into().unwrap());
                let off = u32::from_le_bytes(body[r + 20..r + 24].try_into().unwrap());
                cues.push((cid, off));
            }
        }
        if &id == b"LIST" && body.len() >= 4 && &body[0..4] == b"adtl" {
            let mut q = 4;
            while q + 8 <= body.len() {
                let sid = &body[q..q + 4];
                let sn = u32::from_le_bytes(body[q + 4..q + 8].try_into().unwrap()) as usize;
                let sb = &body[q + 8..(q + 8 + sn).min(body.len())];
                if sid == b"labl" && sb.len() >= 4 {
                    let cid = u32::from_le_bytes(sb[0..4].try_into().unwrap());
                    let text = String::from_utf8_lossy(sb[4..].split(|c| *c == 0).next().unwrap_or(&[])).into_owned();
                    labels.insert(cid, text);
                }
                q += 8 + sn + (sn & 1);
            }
        }
    }
    let mut out: Vec<(u32, String)> = cues.into_iter().map(|(id, off)| (off, labels.remove(&id).unwrap_or_else(|| format!("Marker {id}")))).collect();
    out.sort_by_key(|m| m.0);
    out
}

/// The WAV file with its markers replaced (none: removed).
pub fn with_wav_markers(b: &[u8], markers: &[(u32, String)]) -> Option<Vec<u8>> {
    let cs = chunks(b);
    if cs.is_empty() {
        return None;
    }
    let mut out = b[0..12].to_vec();
    for &(o, id, n) in &cs {
        let is_adtl = &id == b"LIST" && b.get(o + 8..o + 12) == Some(b"adtl");
        if &id == b"cue " || is_adtl {
            continue;
        }
        let end = (o + 8 + n + (n & 1)).min(b.len());
        out.extend_from_slice(&b[o..end]);
        if n & 1 == 1 && end == b.len() {
            out.push(0);
        }
    }
    if !markers.is_empty() {
        let mut cue = (markers.len() as u32).to_le_bytes().to_vec();
        let mut adtl = b"adtl".to_vec();
        for (k, (off, name)) in markers.iter().enumerate() {
            let id = k as u32 + 1;
            cue.extend(id.to_le_bytes());
            cue.extend(off.to_le_bytes());
            cue.extend(b"data");
            cue.extend(0u32.to_le_bytes());
            cue.extend(0u32.to_le_bytes());
            cue.extend(off.to_le_bytes());
            let mut l = id.to_le_bytes().to_vec();
            l.extend(name.as_bytes());
            l.push(0);
            adtl.extend(b"labl");
            adtl.extend((l.len() as u32).to_le_bytes());
            let odd = l.len() & 1 == 1;
            adtl.extend(l);
            if odd {
                adtl.push(0);
            }
        }
        for (id, body) in [(b"cue ", cue), (b"LIST", adtl)] {
            out.extend(id);
            out.extend((body.len() as u32).to_le_bytes());
            out.extend(body);
        }
    }
    let size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(at: &[u32]) -> Vec<Marker> {
        let mut v = Vec::new();
        for &a in at {
            create(&mut v, a, None);
        }
        v
    }

    #[test]
    fn names_follow_ruler_order_and_quarter_rule() {
        let mut v = ms(&[3840, 0]);
        assert_eq!(v.iter().map(|m| m.1.as_str()).collect::<Vec<_>>(), ["Marker 1", "Marker 2"]);
        v[1].1 = "Chorus".into();
        create(&mut v, 1920, None);
        assert_eq!(v[1].1, "Marker 2");
        assert_eq!(v[2].1, "Chorus");
        // within a quarter note of one: none
        assert!(create(&mut v, 1920 + PPQ - 1, None).is_none());
        assert!(create(&mut v, 1920 + PPQ, None).is_some());
    }

    #[test]
    fn lengths_never_overlap() {
        let mut v = ms(&[0, 3840, 7680]);
        assert_eq!(end_of(&v, 0, 20000), 3840);
        assert_eq!(end_of(&v, 2, 20000), 20000);
        set_range(&mut v, 0, 0, 1000);
        assert_eq!(end_of(&v, 0, 20000), 1000);
        set_range(&mut v, 0, 0, 9000);
        assert_eq!(end_of(&v, 0, 20000), 3840);
        // the start can't pass the previous marker
        set_range(&mut v, 1, 100, 3840 + 500);
        assert!(v[1].0 > 0);
        set_len(&mut v, 1, 1);
        assert_eq!(end_of(&v, 1, 20000), 7680);
        trim_end(&mut v, 2, 9000);
        assert_eq!(end_of(&v, 2, 20000), 9000);
        trim_start(&mut v, 2, 8000, 20000);
        assert_eq!((v[2].0, end_of(&v, 2, 20000)), (8000, 9000));
        let mut w = ms(&[0, 3840]);
        junction(&mut w, 0, 2000, 20000);
        assert_eq!((end_of(&w, 0, 20000), w[1].0), (2000, 2000));
    }

    #[test]
    fn locked_markers_keep_their_time() {
        let mut v = ms(&[3840]);
        let slow = Map::tempo(120.0);
        lock(&mut v, 0, &slow);
        let fast = Map::tempo(60.0);
        assert!(relock(&mut v, &fast));
        assert_eq!(v[0].0, 1920);
        assert!(unlock(&mut v, 0));
        let mut s = vec![Marker(0, "Scene 1".into(), MarkerInfo { lock: Some(0.0), ..Default::default() })];
        assert!(!unlock(&mut s, 0));
    }

    #[test]
    fn copy_paste_and_sets() {
        let mut v = ms(&[0, 3840]);
        v[0].2.color = Some(3);
        let placed = copy_to(&mut v, 0, 7680).unwrap();
        assert_eq!(v[placed].2.color, Some(3));
        assert_eq!(v.len(), 3);
        let (mut sets, mut cur) = (Vec::new(), 0);
        new_set(&mut sets, &mut cur, &mut v, None);
        assert!(v.is_empty());
        assert_eq!((sets.len(), cur, sets[1].name.as_str()), (2, 1, "Marker Set 2"));
        create(&mut v, 0, None);
        choose_set(&mut sets, &mut cur, &mut v, 0);
        assert_eq!(v.len(), 3);
        choose_set(&mut sets, &mut cur, &mut v, 1);
        assert_eq!(v.len(), 1);
        assert!(delete_set(&mut sets, &mut cur, &mut v));
        assert_eq!(v.len(), 3);
        assert!(!delete_set(&mut sets, &mut cur, &mut v));
    }

    #[test]
    fn text_first_paragraph_is_the_name() {
        let mut m = Marker::new(0, "x");
        m.set_full_text("Verse\nsing softly\nthen loud");
        assert_eq!((m.1.as_str(), m.2.text.as_str()), ("Verse", "sing softly\nthen loud"));
        assert_eq!(m.full_text(), "Verse\nsing softly\nthen loud");
    }

    #[test]
    fn wav_cue_round_trip() {
        let mut w = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        w.extend(16u32.to_le_bytes());
        w.extend([1, 0, 1, 0, 0x44, 0xac, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0]);
        w.extend(b"data");
        w.extend(4u32.to_le_bytes());
        w.extend([0, 0, 0, 0]);
        let n = (w.len() - 8) as u32;
        w[4..8].copy_from_slice(&n.to_le_bytes());
        let m = vec![(10, "Intro".to_string()), (44100, "Odd".to_string())];
        let out = with_wav_markers(&w, &m).unwrap();
        assert_eq!(wav_markers(&out), m);
        let again = with_wav_markers(&out, &m[..1]).unwrap();
        assert_eq!(wav_markers(&again), m[..1].to_vec());
        let none = with_wav_markers(&again, &[]).unwrap();
        assert!(wav_markers(&none).is_empty());
        assert_eq!(none, w);
    }
}
