//! Save's second half: what a project edited in Studio changed beyond notes, regions, tempo and
//! automation (save.rs), written into Logic's ProjectData where the format is decoded, and
//! named in the report where it isn't.
//!
//! Written (format-analysis sections in brackets):
//! - track names: the channel's Envi object, u16 length + text at payload 158 (§15)
//! - region names: a MIDI region's own MSeq (u16 length + text at payload 16, §14); an audio
//!   region's AuRg (length-prefixed name)
//! - audio region file windows: AuRg offset @0x06 and length @0x16 in samples (§25)
//! - channel strips (tracks' and auxes'): AuCO volume @0x79 and pan @0x7D (0..127), solo @0x7C
//!   and mute @0x7E bit 0 (§15, §24)
//! - group membership (Envi payload 0x18) and an existing group's settings: link mask (MSeq
//!   0x46..0x49) and its linked-parameter records (0x50 in its event list) (§23, §25)
//! - plug-in settings: the floats of a plug-in's GAMETSPP block, when the plug-in in that slot
//!   is the one that was read (§3, E05)
//! - the project's time signature and key: the first 0x30 / 0x32 records of the signature list
//!   (EvSq id 0, header index 1) and Song +0x39E (E03, §12, §13)
//! - tempo changes: the tempo list (EvSq id 0, header index 3) rebuilt from 0x60 records, with
//!   the cached time (half-milliseconds from 01:00:00:00) worked out (§11)
//! - markers (Marker list: EvSq 0x40000, header index 22) and arrangement markers (EvSq id 0,
//!   header index 5), with their names in TxSq text chunks (§10)
//!
//! Left out (named in the report): new tracks and deleted tracks (a channel's Envi / AuCO /
//! AuCU objects aren't decoded far enough to make or retire one), track order, colours, plug-ins
//! added / removed / replaced and bypass, sends, outputs, new groups and group names, VCAs, region gain and the
//! Region inspector's other values.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::chunks::{self, Chunk};
use crate::events::{self, EVENT_BAR_ONE};
use crate::project::{Project, Track};

/// Chunk edits: a new payload for a chunk (by index), new whole chunks after one.
#[derive(Default)]
struct Edits {
    payload: HashMap<usize, Vec<u8>>,
    after: BTreeMap<usize, Vec<Vec<u8>>>,
}

impl Edits {
    fn is_empty(&self) -> bool {
        self.payload.is_empty() && self.after.is_empty()
    }
}

fn rebuild(data: &[u8], chunks: &[Chunk], e: &Edits) -> Vec<u8> {
    if e.is_empty() {
        return data.to_vec();
    }
    let mut body = Vec::with_capacity(data.len() + 4096);
    for (i, c) in chunks.iter().enumerate() {
        match e.payload.get(&i) {
            Some(pl) => {
                let mut h = c.raw[..36].to_vec();
                h[28..36].copy_from_slice(&(pl.len() as u64).to_le_bytes());
                body.extend(h);
                body.extend(pl);
            }
            None => body.extend(c.raw),
        }
        if let Some(extra) = e.after.get(&i) {
            for x in extra {
                body.extend(x);
            }
        }
    }
    let mut out = data[..24].to_vec();
    out[0x10..0x14].copy_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

/// A payload with the u16-length-prefixed text at `off` replaced (the rest moved along).
pub fn set_short_string(payload: &[u8], off: usize, text: &str) -> Option<Vec<u8>> {
    let n = u16::from_le_bytes(payload.get(off..off + 2)?.try_into().ok()?) as usize;
    payload.get(off + 2..off + 2 + n)?;
    let mut out = payload[..off].to_vec();
    out.extend_from_slice(&(text.len() as u16).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(&payload[off + 2 + n..]);
    Some(out)
}

fn not_undo(c: &Chunk) -> bool {
    !c.is("USEl") && !c.is("UStk")
}

/// The last chunk (index) with this tag for which `f` holds (the reader takes the last copy).
fn last(chunks: &[Chunk], tag: &str, f: impl Fn(&Chunk) -> bool) -> Option<usize> {
    (0..chunks.len()).rev().find(|&i| chunks[i].is(tag) && not_undo(&chunks[i]) && f(&chunks[i]))
}

/// The edited payload of chunk `i` (an earlier edit in this pass, or the chunk's own).
fn current<'a>(e: &'a Edits, chunks: &'a [Chunk], i: usize) -> &'a [u8] {
    e.payload.get(&i).map_or(chunks[i].payload, |v| v.as_slice())
}

/// The track as it was read, for a track now (same channel object).
fn before_of(before: &Project, t: &Track) -> Option<usize> {
    (t.channel != 0).then(|| before.tracks.iter().position(|b| b.channel == t.channel)).flatten()
}

/// Runs every pass over `data` (save.rs's output); returns the new bytes and what couldn't be
/// written.
pub fn apply(p: &Project, before: &Project, mut data: Vec<u8>) -> (Vec<u8>, Vec<String>) {
    let mut left = Vec::new();
    type Pass = fn(&Project, &Project, &[u8], &[Chunk], &mut Edits, &mut Vec<String>);
    let passes: [Pass; 7] = [names, audio_windows, strips, plugins, signature_and_key, tempo_list, markers];
    for pass in passes {
        let new = {
            let Ok(chunks) = chunks::walk(&data) else { break };
            let mut e = Edits::default();
            pass(p, before, &data, &chunks, &mut e, &mut left);
            if e.is_empty() {
                None
            } else {
                Some(rebuild(&data, &chunks, &e))
            }
        };
        if let Some(n) = new {
            data = n;
        }
    }
    tracks_left_out(p, before, &mut left);
    (data, left)
}

/// What can't be written about the tracks themselves.
fn tracks_left_out(p: &Project, before: &Project, left: &mut Vec<String>) {
    for t in &p.tracks {
        if t.channel == 0 || before_of(before, t).is_none() {
            left.push(format!("new track “{}”", t.name));
        }
    }
    let now: HashSet<u32> = p.tracks.iter().map(|t| t.channel).collect();
    for b in &before.tracks {
        if b.channel != 0 && !now.contains(&b.channel) {
            left.push(format!("deleted track “{}” (stays, without its regions)", b.name));
        }
    }
    let order = |ts: &[Track]| ts.iter().filter(|t| t.channel != 0).map(|t| t.channel).collect::<Vec<_>>();
    let kept: Vec<u32> = order(&before.tracks).into_iter().filter(|c| now.contains(c)).collect();
    let mut seen = HashSet::new();
    let now_order: Vec<u32> = order(&p.tracks).into_iter().filter(|c| seen.insert(*c) && kept.contains(c)).collect();
    let mut seen = HashSet::new();
    let kept: Vec<u32> = kept.into_iter().filter(|c| seen.insert(*c)).collect();
    if now_order != kept {
        left.push("track order".into());
    }
}

// --- names -------------------------------------------------------------------------------

fn names(p: &Project, before: &Project, _data: &[u8], chunks: &[Chunk], e: &mut Edits, _left: &mut Vec<String>) {
    let mut done = HashSet::new();
    for t in &p.tracks {
        let Some(b) = before_of(before, t) else { continue };
        if t.name == before.tracks[b].name || !done.insert(t.channel) {
            continue;
        }
        if let Some(i) = last(chunks, "Envi", |c| c.id >> 16 == t.channel && chunks::short_string(c.payload, 158).is_some()) {
            if let Some(pl) = set_short_string(current(e, chunks, i), 158, &t.name) {
                e.payload.insert(i, pl);
            }
        }
    }
    // regions: by where they were read from
    let was: HashMap<crate::project::Source, &crate::project::Region> = before.tracks.iter().flat_map(|t| t.regions.iter()).filter_map(|r| Some((r.source?, r))).collect();
    for r in p.tracks.iter().flat_map(|t| t.regions.iter()) {
        let Some(s) = r.source else { continue };
        let Some(old) = was.get(&s) else { continue };
        if old.name == r.name || r.name.is_empty() {
            continue;
        }
        if r.audio.is_some() {
            let (file, idx) = audio_ref(s.audio_region);
            if let Some(i) = last(chunks, "AuRg", |c| c.id >> 16 == file && c.u32_at(12).map(|x| x >> 16) == Some(idx)) {
                let pl = current(e, chunks, i);
                if let Some(off) = audio_region_name_at(pl) {
                    if let Some(n) = set_short_string(pl, off, &r.name) {
                        e.payload.insert(i, n);
                    }
                }
            }
        } else if let Some(seq) = s.seq {
            if let Some(i) = last(chunks, "MSeq", |c| c.id == seq) {
                if let Some(n) = set_short_string(current(e, chunks, i), 16, &r.name) {
                    e.payload.insert(i, n);
                }
            }
        }
    }
}

/// An audio placement's key (events::Placement::audio_region) as (file, region number): the
/// AuFl's and AuRg's header id >> 16, and the AuRg's u32 @12 >> 16.
pub fn audio_ref(key: u32) -> (u32, u32) {
    let k = key.saturating_sub(1);
    ((k >> 16) << 2, k & 0xFFFF)
}

/// Where an AuRg's name (u16 length + ASCII) starts: the first length-prefixed printable run
/// (as the reader finds it).
fn audio_region_name_at(payload: &[u8]) -> Option<usize> {
    (0..payload.len().saturating_sub(4).min(160)).find(|&off| {
        let n = u16::from_le_bytes([payload[off], payload[off + 1]]) as usize;
        (2..=200).contains(&n) && payload.get(off + 2..off + 2 + n).is_some_and(|s| s.iter().all(|b| (0x20..0x7f).contains(b)))
    })
}

// --- audio regions' file windows -------------------------------------------------------------

/// Samples for `ticks` at the project's opening tempo.
fn ticks_to_samples(ticks: u32, tempo: f64, rate: u32) -> u32 {
    (ticks as f64 / events::PPQ as f64 * 60.0 / tempo.max(1.0) * rate as f64).round() as u32
}

fn audio_windows(p: &Project, before: &Project, _data: &[u8], chunks: &[Chunk], e: &mut Edits, _left: &mut Vec<String>) {
    let was: HashMap<crate::project::Source, &crate::project::Region> = before.tracks.iter().flat_map(|t| t.regions.iter()).filter_map(|r| Some((r.source?, r))).collect();
    for r in p.tracks.iter().flat_map(|t| t.regions.iter()).filter(|r| r.audio.is_some()) {
        let Some(s) = r.source else { continue };
        let Some(old) = was.get(&s) else { continue };
        if old.length == r.length && old.props.audio_offset == r.props.audio_offset {
            continue;
        }
        let (file, idx) = audio_ref(s.audio_region);
        let rate = last(chunks, "AuFl", |c| c.id >> 16 == file).and_then(|i| crate::project::audio_file_format(chunks[i].payload)).map_or(p.sample_rate.max(1), |f| f.1);
        let Some(i) = last(chunks, "AuRg", |c| c.id >> 16 == file && c.u32_at(12).map(|x| x >> 16) == Some(idx)) else { continue };
        let mut pl = current(e, chunks, i).to_vec();
        if pl.len() < 0x1A {
            continue;
        }
        pl[6..10].copy_from_slice(&r.props.audio_offset.to_le_bytes());
        if old.length != r.length {
            pl[0x16..0x1A].copy_from_slice(&ticks_to_samples(r.length, p.tempo, rate).to_le_bytes());
        }
        e.payload.insert(i, pl);
    }
}

// --- channel strips ------------------------------------------------------------------------

/// The AuCO chunks of the strips in use, in mixer::read's order (Mixer::strips).
fn strip_chunks(chunks: &[Chunk]) -> Vec<usize> {
    (0..chunks.len())
        .filter(|&i| {
            let c = &chunks[i];
            c.is("AuCO") && not_undo(c) && c.raw.len() > 0xC0 && c.raw[0x7A] != 0 && c.raw[0x28] != 0x46
        })
        .collect()
}

/// Writes a strip's fader, pan, mute and solo into its AuCO payload (offsets from the chunk
/// start, header included, as the research gives them).
fn set_strip(pl: &mut [u8], volume_db: f32, pan: f32, mute: bool, solo: bool) {
    let at = |off: usize| off - 36;
    pl[at(0x79)] = crate::mixer::db_to_fader(volume_db).round().clamp(0.0, 127.0) as u8;
    pl[at(0x7D)] = (64.0 + pan.clamp(-1.0, 1.0) * 63.0).round().clamp(0.0, 127.0) as u8;
    pl[at(0x7E)] = (pl[at(0x7E)] & !1) | mute as u8;
    pl[at(0x7C)] = (pl[at(0x7C)] & !1) | solo as u8;
}

fn strip_changed(a: (f32, f32, bool, bool), b: (f32, f32, bool, bool)) -> bool {
    let fv = |db: f32| crate::mixer::db_to_fader(db).round() as i32;
    let pv = |pan: f32| (64.0 + pan.clamp(-1.0, 1.0) * 63.0).round() as i32;
    fv(a.0) != fv(b.0) || pv(a.1) != pv(b.1) || a.2 != b.2 || a.3 != b.3
}

/// The group sequences (MSeq id 0 with a group mask), in order: (MSeq chunk, its event list).
fn group_chunks(chunks: &[Chunk]) -> Vec<(usize, Option<usize>)> {
    let mut out = Vec::new();
    for i in 0..chunks.len() {
        let c = &chunks[i];
        if !(c.is("MSeq") && c.id == 0 && not_undo(c)) {
            continue;
        }
        let Some(mask) = c.payload.get(0x46..0x4A) else { continue };
        let name = chunks::short_string(c.payload, 16).unwrap_or_default();
        if mask[3] & 0x80 != 0 && name != "Untitled" && name != "TRASH" {
            let ev = (i + 1..chunks.len()).take_while(|&j| !chunks[j].is("MSeq")).find(|&j| chunks[j].is("EvSq") && chunks[j].id == 0);
            out.push((i, ev));
        }
    }
    out
}

fn strips(p: &Project, before: &Project, _data: &[u8], chunks: &[Chunk], e: &mut Edits, left: &mut Vec<String>) {
    let objs = strip_chunks(chunks);
    if objs.len() != before.mixer.strips.len() {
        // (the strips aren't where they were read: write nothing rather than the wrong ones)
        if p.tracks.iter().any(|t| before_of(before, t).is_some_and(|b| strip_changed((t.volume_db, t.pan, t.mute, t.solo), (before.tracks[b].volume_db, before.tracks[b].pan, before.tracks[b].mute, before.tracks[b].solo)))) {
            left.push("channel strip settings".into());
        }
        return;
    }
    let mut done = HashSet::new();
    for t in &p.tracks {
        let Some(b) = before_of(before, t) else { continue };
        let bt = &before.tracks[b];
        if !done.insert(t.channel) || !strip_changed((t.volume_db, t.pan, t.mute, t.solo), (bt.volume_db, bt.pan, bt.mute, bt.solo)) {
            continue;
        }
        let Some(s) = before.mixer.track_strip.get(b).copied().flatten() else {
            left.push(format!("fader, pan, mute or solo of “{}” (no channel strip read for it)", t.name));
            continue;
        };
        let i = objs[s];
        let mut pl = current(e, chunks, i).to_vec();
        set_strip(&mut pl, t.volume_db, t.pan, t.mute, t.solo);
        e.payload.insert(i, pl);
    }
    // auxes (and the other strips the app keeps in p.mixer)
    for (s, st) in p.mixer.strips.iter().enumerate() {
        let Some(old) = before.mixer.strips.get(s) else { continue };
        if before.mixer.track_strip.contains(&Some(s)) || !strip_changed((st.volume_db, st.pan, st.mute, st.solo), (old.volume_db, old.pan, old.mute, old.solo)) {
            continue;
        }
        let i = objs[s];
        let mut pl = current(e, chunks, i).to_vec();
        set_strip(&mut pl, st.volume_db, st.pan, st.mute, st.solo);
        e.payload.insert(i, pl);
    }
    if p.mixer.strips.len() > before.mixer.strips.len() {
        left.push(format!("{} new channel strip(s) (auxes made here)", p.mixer.strips.len() - before.mixer.strips.len()));
    }
    if p.mixer.sends != before.mixer.sends {
        left.push("sends (their level field isn't confirmed)".into());
    }
    if p.mixer.strips.iter().zip(&before.mixer.strips).any(|(a, b)| a.output_bus != b.output_bus || a.input_bus != b.input_bus) {
        left.push("channel strip inputs / outputs".into());
    }
    // groups: membership (Envi payload 0x18) for the groups the file has
    let groups = group_chunks(chunks);
    let mut done = HashSet::new();
    for (k, t) in p.tracks.iter().enumerate() {
        let Some(b) = before_of(before, t) else { continue };
        let g = p.mixer.track_group.get(k).copied().unwrap_or(0);
        let was = before.mixer.track_group.get(b).copied().unwrap_or(0);
        if g == was || !done.insert(t.channel) {
            continue;
        }
        if g as usize > groups.len().max(before.mixer.groups.len()) {
            left.push(format!("Group {g} (a new group) on “{}”", t.name));
            continue;
        }
        if let Some(i) = last(chunks, "Envi", |c| c.id >> 16 == t.channel && c.payload.len() > 0x18) {
            let mut pl = current(e, chunks, i).to_vec();
            pl[0x18] = g;
            e.payload.insert(i, pl);
        }
    }
    // group settings
    for g in &p.mixer.groups {
        let Some(old) = before.mixer.groups.iter().find(|x| x.number == g.number) else { continue };
        let Some(&(mi, ev)) = groups.get(g.number as usize - 1) else {
            if g.mask != old.mask || g.name != old.name {
                left.push(format!("Group {}'s settings (no settings sequence for it in the file)", g.number));
            }
            continue;
        };
        if g.mask != old.mask {
            let mut pl = current(e, chunks, mi).to_vec();
            pl[0x46..0x4A].copy_from_slice(&g.mask);
            e.payload.insert(mi, pl);
            if let Some(ei) = ev {
                if let Some(n) = group_params(current(e, chunks, ei), g.mask[0]) {
                    e.payload.insert(ei, n);
                } else if (g.mask[0] ^ old.mask[0]) & 0x0F != 0 {
                    left.push(format!("Group {}'s linked-parameter list", g.number));
                }
            }
        }
        // (a group's name isn't decoded: the settings MSeq's own name field is empty in E24 and
        // the mask sits at a fixed offset after it)
        if g.name != old.name {
            left.push(format!("Group {}'s name", g.number));
        }
    }
}

/// A group's event list with one 0x50 record per linked parameter (§23: 7 Volume 90, 10 Pan 64,
/// 9 Mute 0, 3 Solo 0), modelled on a 0x50 record already there; None without a model.
fn group_params(payload: &[u8], low: u8) -> Option<Vec<u8>> {
    let recs = events::records(payload);
    let model = recs.iter().find(|r| r.kind & 0xF0 == 0x50)?.bytes.clone();
    let want: Vec<(u8, u8)> = [(0x01, 7u8, 90u8), (0x04, 9, 0), (0x02, 10, 64), (0x08, 3, 0)].iter().filter(|(bit, _, _)| low & bit != 0).map(|&(_, p, v)| (p, v)).collect();
    let mut out: Vec<u8> = Vec::new();
    for r in &recs {
        if r.kind & 0xF0 == 0x50 {
            if want.iter().any(|w| w.0 == r.u8_at(12)) {
                out.extend(&r.bytes);
            }
        } else {
            out.extend(&r.bytes);
        }
    }
    for (param, value) in &want {
        if !recs.iter().any(|r| r.kind & 0xF0 == 0x50 && r.u8_at(12) == *param) {
            let mut b = model.clone();
            b[11] = *value;
            b[12] = *param;
            out.extend(b);
        }
    }
    out.extend(end_unit(payload));
    Some(out)
}

/// An event list's end record (its own, or a fresh one).
fn end_unit(payload: &[u8]) -> Vec<u8> {
    payload.chunks_exact(16).rev().find(|u| u[0] == 0xF1).map(|u| u.to_vec()).unwrap_or_else(|| {
        let mut u = vec![0u8; 16];
        u[0] = 0xF1;
        u[4..8].copy_from_slice(&events::END.to_le_bytes());
        u
    })
}

// --- plug-ins ------------------------------------------------------------------------------

fn plugins(p: &Project, before: &Project, _data: &[u8], chunks: &[Chunk], e: &mut Edits, left: &mut Vec<String>) {
    use crate::plugin::Plugin;
    // (now, as read, where) for every strip Studio shows
    let mut lists: Vec<(&[Plugin], &[Plugin], String)> = Vec::new();
    let mut done = HashSet::new();
    for t in &p.tracks {
        let Some(b) = before_of(before, t) else { continue };
        if !done.insert(t.channel) {
            continue;
        }
        let bt = &before.tracks[b];
        let strip = before.mixer.track_strip.get(b).copied().flatten().map(|s| before.mixer.strips[s].plugins.as_slice()).unwrap_or(&[]);
        let was: &[Plugin] = if bt.plugins.is_empty() { strip } else { &bt.plugins };
        // (an audio track whose effects the app didn't take in: nothing to compare)
        if t.plugins.is_empty() && bt.plugins.is_empty() {
            continue;
        }
        lists.push((&t.plugins, was, t.name.clone()));
    }
    for (s, st) in p.mixer.strips.iter().enumerate() {
        if before.mixer.track_strip.contains(&Some(s)) {
            continue;
        }
        if let Some(old) = before.mixer.strips.get(s) {
            lists.push((&st.plugins, &old.plugins, st.name.clone()));
        }
    }
    for (now, was, name) in lists {
        for u in now {
            let Some(w) = was.iter().find(|w| w.slot == u.slot && w.name == u.name && (u.channel == 0 || w.channel == u.channel)) else {
                left.push(format!("plug-in {} on “{name}” (added or moved)", u.name));
                continue;
            };
            if w.bypass != u.bypass {
                left.push(format!("bypass of {} on “{name}”", u.name));
            }
            if w.params == u.params {
                continue;
            }
            if w.params.len() != u.params.len() {
                left.push(format!("{} settings on “{name}” (parameter count changed)", u.name));
                continue;
            }
            let found = last(chunks, "AuCU", |c| c.u32_at(12).map(|x| x >> 16) == Some(w.channel) && c.u32_at(16).map(|x| x >> 16) == Some(w.slot) && crate::plugin::read_unit(c.payload).is_some_and(|r| r.name == w.name));
            let Some(i) = found else {
                left.push(format!("{} settings on “{name}”", u.name));
                continue;
            };
            let mut pl = current(e, chunks, i).to_vec();
            let Some(at) = pl.windows(8).position(|x| x == b"GAMETSPP") else {
                left.push(format!("{} settings on “{name}” (no parameter block)", u.name));
                continue;
            };
            for (k, v) in u.params.iter().enumerate() {
                let o = at + 16 + 4 * k;
                if o + 4 <= pl.len() {
                    pl[o..o + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
            e.payload.insert(i, pl);
        }
        for w in was {
            if !now.iter().any(|u| u.slot == w.slot && u.name == w.name) && !now.is_empty() {
                left.push(format!("removing {} from “{name}”", w.name));
            }
        }
    }
}

// --- signature, key ------------------------------------------------------------------------

fn signature_and_key(p: &Project, before: &Project, _data: &[u8], chunks: &[Chunk], e: &mut Edits, left: &mut Vec<String>) {
    if !p.global.signatures.is_empty() || !p.global.keys.is_empty() {
        left.push("time / key signature changes after bar 1".into());
    }
    if p.global.tempo_sets.len() > 1 {
        left.push("tempo sets other than the current one".into());
    }
    if p.tracks.iter().any(|t| !t.extra.midi_fx.is_empty()) {
        left.push("MIDI FX slots".into());
    }
    if p.tracks.iter().flat_map(|t| t.regions.iter()).any(|r| r.ext.alias_of != 0) {
        left.push("aliases (saved as copies)".into());
    }
    let sig_changed = p.signature != before.signature;
    let key_changed = events::key_byte(&p.key) != events::key_byte(&before.key);
    if !sig_changed && !key_changed {
        return;
    }
    let Some(i) = (0..chunks.len()).find(|&i| chunks[i].is("EvSq") && chunks[i].id == 0 && chunks[i].index == 1 && not_undo(&chunks[i])) else {
        left.push("time signature / key (no signature list)".into());
        return;
    };
    let pl = current(e, chunks, i).to_vec();
    let recs = events::records(&pl);
    let mut out = Vec::new();
    let (mut sig_done, mut key_done) = (false, false);
    for r in recs {
        let mut b = r.bytes.clone();
        if r.kind == 0x30 && !sig_done {
            sig_done = true;
            let (n, d) = p.signature;
            b[11] = (d.max(1) as u32).trailing_zeros() as u8;
            b[12] = n;
            if b.len() >= 32 {
                // the bar number at the song's start (E03: −10 in 4/4, −13 in 3/4)
                let bars = -((EVENT_BAR_ONE / crate::project::bar_ticks(p.signature).max(1)) as i16);
                b[24..26].copy_from_slice(&bars.to_le_bytes());
            }
        }
        if r.kind == 0x32 && !key_done {
            key_done = true;
            match events::key_byte(&p.key) {
                Some(k) if key_changed => b[12] = k,
                None if key_changed => left.push(format!("key “{}”", p.key)),
                _ => {}
            }
        }
        out.extend(b);
    }
    out.extend(end_unit(&pl));
    e.payload.insert(i, out);
    if sig_changed {
        if let Some(s) = (0..chunks.len()).find(|&i| chunks[i].is("Song")) {
            let mut sp = current(e, chunks, s).to_vec();
            if sp.len() > 0x39E - 36 {
                sp[0x39E - 36] = p.signature.0;
                e.payload.insert(s, sp);
            }
        }
    }
}

// --- tempo changes -------------------------------------------------------------------------

/// Seconds from bar 1 to `tick` along a tempo map.
fn seconds_at(map: &[(u32, f64)], tick: u32) -> f64 {
    let mut secs = 0.0;
    for (k, &(at, bpm)) in map.iter().enumerate() {
        if at >= tick {
            break;
        }
        let end = map.get(k + 1).map_or(tick, |n| n.0.min(tick));
        secs += (end - at) as f64 / events::PPQ as f64 * 60.0 / bpm.max(1.0);
    }
    secs
}

fn tempo_list(p: &Project, before: &Project, _data: &[u8], chunks: &[Chunk], e: &mut Edits, left: &mut Vec<String>) {
    let norm = |m: &[(u32, f64)]| m.iter().map(|&(t, b)| (t, (b * 10000.0).round() as u32)).collect::<Vec<_>>();
    // tempo curves go in as the tempo events they stand for (1/16 apart), as Logic keeps them
    let mut now: Vec<(u32, f64)> = if p.global.tempo_curves.is_empty() { p.tempo_map.clone() } else { crate::timeline::Map::of(p).expanded(240).into_iter().map(|x| (x.0, x.1)).collect() };
    if now.is_empty() {
        now.push((0, p.tempo));
    }
    now[0] = (0, p.tempo);
    let mut was = before.tempo_map.clone();
    if was.is_empty() {
        was.push((0, before.tempo));
    }
    was[0] = (0, before.tempo);
    // (a single tempo is written by save.rs)
    if norm(&now) == norm(&was) || (now.len() == 1 && was.len() == 1) {
        return;
    }
    let Some(i) = (0..chunks.len()).find(|&i| chunks[i].is("EvSq") && chunks[i].id == 0 && chunks[i].index == 3 && not_undo(&chunks[i])) else {
        left.push("tempo changes (no tempo list)".into());
        return;
    };
    let pl = current(e, chunks, i);
    let recs = events::records(pl);
    let tempos: Vec<&events::Record> = recs.iter().filter(|r| r.kind == 0x60 && r.units() >= 2).collect();
    let Some(first) = tempos.first() else {
        left.push("tempo changes".into());
        return;
    };
    let later = tempos.get(1).copied().unwrap_or(first);
    // the trailer units after the last tempo event's own two (§11)
    let trailer: Vec<u8> = tempos.last().map(|r| r.bytes[32..].to_vec()).unwrap_or_default();
    let mut out: Vec<u8> = recs.iter().filter(|r| r.kind != 0x60).flat_map(|r| r.bytes.clone()).collect();
    for (k, &(tick, bpm)) in now.iter().enumerate() {
        let model = if k == 0 { first } else { later };
        let mut b = model.bytes[..32].to_vec();
        let pos = if k == 0 { first.pos } else { tick + EVENT_BAR_ONE };
        let pb = pos.to_le_bytes();
        b[4..7].copy_from_slice(&pb[..3]);
        b[7] = pb[3] & 0x7F;
        b[16..20].copy_from_slice(&((bpm * 10000.0).round() as u32).to_le_bytes());
        // the time cached with each event: half-milliseconds, bar 1 = 01:00:00:00
        let cache = ((3600.0 + seconds_at(&now, tick)) * 2000.0).round() as u32;
        b[24..28].copy_from_slice(&cache.to_le_bytes());
        out.extend(b);
        if k + 1 == now.len() {
            out.extend(&trailer);
        }
    }
    out.extend(end_unit(pl));
    e.payload.insert(i, out);
}

// --- markers -------------------------------------------------------------------------------

/// A TxSq text chunk's text: plain (kind 0x14) as it is, RTF (0x13) reduced to its text
/// ("##" is Logic's auto-number, left for the caller), empty (0x11) "".
pub fn txsq_text(payload: &[u8]) -> String {
    let at = payload.get(16..20).map_or(0x62, |b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
    let raw = payload.get(at..).unwrap_or(&[]);
    let raw = raw.split(|b| *b == 0).next().unwrap_or(&[]);
    let s = String::from_utf8_lossy(raw).into_owned();
    if s.starts_with("{\\rtf") { rtf_plain(&s) } else { s }
}

/// The visible text of a small RTF string (Logic's marker names).
fn rtf_plain(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    let mut skip_depth: Option<i32> = None;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                depth += 1;
                // a destination group: {\fonttbl …}, {\colortbl …}, {\*…}
                let rest: String = chars.clone().take(10).collect();
                if skip_depth.is_none() && (rest.starts_with("\\fonttbl") || rest.starts_with("\\colortbl") || rest.starts_with("\\*") || rest.starts_with("\\expanded")) {
                    skip_depth = Some(depth);
                }
            }
            '}' => {
                if skip_depth == Some(depth) {
                    skip_depth = None;
                }
                depth -= 1;
            }
            '\\' => {
                let mut word = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_alphabetic() {
                        word.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if word.is_empty() {
                    if let Some(n) = chars.next() {
                        if skip_depth.is_none() && (n == '\\' || n == '{' || n == '}') {
                            out.push(n);
                        }
                    }
                    continue;
                }
                while chars.peek().is_some_and(|n| n.is_ascii_digit() || *n == '-') {
                    chars.next();
                }
                if chars.peek() == Some(&' ') {
                    chars.next();
                }
                if skip_depth.is_none() && word == "par" {
                    out.push('\n');
                }
            }
            '\n' | '\r' => {}
            c => {
                if skip_depth.is_none() && depth > 0 {
                    out.push(c);
                }
            }
        }
    }
    out.trim().to_string()
}

/// A TxSq payload holding `text` as plain text (§10: total length, 12 zero bytes, the text's
/// offset 0x62, the length again, kind 0x14 with 0x01 in its top byte, the "\x1b\x1b//RR"
/// mark, zeros to the offset, the text and a NUL).
pub fn txsq_payload(text: &str) -> Vec<u8> {
    let len = 0x62 + text.len() + 1;
    let mut b = vec![0u8; 0x62];
    b[0..4].copy_from_slice(&(len as u32).to_le_bytes());
    b[16..20].copy_from_slice(&0x62u32.to_le_bytes());
    b[20..24].copy_from_slice(&(len as u32).to_le_bytes());
    b[24..28].copy_from_slice(&[0x14, 0, 0, 1]);
    b[28..34].copy_from_slice(b"\x1b\x1b//RR");
    b.extend_from_slice(text.as_bytes());
    b.push(0);
    b
}

/// A whole TxSq chunk (header as Logic writes it: version 1, index 0x20).
fn txsq_chunk(id: u32, text: &str) -> Vec<u8> {
    let pl = txsq_payload(text);
    let mut h = vec![0u8; 36];
    h[..4].copy_from_slice(b"qSxT");
    h[4..6].copy_from_slice(&1u16.to_le_bytes());
    h[6..8].copy_from_slice(&0x20u16.to_le_bytes());
    h[8..12].copy_from_slice(&id.to_le_bytes());
    h[14..22].copy_from_slice(&[0xFF; 8]);
    h[22] = 2;
    h[26] = 2;
    h[28..36].copy_from_slice(&(pl.len() as u64).to_le_bytes());
    h.extend(pl);
    h
}

/// A 3-unit marker record (§10): position, the text's link (TxSq id >> 16), its number in the
/// list and its length in ticks (1 for a marker: up to the next one).
fn marker_record(pos: u32, link: u32, index: u32, length: u32) -> Vec<u8> {
    let mut b = vec![0u8; 48];
    b[0] = 0x12;
    let pb = pos.to_le_bytes();
    b[4..7].copy_from_slice(&pb[..3]);
    b[7] = pb[3] & 0x7F;
    b[16..20].copy_from_slice(&link.to_le_bytes());
    b[23] = 0x88;
    b[24..28].copy_from_slice(&index.to_le_bytes());
    b[28..32].copy_from_slice(&length.to_le_bytes());
    b[39] = 0x88;
    b
}

/// The marker lists and their texts, as the reader gives them to a project.
pub fn read_markers(live: &[&Chunk], p: &mut Project) {
    let texts: HashMap<u32, String> = live.iter().filter(|c| c.is("TxSq")).map(|c| (c.id >> 16, txsq_text(c.payload))).collect();
    let list = |want: &dyn Fn(&Chunk) -> bool| -> Vec<(u32, u32, String)> {
        let Some(c) = live.iter().rev().find(|c| c.is("EvSq") && want(c)) else { return Vec::new() };
        let mut out: Vec<(u32, u32, String)> = events::records(c.payload).iter().filter(|r| r.kind == 0x12 && r.units() >= 2).map(|r| (r.pos.saturating_sub(EVENT_BAR_ONE), r.u32_at(28), texts.get(&r.u32_at(16)).cloned().unwrap_or_default())).collect();
        out.sort_by_key(|m| m.0);
        out
    };
    p.markers = list(&|c: &Chunk| c.id == 0x40000 && c.index == 22).into_iter().enumerate().map(|(k, (pos, _, name))| (pos, name.replace("##", &(k + 1).to_string()))).collect();
    p.arrangement = list(&|c: &Chunk| c.id == 0 && c.index == 5);
}

fn markers(p: &Project, before: &Project, _data: &[u8], chunks: &[Chunk], e: &mut Edits, left: &mut Vec<String>) {
    let now_m: Vec<(u32, u32, String)> = p.markers.iter().map(|(pos, n)| (*pos, 1, n.clone())).collect();
    let was_m: Vec<(u32, u32, String)> = before.markers.iter().map(|(pos, n)| (*pos, 1, n.clone())).collect();
    let mut next_id = chunks.iter().filter(|c| c.is("TxSq") && not_undo(c)).map(|c| c.id).max().map_or(0x40000, |m| m + 0x40000);
    let mut new_texts: Vec<Vec<u8>> = Vec::new();
    for (now, was, find, what) in [
        (&now_m, &was_m, &(|c: &Chunk| c.id == 0x40000 && c.index == 22) as &dyn Fn(&Chunk) -> bool, "markers"),
        (&p.arrangement, &before.arrangement, &(|c: &Chunk| c.id == 0 && c.index == 5) as &dyn Fn(&Chunk) -> bool, "arrangement markers"),
    ] {
        if now == was {
            continue;
        }
        let Some(i) = (0..chunks.len()).rev().find(|&i| chunks[i].is("EvSq") && not_undo(&chunks[i]) && find(&chunks[i])) else {
            left.push(format!("{what} (this project has no list for them yet)"));
            continue;
        };
        let pl = current(e, chunks, i).to_vec();
        let recs = events::records(&pl);
        // the records as read, in position order, with their links
        let mut old: Vec<(u32, u32, bool)> = recs.iter().filter(|r| r.kind == 0x12 && r.units() >= 2).map(|r| (r.pos.saturating_sub(EVENT_BAR_ONE), r.u32_at(16), false)).collect();
        old.sort_by_key(|o| o.0);
        let mut out: Vec<u8> = recs.iter().filter(|r| r.kind != 0x12).flat_map(|r| r.bytes.clone()).collect();
        let mut sorted = now.clone();
        sorted.sort_by_key(|m| m.0);
        for (k, (pos, len, name)) in sorted.iter().enumerate() {
            // the same marker as read (same name, the same or a moved position), else one renamed
            // at the same position, else a new text
            let same = (0..old.len()).filter(|&j| !old[j].2 && was.get(j).is_some_and(|w| &w.2 == name)).min_by_key(|&j| (old[j].0 as i64 - *pos as i64).abs());
            let renamed = || (0..old.len()).find(|&j| !old[j].2 && old[j].0 == *pos);
            let link = if let Some(j) = same {
                old[j].2 = true;
                old[j].1
            } else if let Some(j) = renamed() {
                old[j].2 = true;
                if let Some(t) = last(chunks, "TxSq", |c| c.id >> 16 == old[j].1) {
                    e.payload.insert(t, txsq_payload(name));
                }
                old[j].1
            } else {
                let id = next_id;
                next_id += 0x40000;
                new_texts.push(txsq_chunk(id, name));
                id >> 16
            };
            out.extend(marker_record(pos + EVENT_BAR_ONE, link, k as u32, (*len).max(1)));
        }
        out.extend(end_unit(&pl));
        e.payload.insert(i, out);
    }
    if !new_texts.is_empty() {
        // after the last text chunk, else after the last GenM, else before the undo history
        let at = (0..chunks.len()).rev().find(|&i| chunks[i].is("TxSq") && not_undo(&chunks[i])).or_else(|| (0..chunks.len()).rev().find(|&i| chunks[i].is("GenM"))).unwrap_or_else(|| (0..chunks.len()).find(|&i| !not_undo(&chunks[i]) || chunks[i].is("Grid")).unwrap_or(chunks.len()).saturating_sub(1));
        e.after.entry(at).or_default().extend(new_texts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_chunks_both_ways() {
        let pl = txsq_payload("Verse");
        assert_eq!(pl.len(), 0x68, "as Logic's \"Verse\" (E23)");
        assert_eq!(txsq_text(&pl), "Verse");
        let rtf = "{\\rtf1\\ansi\\ansicpg1252\\cocoartf2907\n\\cocoatextscaling0\\cocoaplatform0{\\fonttbl\\f0\\fnil\\fcharset0 HelveticaNeue;}\n{\\colortbl;\\red255\\green255\\blue255;}\n\\pard\\f0\\fs24 \\cf2 Marker ##}";
        assert_eq!(rtf_plain(rtf), "Marker ##");
    }

    #[test]
    fn names_are_replaced_in_place() {
        let mut pl = vec![9u8; 16];
        pl.extend_from_slice(&7u16.to_le_bytes());
        pl.extend_from_slice(b"Audio 1");
        pl.extend_from_slice(&[1, 2, 3]);
        let out = set_short_string(&pl, 16, "TrackNameTestQ").unwrap();
        assert_eq!(chunks::short_string(&out, 16).as_deref(), Some("TrackNameTestQ"));
        assert_eq!(&out[out.len() - 3..], &[1, 2, 3]);
    }

    #[test]
    fn tempo_cache_counts_half_milliseconds_from_one_hour() {
        // E23: 120 BPM, a change at bar 7 cached 7,224,000
        let map = [(0u32, 120.0), (23040, 127.0)];
        assert_eq!(((3600.0 + seconds_at(&map, 0)) * 2000.0) as u32, 7_200_000);
        assert_eq!(((3600.0 + seconds_at(&map, 23040)) * 2000.0).round() as u32, 7_224_000);
    }

    /// The research's sample projects (this checkout's testdata-private, or the main checkout's).
    fn samples_dir() -> Option<std::path::PathBuf> {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata-private/sample-files");
        let main = std::path::PathBuf::from(std::env::var("HOME").ok()?).join("CascadeProjects/bramble-office/logic-linux/testdata-private/sample-files");
        [here, main].into_iter().find(|d| d.is_dir())
    }

    fn all_project_data(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                all_project_data(&p, out);
            } else if p.file_name().is_some_and(|n| n == "ProjectData") {
                out.push(p);
            }
        }
    }

    fn sample(name: &str, alt: &str) -> Option<Vec<u8>> {
        std::fs::read(samples_dir()?.join(name).join("Alternatives").join(alt).join("ProjectData")).ok()
    }

    fn resave(p: &Project, orig: &[u8]) -> (Vec<u8>, Project, crate::save::Saved) {
        let saved = crate::save::save(p, orig).unwrap();
        let back = crate::project::read_project_data(&saved.data).unwrap();
        (saved.data.clone(), back, saved)
    }

    /// Every sample ProjectData, read and saved unchanged, comes back byte for byte.
    #[test]
    fn every_sample_round_trips_unchanged() {
        let Some(dir) = samples_dir() else { return };
        let mut files = Vec::new();
        all_project_data(&dir, &mut files);
        assert!(files.len() > 40, "{}", files.len());
        for f in files {
            let orig = std::fs::read(&f).unwrap();
            let Ok(p) = crate::project::read_project_data(&orig) else { continue };
            let saved = crate::save::save(&p, &orig).unwrap();
            assert!(saved.data == orig, "{} changed when saved unchanged", f.display());
            assert!(saved.left_out.is_empty(), "{}: {:?}", f.display(), saved.left_out);
            // and opened as the app opens it (MetaData, audio files)
            let pkg = f.parent().and_then(|a| a.parent()).and_then(|a| a.parent()).unwrap();
            if pkg.extension().is_some_and(|e| e == "logicx") && f.parent().unwrap().parent().unwrap().ends_with("Alternatives") {
                if let Ok(q) = crate::project::open(pkg) {
                    if crate::package::active(pkg).is_some_and(|a| a.dir == f.parent().unwrap()) {
                        let s2 = crate::save::save(&q, &orig).unwrap();
                        assert!(s2.data == orig, "{} (opened) changed when saved unchanged: {:?}", f.display(), s2.left_out);
                    }
                }
            }
        }
    }

    /// E23: names, Region inspector values, the mixer, signature, key, tempo changes and
    /// markers edited, saved, read again.
    #[test]
    fn e23_edits_come_back() {
        let Some(orig) = sample("E23-live-loops-base.logicx", "000") else { return };
        let p = crate::project::read_project_data(&orig).unwrap();
        // what the reader finds there (format-analysis §10-15)
        assert_eq!(p.markers.len(), 1, "{:?}", p.markers);
        assert_eq!(p.markers[0], (0, "Marker 1".to_string()));
        assert_eq!(p.arrangement.iter().map(|a| a.2.as_str()).collect::<Vec<_>>(), ["Intro", "Verse", "Chorus"]);
        assert_eq!(p.arrangement[1], (30720, 30720, "Verse".to_string()));
        assert_eq!(p.tempo_map, vec![(0, 120.0), (23040, 127.0)], "a change at bar 7");
        let t = p.tracks.iter().position(|t| t.name == "Deluxe Classic").expect("track");
        let r = p.tracks[t].regions.iter().position(|r| r.name == "RenamedRegionXY").expect("region");
        let pr = p.tracks[t].regions[r].props;
        assert!(pr.looped && pr.velocity == 10 && pr.transpose == 3, "{pr:?}");
        assert_eq!(p.signature, (4, 4));
        assert_eq!(p.key, "C major");

        let mut e = p.clone();
        e.tracks[t].name = "Keys Renamed".into();
        e.tracks[t].regions[r].name = "Chorus Keys".into();
        e.tracks[t].regions[r].props.looped = false;
        e.tracks[t].regions[r].props.velocity = -5;
        e.tracks[t].regions[r].props.transpose = -12;
        // (the fader on a track whose strip the reader matched)
        let m = (0..p.tracks.len()).find(|&i| p.mixer.track_strip[i].is_some()).expect("a track with a strip");
        e.tracks[m].volume_db = crate::mixer::fader_to_db(70.0);
        e.tracks[m].pan = (80.0 - 64.0) / 63.0;
        e.tracks[m].mute = !e.tracks[m].mute;
        e.tracks[m].solo = true;
        if let Some(u) = e.tracks[t].plugins.iter_mut().find(|u| !u.params.is_empty()) {
            u.params[0] += 1.5;
        }
        e.signature = (3, 4);
        e.key = "F minor".into();
        e.tempo_map = vec![(0, 120.0), (23040, 127.0), (46080, 90.5)];
        e.markers = vec![(0, "Top".into()), (7680, "Bridge".into())];
        e.arrangement[1].2 = "Verse 1".into();
        e.arrangement.push((92160, 15360, "Outro".into()));
        let (_, back, saved) = resave(&e, &orig);
        assert!(saved.left_out.is_empty(), "{:?}", saved.left_out);
        let bt = back.tracks.iter().position(|x| x.channel == e.tracks[t].channel).unwrap();
        assert_eq!(back.tracks[bt].name, "Keys Renamed");
        let br = back.tracks[bt].regions.iter().find(|x| x.source.map(|s| s.seq) == e.tracks[t].regions[r].source.map(|s| s.seq)).unwrap();
        assert_eq!(br.name, "Chorus Keys");
        assert_eq!((br.props.looped, br.props.velocity, br.props.transpose), (false, -5, -12));
        assert_eq!(br.notes, e.tracks[t].regions[r].notes);
        assert_eq!(back.tracks[bt].plugins, e.tracks[t].plugins);
        let bm = back.tracks.iter().position(|x| x.channel == e.tracks[m].channel).unwrap();
        let (a, b) = (&back.tracks[bm], &e.tracks[m]);
        assert_eq!((crate::mixer::db_to_fader(a.volume_db).round(), (a.pan * 63.0).round(), a.mute, a.solo), (70.0, 16.0, b.mute, true));
        assert_eq!(back.signature, (3, 4));
        assert_eq!(back.key, "F minor");
        assert_eq!(back.tempo_map, e.tempo_map);
        assert_eq!(back.markers, e.markers);
        assert_eq!(back.arrangement, e.arrangement);
        // the rest as it was
        assert_eq!(back.tracks.len(), p.tracks.len());
        for (x, y) in back.tracks.iter().zip(&p.tracks) {
            assert_eq!(x.regions.len(), y.regions.len(), "{}", y.name);
        }
        // saved again unchanged: the same bytes
        let again = crate::save::save(&back, &saved.data).unwrap();
        assert!(again.data == saved.data && again.left_out.is_empty());
    }

    /// E24: group membership and settings; mute.
    #[test]
    fn e24_groups_come_back() {
        let Some(orig) = sample("E24-groups.logicx", "000") else { return };
        let p = crate::project::read_project_data(&orig).unwrap();
        let mut e = p.clone();
        let (a, b) = (e.mixer.track_group.iter().position(|g| *g == 1).unwrap(), e.mixer.track_group.iter().position(|g| *g == 2).unwrap());
        // (every track on the channel: a take folder's rows share it)
        let (ca, cb) = (e.tracks[a].channel, e.tracks[b].channel);
        for i in 0..e.tracks.len() {
            if e.tracks[i].channel == ca {
                e.mixer.track_group[i] = 2;
            } else if e.tracks[i].channel == cb {
                e.mixer.track_group[i] = 0;
            }
        }
        e.mixer.groups[0].mask = [0x07, 0x01, 0x40, 0x81];

        let (_, back, saved) = resave(&e, &orig);
        assert!(saved.left_out.is_empty(), "{:?}", saved.left_out);
        assert_eq!(back.mixer.track_group, e.mixer.track_group);
        assert_eq!(back.mixer.groups[0].mask, [0x07, 0x01, 0x40, 0x81]);

        // Group 2 has no settings sequence: its settings are named, not written
        let mut g2 = p.clone();
        g2.mixer.groups[1].mask[0] |= 0x02;
        assert!(crate::save::save(&g2, &orig).unwrap().left_out.iter().any(|l| l.contains("Group 2")));
        // a new group is named, not written
        let mut n = p.clone();
        n.mixer.track_group[b] = 3;
        assert!(crate::save::save(&n, &orig).unwrap().left_out.iter().any(|l| l.contains("Group 3")));
    }

    /// R01: an audio region's fades, file window and name; a new region on the same file and
    /// one on a new (recorded) file.
    #[test]
    fn r01_audio_regions_come_back() {
        let Some(orig) = sample("R01-render-Compressor-Opto.logicx", "000") else { return };
        let p = crate::project::read_project_data(&orig).unwrap();
        let t = p.tracks.iter().position(|t| t.regions.iter().any(|r| r.audio.is_some())).unwrap();
        let r0 = p.tracks[t].regions.iter().position(|r| r.audio.is_some()).unwrap();
        let rg = &p.tracks[t].regions[r0];
        assert_eq!(rg.name, "composite-40s", "the placement names its own file and region");
        assert_eq!((rg.props.fade_in, rg.props.fade_out), ((1500, 50), (800, -30)), "§8");
        let mut e = p.clone();
        {
            let rg = &mut e.tracks[t].regions[r0];
            rg.props.fade_in = (250, -10);
            rg.props.fade_out = (0, 0);
            rg.props.audio_offset = 48000;
            rg.length -= 1920;
            rg.name = "composite trimmed".into();
        }
        let mut copy = e.tracks[t].regions[r0].clone();
        copy.source = None;
        copy.start += 3840 * 20;
        copy.name = "composite copy".into();
        e.tracks[t].regions.push(copy.clone());
        // a recording: a new WAV beside the test's
        let dir = std::env::temp_dir().join(format!("lg-writeback-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("Audio 1#01.wav");
        let mut w = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        w.extend(16u32.to_le_bytes());
        w.extend([1, 0, 1, 0]);
        w.extend(48000u32.to_le_bytes());
        w.extend(96000u32.to_le_bytes());
        w.extend([2, 0, 16, 0]);
        w.extend(b"data");
        w.extend(9600u32.to_le_bytes());
        w.extend(vec![0u8; 9600]);
        std::fs::write(&wav, &w).unwrap();
        let mut rec = copy.clone();
        rec.name = "Audio 1#01".into();
        rec.start = 3840 * 30;
        rec.length = 192;
        rec.props = Default::default();
        rec.audio = Some(crate::project::AudioRef { file: wav.clone(), seconds: 0.1 });
        e.tracks[t].regions.push(rec);
        let (_, back, saved) = resave(&e, &orig);
        assert!(saved.unsaved.is_empty() && saved.left_out.is_empty(), "{:?} {:?}", saved.unsaved, saved.left_out);
        let bt = &back.tracks[back.tracks.iter().position(|x| x.channel == e.tracks[t].channel).unwrap()];
        let got = |n: &str| bt.regions.iter().find(|r| r.name == n).unwrap_or_else(|| panic!("{n}: {:?}", bt.regions.iter().map(|r| &r.name).collect::<Vec<_>>()));
        let a = got("composite trimmed");
        assert_eq!((a.props.fade_in, a.props.fade_out, a.props.audio_offset), ((250, -10), (0, 0), 48000));
        assert!((a.length as i64 - e.tracks[t].regions[r0].length as i64).abs() <= 1);
        let c = got("composite copy");
        assert_eq!((c.start, c.props.fade_in, c.props.audio_offset), (copy.start, (250, -10), 48000));
        assert!(c.audio.as_ref().unwrap().file.ends_with("composite-40s.wav"));
        let n = got("Audio 1#01");
        assert_eq!(n.start, 3840 * 30);
        assert!(n.audio.as_ref().unwrap().file.ends_with("Audio 1#01.wav"));
        assert!((n.audio.as_ref().unwrap().seconds - 0.1).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// E02: a new MIDI region drawn on the track (no copy to model it on) and one on a track
    /// made here (listed).
    #[test]
    fn e02_new_midi_regions() {
        let Some(orig) = sample("E02-4-notes.logicx", "000") else { return };
        let p = crate::project::read_project_data(&orig).unwrap();
        let t = p.tracks.iter().position(|t| !t.regions.is_empty()).unwrap();
        let mut e = p.clone();
        let mut r = e.tracks[t].regions[0].clone();
        r.source = None;
        r.origin = None;
        r.name = "Drawn".into();
        r.start = 3840 * 8;
        r.length = 3840;
        r.notes = vec![events::Note { channel: 0, pitch: 72, velocity: 100, pos: 3840 * 8 + 480, len: 480, muted: false }];
        r.props.transpose = 2;
        e.tracks[t].regions.push(r.clone());
        let mut nt = e.tracks[t].clone();
        nt.channel = 0;
        nt.name = "New Keys".into();
        nt.regions = vec![r.clone()];
        e.tracks.push(nt);
        let (_, back, saved) = resave(&e, &orig);
        assert_eq!(saved.unsaved, vec!["Drawn".to_string()], "the new track's region");
        assert!(saved.left_out.iter().any(|l| l.contains("New Keys")), "{:?}", saved.left_out);
        let bt = &back.tracks[t];
        let d = bt.regions.iter().find(|x| x.name == "Drawn").expect("the drawn region");
        assert_eq!((d.start, d.props.transpose), (3840 * 8, 2));
        assert_eq!(d.notes, r.notes, "only its own notes");
        assert_eq!(bt.regions.len(), p.tracks[t].regions.len() + 1);
    }
}
