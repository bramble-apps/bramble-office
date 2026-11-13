//! Logic's editing commands (key commands and menu items by their Logic names), for the Tracks
//! area and, where the name says so, the Piano Roll's notes: selection, split and join, nudge,
//! transpose, trims, quantize, mute and loop, tracks, locators and navigation, zoom, tools.
//! `App::key_command` passes every name it doesn't run itself here; the build lists the names
//! matched below, so the menus enable them.

use lg_core::events::Note;
use lg_core::TrackKind;

use crate::app::{App, Bottom, RightPane};

/// The Tracks area's tools (T menu; Logic's names).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pointer,
    Pencil,
    Eraser,
    Text,
    Scissors,
    Glue,
    Solo,
    Mute,
    Zoom,
    Fade,
    Automation,
    Marquee,
    Flex,
    Velocity,
    Quantize,
    Brush,
    /// The Finger tool (Piano Roll, Step Editor): drag anywhere on a note to change its length.
    Finger,
    /// The Gain tool: drag an audio region up or down for its gain (regionparams.rs).
    Gain,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Pointer => "Pointer Tool",
            Tool::Pencil => "Pencil Tool",
            Tool::Eraser => "Eraser Tool",
            Tool::Text => "Text Tool",
            Tool::Scissors => "Scissors Tool",
            Tool::Glue => "Join Tool",
            Tool::Solo => "Solo Tool",
            Tool::Mute => "Mute Tool",
            Tool::Zoom => "Zoom Tool",
            Tool::Fade => "Fade Tool",
            Tool::Automation => "Automation Select Tool",
            Tool::Marquee => "Marquee Tool",
            Tool::Flex => "Flex Tool",
            Tool::Velocity => "Velocity Tool",
            Tool::Quantize => "Quantize Tool",
            Tool::Brush => "Brush Tool",
            Tool::Finger => "Finger Tool",
            Tool::Gain => "Gain Tool",
        }
    }

    pub const TRACKS: [Tool; 14] = [Tool::Pointer, Tool::Pencil, Tool::Eraser, Tool::Text, Tool::Scissors, Tool::Glue, Tool::Solo, Tool::Mute, Tool::Zoom, Tool::Fade, Tool::Automation, Tool::Marquee, Tool::Flex, Tool::Gain];
    /// (in the order of Logic's Piano Roll Tool menu; Studio's Solo and Marquee where the
    /// Tracks area's menu has them)
    pub const PIANO_ROLL: [Tool; 14] = [Tool::Pointer, Tool::Pencil, Tool::Eraser, Tool::Finger, Tool::Scissors, Tool::Glue, Tool::Solo, Tool::Mute, Tool::Quantize, Tool::Velocity, Tool::Zoom, Tool::Automation, Tool::Marquee, Tool::Brush];

    pub fn key(self) -> &'static str {
        match self {
            Tool::Pointer => "T",
            Tool::Pencil => "P",
            Tool::Eraser => "E",
            Tool::Text => "⇧T",
            Tool::Scissors => "I",
            Tool::Glue => "J",
            Tool::Solo => "S",
            Tool::Mute => "M",
            Tool::Zoom => "Y",
            Tool::Fade => "A",
            Tool::Automation => "U",
            Tool::Marquee => "R",
            Tool::Flex => "X",
            Tool::Velocity => "V",
            Tool::Quantize => "Q",
            Tool::Brush => "B",
            Tool::Finger => "F",
            Tool::Gain => "G",
        }
    }
}

fn tool_named(name: &str) -> Option<Tool> {
    Some(match name {
        "Pointer Tool" => Tool::Pointer,
        "Pencil Tool" => Tool::Pencil,
        "Eraser Tool" => Tool::Eraser,
        "Text Tool" => Tool::Text,
        "Scissors Tool" => Tool::Scissors,
        "Join Tool" | "Glue Tool" => Tool::Glue,
        "Solo Tool" => Tool::Solo,
        "Mute Tool" => Tool::Mute,
        "Zoom Tool" => Tool::Zoom,
        "Fade Tool" => Tool::Fade,
        "Automation Select Tool" | "Automation Curve Tool" => Tool::Automation,
        "Marquee Tool" => Tool::Marquee,
        "Flex Tool" => Tool::Flex,
        "Velocity Tool" => Tool::Velocity,
        "Quantize Tool" => Tool::Quantize,
        "Brush Tool" => Tool::Brush,
        "Finger Tool" => Tool::Finger,
        "Gain Tool" | "Set Gain Tool" => Tool::Gain,
        _ => return None,
    })
}

impl App {
    /// The selected regions, in track and time order.
    pub fn region_selection(&self) -> Vec<(usize, usize)> {
        let mut v = self.selected_regions.clone();
        if v.is_empty() {
            v.extend(self.selected_region);
        }
        let p = self.project.as_ref();
        v.sort_by_key(|(t, r)| (*t, p.and_then(|p| p.tracks.get(*t)?.regions.get(*r)).map_or(0, |x| x.start)));
        v
    }

    fn select_regions(&mut self, v: Vec<(usize, usize)>) {
        self.selected_region = v.last().copied();
        self.selected_regions = v;
        if let Some((t, _)) = self.selected_region {
            self.selected_track = Some(t);
        }
    }

    fn all_regions(&self) -> Vec<(usize, usize)> {
        let Some(p) = &self.project else { return Vec::new() };
        p.tracks.iter().enumerate().flat_map(|(t, tr)| (0..tr.regions.len()).map(move |r| (t, r))).collect()
    }

    fn notes_focus(&self) -> bool {
        self.bottom == Bottom::PianoRoll && self.selected_region.is_some() && crate::focus::editor(self)
    }

    /// Changes every selected region (or, in the Piano Roll, the selected notes) with `f`.
    fn each_region(&mut self, what: &str, mut f: impl FnMut(&mut lg_core::Region, u32)) {
        let sel = self.region_selection();
        if sel.is_empty() {
            return;
        }
        self.checkpoint(what);
        let bar = self.bar_ticks();
        if let Some(p) = self.project.as_mut() {
            for (t, r) in sel {
                if let Some(rg) = p.tracks.get_mut(t).and_then(|tr| tr.regions.get_mut(r)) {
                    f(rg, bar);
                }
            }
        }
        self.after_edit();
    }

    fn each_note(&mut self, what: &str, mut f: impl FnMut(&mut Note)) {
        let Some((t, r)) = self.selected_region else { return };
        self.checkpoint(what);
        let sel = self.selected_notes.clone();
        if let Some(rg) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            for (i, n) in rg.notes.iter_mut().enumerate() {
                if sel.is_empty() || sel.contains(&i) {
                    f(n);
                }
            }
        }
        self.after_edit();
    }

    /// Moves notes with their region when it moves (notes are at song positions).
    fn shift_region(rg: &mut lg_core::Region, by: i64) {
        if crate::regionparams::locked(rg) {
            return;
        }
        let to = (rg.start as i64 + by).max(0);
        let d = to - rg.start as i64;
        rg.start = to as u32;
        for n in &mut rg.notes {
            n.pos = (n.pos as i64 + d).max(0) as u32;
        }
    }

    fn transpose(&mut self, by: i8) {
        if self.notes_focus() || !self.selected_notes.is_empty() && self.bottom == Bottom::PianoRoll {
            self.each_note("Transpose", |n| n.pitch = (n.pitch as i16 + by as i16).clamp(0, 127) as u8);
        } else {
            self.each_region("Transpose", |rg, _| {
                for n in &mut rg.notes {
                    n.pitch = (n.pitch as i16 + by as i16).clamp(0, 127) as u8;
                }
            });
        }
    }

    fn nudge_by(&mut self, by: i64) {
        if self.bottom == Bottom::PianoRoll && !self.selected_notes.is_empty() {
            self.each_note("Nudge", |n| n.pos = (n.pos as i64 + by).max(0) as u32);
        } else {
            self.each_region("Nudge", |rg, _| Self::shift_region(rg, by));
        }
    }

    /// Splits the selected regions (or the regions under the playhead on the selected track) at
    /// `at`: notes before stay, the rest go to a new region (Logic: notes starting after the cut).
    pub fn split_at(&mut self, cuts: &[u32]) {
        let mut sel = self.region_selection();
        if sel.is_empty() {
            if let (Some(t), Some(p)) = (self.selected_track, &self.project) {
                sel = p.tracks[t].regions.iter().enumerate().filter(|(_, r)| cuts.iter().any(|&c| c > r.start && c < r.start + r.length)).map(|(i, _)| (t, i)).collect();
            }
        }
        if sel.is_empty() {
            return;
        }
        // notes across a cut: Keep, Shorten or Split (asked when they reach well past it)
        let Some(mode) = crate::regionparams::split_mode(self, &sel, cuts) else { return };
        self.checkpoint("Split Regions");
        let mut new_sel = Vec::new();
        if let Some(p) = self.project.as_mut() {
            // per track, from the last region back, so indexes stay right
            let mut by_track: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
            for (t, r) in sel {
                by_track.entry(t).or_default().push(r);
            }
            for (t, mut rs) in by_track {
                rs.sort_unstable();
                for r in rs.into_iter().rev() {
                    let tr = &mut p.tracks[t];
                    let mut pieces = vec![tr.regions[r].clone()];
                    for &c in cuts {
                        let mut next = Vec::new();
                        for pc in pieces {
                            if c > pc.start && c < pc.start + pc.length {
                                let mut a = pc.clone();
                                let mut b = pc.clone();
                                a.length = c - pc.start;
                                a.notes.retain(|n| n.pos < c);
                                b.start = c;
                                b.length = pc.start + pc.length - c;
                                b.notes.retain(|n| n.pos >= c);
                                crate::regionparams::split_notes(&mut a, &mut b, c, mode);
                                // controllers (kept from the region's start) split with it
                                lg_core::regions::split_events(&mut a.ext, &mut b.ext, c - pc.start);
                                b.origin = pc.source.or(pc.origin);
                                b.source = None;
                                if let Some(au) = b.audio.as_mut() {
                                    let _ = au;
                                }
                                next.push(a);
                                next.push(b);
                            } else {
                                next.push(pc);
                            }
                        }
                        pieces = next;
                    }
                    let n = pieces.len();
                    tr.regions.splice(r..r + 1, pieces);
                    for k in 0..n {
                        new_sel.push((t, r + k));
                    }
                }
            }
        }
        self.select_regions(new_sel);
        self.after_edit();
    }

    /// Join: the selected regions on each track become one (Logic's Join Regions).
    fn join(&mut self) {
        // audio regions (a mixdown) and MIDI regions across tracks: joinaudio.rs
        if crate::joinaudio::join(self) {
            return;
        }
        let sel = self.region_selection();
        if sel.len() == 1 && self.project.as_ref().is_some_and(|p| p.tracks[sel[0].0].regions[sel[0].1].props.looped) {
            // a looped region joins with its loops
            self.checkpoint("Join Regions");
            crate::regionparams::before_join(self, &sel);
            self.after_edit();
            return;
        }
        if sel.len() < 2 {
            return;
        }
        self.checkpoint("Join Regions");
        crate::regionparams::before_join(self, &sel);
        let mut keep = Vec::new();
        if let Some(p) = self.project.as_mut() {
            let mut by_track: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
            for (t, r) in sel {
                by_track.entry(t).or_default().push(r);
            }
            for (t, mut rs) in by_track {
                rs.sort_unstable();
                let tr = &mut p.tracks[t];
                if rs.len() < 2 || rs.iter().any(|&r| tr.regions[r].audio.is_some()) {
                    continue;
                }
                let start = rs.iter().map(|&r| tr.regions[r].start).min().unwrap_or(0);
                let end = rs.iter().map(|&r| tr.regions[r].start + tr.regions[r].length).max().unwrap_or(0);
                let mut joined = tr.regions[rs[0]].clone();
                joined.start = start;
                joined.length = end - start;
                joined.notes = rs.iter().flat_map(|&r| tr.regions[r].notes.clone()).collect();
                joined.notes.sort_by_key(|n| n.pos);
                joined.origin = joined.source.or(joined.origin);
                joined.source = None;
                for &r in rs.iter().rev() {
                    tr.regions.remove(r);
                }
                tr.regions.push(joined);
                keep.push((t, tr.regions.len() - 1));
            }
        }
        self.select_regions(keep);
        self.after_edit();
    }


    /// Runs Logic's editing commands; false for a name it doesn't know.
    pub fn edit_command(&mut self, ctx: &egui::Context, name: &str) -> bool {
        let bar = self.bar_ticks();
        let beat = bar / self.project.as_ref().map_or(4, |p| p.signature.0.max(1) as u32);
        let pos = self.position() as u32;
        if let Some(t) = tool_named(name) {
            if self.bottom == Bottom::PianoRoll && crate::focus::editor(self) {
                self.roll_tool = t;
            } else {
                self.tool = t;
            }
            return true;
        }
        match name {
            // ---- selection
            "Select All" | "All" => {
                if self.notes_focus() {
                    if let Some((t, r)) = self.selected_region {
                        let n = self.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map_or(0, |x| x.notes.len());
                        self.selected_notes = (0..n).collect();
                    }
                } else {
                    let all = self.all_regions();
                    self.select_regions(all);
                }
            }
            "Deselect All" => {
                if self.notes_focus() {
                    self.selected_notes.clear();
                } else {
                    self.selected_regions.clear();
                    self.selected_region = None;
                }
            }
            "Invert Selection" => {
                let sel = self.region_selection();
                let inv: Vec<_> = self.all_regions().into_iter().filter(|x| !sel.contains(x)).collect();
                self.select_regions(inv);
            }
            "Select All Following" | "All Following" => {
                let from = self.region_selection().first().and_then(|(t, r)| self.project.as_ref()?.tracks.get(*t)?.regions.get(*r).map(|x| x.start)).unwrap_or(pos);
                let v: Vec<_> = self.all_regions().into_iter().filter(|(t, r)| self.project.as_ref().is_some_and(|p| p.tracks[*t].regions[*r].start >= from)).collect();
                self.select_regions(v);
            }
            "Select All Following of Same Track/Pitch" | "All Following of Same Track" => {
                if let Some((t, r)) = self.selected_region {
                    let from = self.project.as_ref().map_or(0, |p| p.tracks[t].regions[r].start);
                    let v: Vec<_> = self.all_regions().into_iter().filter(|(tt, rr)| *tt == t && self.project.as_ref().is_some_and(|p| p.tracks[*tt].regions[*rr].start >= from)).collect();
                    self.select_regions(v);
                }
            }
            "Select All Inside Locators" | "Inside Locators" => {
                let (a, b) = self.cycle;
                let v: Vec<_> = self.all_regions().into_iter().filter(|(t, r)| self.project.as_ref().is_some_and(|p| {
                    let x = &p.tracks[*t].regions[*r];
                    x.start >= a && x.start + x.length <= b
                })).collect();
                self.select_regions(v);
            }
            "Select Muted Regions/Cells/Events" | "Muted Regions" => {
                let v: Vec<_> = self.all_regions().into_iter().filter(|(t, r)| self.project.as_ref().is_some_and(|p| p.tracks[*t].regions[*r].muted)).collect();
                self.select_regions(v);
            }
            "Select Equal Regions/Events" | "Equal Regions" | "Select Similar Regions/Events" | "Similar Regions" => {
                if let Some((t, r)) = self.selected_region {
                    let (len, n) = self.project.as_ref().map_or((0, 0), |p| (p.tracks[t].regions[r].length, p.tracks[t].regions[r].notes.len()));
                    let similar = name.contains("Similar");
                    let v: Vec<_> = self.all_regions().into_iter().filter(|(tt, rr)| self.project.as_ref().is_some_and(|p| {
                        let x = &p.tracks[*tt].regions[*rr];
                        x.length == len && (similar || x.notes.len() == n)
                    })).collect();
                    self.select_regions(v);
                }
            }
            "Select Overlapped Regions/Events" => {
                let v: Vec<_> = self.all_regions().into_iter().filter(|(t, r)| self.project.as_ref().is_some_and(|p| {
                    let x = &p.tracks[*t].regions[*r];
                    p.tracks[*t].regions.iter().enumerate().any(|(k, o)| k != *r && o.start < x.start + x.length && x.start < o.start + o.length)
                })).collect();
                self.select_regions(v);
            }
            "Select First, or Shift Marquee Selection Left" | "Select Last, or Shift Marquee Selection Right" => {
                let mut all = self.all_regions();
                all.sort_by_key(|(t, r)| self.project.as_ref().map_or(0, |p| p.tracks[*t].regions[*r].start));
                let pick = if name.starts_with("Select First") { all.first() } else { all.last() };
                if let Some(x) = pick.copied() {
                    self.select_regions(vec![x]);
                }
            }
            n if n.starts_with("Select Previous Region/Cell/Event") || n.starts_with("Select Next Region/Cell/Event") || n.starts_with("Toggle Previous Region") || n.starts_with("Toggle Next Region") => {
                let next = n.contains("Next");
                let toggle = n.starts_with("Toggle");
                let Some((t, r)) = self.selected_region.or_else(|| self.selected_track.map(|t| (t, usize::MAX))) else { return true };
                let Some(tr) = self.project.as_ref().and_then(|p| p.tracks.get(t)) else { return true };
                let mut order: Vec<usize> = (0..tr.regions.len()).collect();
                order.sort_by_key(|&k| tr.regions[k].start);
                let at = order.iter().position(|&k| k == r);
                let pick = match (at, next) {
                    (Some(i), true) => order.get(i + 1),
                    (Some(i), false) => i.checked_sub(1).and_then(|i| order.get(i)),
                    (None, _) => order.first(),
                };
                if let Some(&k) = pick {
                    if toggle {
                        let mut v = self.region_selection();
                        if !v.contains(&(t, k)) {
                            v.push((t, k));
                        }
                        self.select_regions(v);
                    } else {
                        self.select_regions(vec![(t, k)]);
                    }
                    self.center_roll(t, k);
                }
            }
            "Select Highest Notes" | "Select Lowest Notes" => {
                if let Some((t, r)) = self.selected_region {
                    let notes = self.project.as_ref().map(|p| p.tracks[t].regions[r].notes.clone()).unwrap_or_default();
                    let mut by_pos: std::collections::BTreeMap<u32, (usize, u8)> = Default::default();
                    for (i, n) in notes.iter().enumerate() {
                        let e = by_pos.entry(n.pos).or_insert((i, n.pitch));
                        if (name.contains("Highest") && n.pitch > e.1) || (name.contains("Lowest") && n.pitch < e.1) {
                            *e = (i, n.pitch);
                        }
                    }
                    self.selected_notes = by_pos.values().map(|v| v.0).collect();
                }
            }
            "Extend Track Selection Up" | "Extend Track Selection Down" | "Select Previous Track" | "Select Next Track" => {
                let n = self.project.as_ref().map_or(0, |p| p.tracks.len());
                if n > 0 {
                    let cur = self.selected_track.unwrap_or(0);
                    self.selected_track = Some(if name.ends_with("Up") || name.contains("Previous") { cur.saturating_sub(1) } else { (cur + 1).min(n - 1) });
                }
            }
            // ---- region editing
            "Split Regions/Events at Playhead Position" | "Split by Playhead" => self.split_at(&[pos]),
            "Split Regions/Events at Locators or Marquee Selection" | "Split by Locators" => {
                if crate::regionops::apply_marquee(self, crate::regionops::Cut::Split) {
                    return true;
                }
                let (a, b) = self.cycle;
                self.split_at(&[a, b]);
            }
            "Join Regions/Notes" | "Join Regions per Tracks / Fill Cell" | "Join" | "Join Regions" => self.join(),
            "Mute Notes/Regions/Cells/Folders On/Off" | "Mute Regions/Cells" | "Mute" => {
                if self.bottom == Bottom::PianoRoll && !self.selected_notes.is_empty() {
                    crate::arranging::mute_notes(self);
                } else {
                    self.each_region("Mute", |rg, _| rg.muted = !rg.muted);
                }
            }
            "Loop Regions/Cells/Folders On/Off" | "Loop" => self.each_region("Loop", |rg, _| rg.props.looped = !rg.props.looped),
            "Move Region/Event to Playhead Position (Pickup Clock)" | "Move Region/Event to Playhead Position and Select Next Region/Event (Pickup Clock+)" => {
                if self.bottom == Bottom::PianoRoll && !self.selected_notes.is_empty() {
                    let first = self.selected_region.and_then(|(t, r)| {
                        let rg = self.project.as_ref()?.tracks.get(t)?.regions.get(r)?;
                        self.selected_notes.iter().filter_map(|&i| rg.notes.get(i)).map(|n| n.pos).min()
                    });
                    if let Some(f) = first {
                        let d = pos as i64 - f as i64;
                        self.each_note("Pickup Clock", |n| n.pos = (n.pos as i64 + d).max(0) as u32);
                    }
                } else {
                    let first = self.region_selection().iter().filter_map(|(t, r)| self.project.as_ref()?.tracks.get(*t)?.regions.get(*r).map(|x| x.start)).min();
                    if let Some(f) = first {
                        let d = pos as i64 - f as i64;
                        self.each_region("Pickup Clock", |rg, _| Self::shift_region(rg, d));
                    }
                    if name.contains("Select Next") {
                        self.edit_command(ctx, "Select Next Region/Cell/Event or Move Marquee End (or Marquee Point) to Next Transient");
                    }
                }
            }
            "Set Region/Event/Marquee Start to Playhead Position" => self.each_region("Trim Region Start", |rg, _| {
                let end = rg.start + rg.length;
                if pos < end {
                    rg.length = end - pos;
                    rg.start = pos;
                    rg.notes.retain(|n| n.pos >= pos);
                }
            }),
            "Set Region/Event/Marquee End to Playhead Position" => self.each_region("Trim Region End", |rg, _| {
                if pos > rg.start {
                    rg.length = pos - rg.start;
                    let end = pos;
                    rg.notes.retain(|n| n.pos < end);
                }
            }),
            "Set Optimal Region/Cell Lengths Rounded by Bar" | "Set Optimal Region Sizes Rounded by Bar" => self.each_region("Set Optimal Region Sizes", |rg, bar| {
                if rg.notes.is_empty() {
                    return;
                }
                let first = rg.notes.iter().map(|n| n.pos).min().unwrap_or(rg.start);
                let last = rg.notes.iter().map(|n| n.pos + n.len).max().unwrap_or(rg.start);
                rg.start = (first / bar) * bar;
                rg.length = (last - rg.start).div_ceil(bar).max(1) * bar;
            }),
            "Remove Overlaps" | "Trim Region End to Next Region" => {
                let sel = self.region_selection();
                self.checkpoint("Remove Overlaps");
                if let Some(p) = self.project.as_mut() {
                    for (t, r) in sel {
                        let tr = &mut p.tracks[t];
                        let start = tr.regions[r].start;
                        if let Some(next) = tr.regions.iter().map(|o| o.start).filter(|&s| s > start).min() {
                            let rg = &mut tr.regions[r];
                            if name == "Trim Region End to Next Region" || rg.start + rg.length > next {
                                rg.length = next - rg.start;
                                rg.notes.retain(|n| n.pos < next);
                            }
                        }
                    }
                }
                self.after_edit();
            }
            "Crop Regions/Cells Outside Locators/Playable Range or Marquee Selection" | "Crop Regions Outside Locators" => {
                if crate::regionops::apply_marquee(self, crate::regionops::Cut::Crop) {
                    return true;
                }
                let (a, b) = self.cycle;
                self.each_region("Crop", |rg, _| {
                    let s = rg.start.max(a);
                    let e = (rg.start + rg.length).min(b);
                    if e > s {
                        rg.start = s;
                        rg.length = e - s;
                        rg.notes.retain(|n| n.pos >= s && n.pos < e);
                    }
                });
            }
            "Trim Regions to Fill within Locators" => {
                let (a, b) = self.cycle;
                self.each_region("Trim to Fill", |rg, _| {
                    let s = rg.start.max(a);
                    rg.length = b.saturating_sub(s).max(1);
                    rg.start = s;
                });
            }
            "Nudge Region/Event Position Right by Nudge Value" | "Nudge Right" => self.nudge_by(self.nudge as i64),
            "Nudge Region/Event Position Left by Nudge Value" | "Nudge Left" => self.nudge_by(-(self.nudge as i64)),
            "Nudge Region/Event Position Right by Bar" => self.nudge_by(bar as i64),
            "Nudge Region/Event Position Left by Bar" => self.nudge_by(-(bar as i64)),
            "Nudge Region/Event Position Right by Beat" => self.nudge_by(beat as i64),
            "Nudge Region/Event Position Left by Beat" => self.nudge_by(-(beat as i64)),
            "Nudge Region/Event Position Right by Tick" => self.nudge_by(1),
            "Nudge Region/Event Position Left by Tick" => self.nudge_by(-1),
            "Nudge Region/Event/Marquee Length Right by Nudge Value" | "Nudge Region/Event/Marquee Length Left by Nudge Value" => {
                let d = if name.contains("Right") { self.nudge as i64 } else { -(self.nudge as i64) };
                if self.bottom == Bottom::PianoRoll && !self.selected_notes.is_empty() {
                    self.each_note("Nudge Length", |n| n.len = (n.len as i64 + d).max(1) as u32);
                } else {
                    self.each_region("Nudge Length", |rg, _| rg.length = (rg.length as i64 + d).max(1) as u32);
                }
            }
            "Slip Right by Nudge Value" | "Slip Left by Nudge Value" | "Rotate Right by Nudge Value" | "Rotate Left by Nudge Value" => {
                let d = if name.contains("Right") { self.nudge as i64 } else { -(self.nudge as i64) };
                let rotate = name.starts_with("Rotate");
                self.each_region(if rotate { "Rotate" } else { "Slip" }, |rg, _| {
                    let (s, e) = (rg.start as i64, (rg.start + rg.length) as i64);
                    for n in &mut rg.notes {
                        let mut p = n.pos as i64 + d;
                        if rotate {
                            let len = e - s;
                            p = s + (p - s).rem_euclid(len.max(1));
                        }
                        n.pos = p.max(0) as u32;
                    }
                    if !rotate {
                        rg.notes.retain(|n| (n.pos as i64) >= s && (n.pos as i64) < e);
                    }
                });
            }
            "Set Nudge Value to Bar" => self.nudge = bar,
            "Set Nudge Value to Beat" => self.nudge = beat,
            "Set Nudge Value to Division" => self.nudge = beat / 4,
            "Set Nudge Value to Tick" => self.nudge = 1,
            "Set Nudge Value to 10 ms" | "Set Nudge Value to 1 ms" | "Set Nudge Value to Sample" | "Set Nudge Value to SMPTE Frame" | "Set Nudge Value to 0.5 SMPTE Frame" => {
                let secs = match name {
                    "Set Nudge Value to 10 ms" => 0.010,
                    "Set Nudge Value to 1 ms" => 0.001,
                    "Set Nudge Value to SMPTE Frame" => 1.0 / 25.0,
                    "Set Nudge Value to 0.5 SMPTE Frame" => 0.5 / 25.0,
                    _ => 1.0 / 48000.0,
                };
                self.nudge = ((secs * self.tempo() / 60.0 * lg_core::events::PPQ as f64).round() as u32).max(1);
            }
            n if n.starts_with("Transpose KeySignature/Chord/Region/Event +1") => self.transpose(1),
            n if n.starts_with("Transpose KeySignature/Chord/Region/Event -1") => self.transpose(-1),
            n if n.starts_with("Transpose Region/Event +12") => self.transpose(12),
            n if n.starts_with("Transpose Region/Event -12") => self.transpose(-12),
            "Shuffle Left" | "Shuffle Right" => {
                // the selected regions close up on the track: each one right after the previous
                let right = name.ends_with("Right");
                let sel = self.region_selection();
                self.checkpoint(name);
                if let Some(p) = self.project.as_mut() {
                    for (t, r) in sel {
                        let tr = &mut p.tracks[t];
                        let (s, len) = (tr.regions[r].start, tr.regions[r].length);
                        let to = if right {
                            tr.regions.iter().filter(|o| o.start > s).map(|o| o.start).min().map(|n| n.saturating_sub(len))
                        } else {
                            Some(tr.regions.iter().filter(|o| o.start + o.length <= s).map(|o| o.start + o.length).max().unwrap_or(0))
                        };
                        if let Some(to) = to {
                            Self::shift_region(&mut tr.regions[r], to as i64 - s as i64);
                        }
                    }
                }
                self.after_edit();
            }
            "Apply Quantization Permanently" | "Apply All Region/Cell Parameters Permanently" | "Apply All Parameters Permanently" => {
                let only_q = name == "Apply Quantization Permanently";
                self.each_region(name, |rg, _| {
                    let props = rg.props;
                    let applied = if only_q { lg_core::project::RegionProps { quantize: props.quantize, swing: props.swing, ..Default::default() } } else { props };
                    let start = rg.start;
                    rg.notes = rg.notes.iter().map(|n| applied.apply(n, start)).collect();
                    if only_q {
                        rg.props.quantize = 0;
                        rg.props.swing = 50;
                    } else {
                        rg.props = lg_core::project::RegionProps { looped: props.looped, ..Default::default() };
                    }
                });
            }
            "Dequantize" => self.each_region("Dequantize", |rg, _| {
                rg.props.quantize = 0;
                rg.props.swing = 50;
            }),
            "Quantize Selected Events" | "Quantize Notes" | "Quantize" => {
                let grid = self.quantize_grid.max(1);
                if !self.selected_notes.is_empty() {
                    self.each_note("Quantize", |n| n.pos = ((n.pos as f64 / grid as f64).round() as u32) * grid);
                } else {
                    self.each_region("Quantize", |rg, _| rg.props.quantize = grid);
                }
            }
            "Delete Duplicated MIDI Events" | "Delete Duplicates" => self.each_region("Delete Duplicates", |rg, _| {
                let mut seen = std::collections::HashSet::new();
                rg.notes.retain(|n| seen.insert((n.pos, n.pitch)));
            }),
            "Trim Note End to Remove Overlaps to Following Notes (8 Ticks Gap)" | "Trim Note End to Following Notes (8 Ticks Gap)" | "Note Force Legato (Selected/Any)" | "Note Overlap Correction (Selected/Selected)" => {
                let force = name.starts_with("Trim Note End to Following") || name.starts_with("Note Force");
                let Some((t, r)) = self.selected_region else { return true };
                self.checkpoint("Trim Note Ends");
                let sel = self.selected_notes.clone();
                if let Some(rg) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                    let starts: Vec<u32> = rg.notes.iter().map(|n| n.pos).collect();
                    for (i, n) in rg.notes.iter_mut().enumerate() {
                        if !sel.is_empty() && !sel.contains(&i) {
                            continue;
                        }
                        if let Some(next) = starts.iter().copied().filter(|&s| s > n.pos).min() {
                            let to = next.saturating_sub(8).max(n.pos + 1);
                            if force || n.pos + n.len > to {
                                n.len = to - n.pos;
                            }
                        }
                    }
                }
                self.after_edit();
            }
            "Rename Regions/Cells/Scenes" | "Name Regions/Cells by Track Name" => {
                let by_track = name.starts_with("Name Regions");
                let names: Vec<String> = self.project.as_ref().map(|p| p.tracks.iter().map(|t| t.name.clone()).collect()).unwrap_or_default();
                if by_track {
                    let sel = self.region_selection();
                    self.checkpoint("Name Regions by Track Name");
                    if let Some(p) = self.project.as_mut() {
                        for (t, r) in sel {
                            p.tracks[t].regions[r].name = names[t].clone();
                        }
                    }
                    self.after_edit();
                } else if let Some((t, r)) = self.selected_region {
                    let cur = self.project.as_ref().map(|p| p.tracks[t].regions[r].name.clone()).unwrap_or_default();
                    self.renaming_region = Some((t, r, cur));
                }
            }
            "Name Tracks by Region/Cell Name" => {
                let sel = self.region_selection();
                self.checkpoint("Name Tracks by Region Name");
                if let Some(p) = self.project.as_mut() {
                    for (t, r) in sel {
                        p.tracks[t].name = p.tracks[t].regions[r].name.clone();
                    }
                }
                self.after_edit();
            }
            "Paste Replace" => {
                if let Some(rg) = self.clipboard.clone() {
                    let t = self.selected_track.unwrap_or(0);
                    self.checkpoint("Paste Replace");
                    if let Some(tr) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                        let (s, e) = (pos, pos + rg.length);
                        tr.regions.retain(|o| o.start + o.length <= s || o.start >= e);
                        let mut copy = rg.clone();
                        let d = pos as i64 - copy.start as i64;
                        Self::shift_region(&mut copy, d);
                        copy.origin = copy.source.or(copy.origin);
                        copy.source = None;
                        tr.regions.push(copy);
                    }
                    self.after_edit();
                }
            }
            // ---- tracks
            "New Track with Duplicate Settings" | "New Track with Duplicate Settings and Content" | "New Track With Duplicate Setting" => {
                let Some(t) = self.selected_track else { return true };
                self.checkpoint("New Track");
                let content = name.ends_with("Content");
                if let Some(p) = self.project.as_mut() {
                    let mut tr = p.tracks[t].clone();
                    if !content {
                        tr.regions.clear();
                    } else {
                        for r in &mut tr.regions {
                            r.origin = r.source.or(r.origin);
                            r.source = None;
                        }
                    }
                    tr.row = p.tracks.len() as u32 + 1;
                    p.tracks.insert(t + 1, tr);
                    let prog = self.programs.get(t).copied().unwrap_or(0);
                    self.programs.insert((t + 1).min(self.programs.len()), prog);
                }
                self.selected_track = Some(t + 1);
                self.after_edit();
            }
            "New Track with Next Channel / Instrument" | "New Track with Same Channel Strip/Instrument" | "New Software Instrument Track" => {
                self.checkpoint("New Track");
                if let Some(p) = self.project.as_mut() {
                    let row = p.tracks.len() as u32 + 1;
                    let same = name.contains("Same");
                    let base = self.selected_track.and_then(|t| p.tracks.get(t)).filter(|_| same).cloned();
                    let tr = match base {
                        Some(mut b) => {
                            b.regions.clear();
                            b.row = row;
                            b
                        }
                        None => lg_core::Track { name: "Deluxe Classic".into(), kind: TrackKind::Instrument, channel: 0, row, regions: Vec::new(), volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new(), automation: Vec::new(), extra: Default::default() },
                    };
                    p.tracks.push(tr);
                    self.programs.push(4);
                    self.selected_track = Some(p.tracks.len() - 1);
                }
                self.after_edit();
            }
            "New Audio Track" => {
                self.checkpoint("New Audio Track");
                if let Some(p) = self.project.as_mut() {
                    let row = p.tracks.len() as u32 + 1;
                    p.tracks.push(lg_core::Track { name: format!("Audio {}", p.tracks.iter().filter(|t| t.kind == TrackKind::Audio).count() + 1), kind: TrackKind::Audio, channel: 0, row, regions: Vec::new(), volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new(), automation: Vec::new(), extra: Default::default() });
                    self.programs.push(0);
                    self.selected_track = Some(p.tracks.len() - 1);
                }
                self.after_edit();
            }
            "Delete unused Tracks" | "Delete Unused Tracks" => {
                self.checkpoint("Delete Unused Tracks");
                if let Some(p) = self.project.as_mut() {
                    let keep: Vec<bool> = p.tracks.iter().map(|t| !t.regions.is_empty()).collect();
                    let mut i = 0;
                    p.tracks.retain(|_| {
                        i += 1;
                        keep[i - 1]
                    });
                    let mut i = 0;
                    self.programs.retain(|_| {
                        i += 1;
                        keep.get(i - 1).copied().unwrap_or(true)
                    });
                }
                self.selected_track = None;
                self.selected_region = None;
                self.after_edit();
            }
            "Rename Track" => {
                if let Some(t) = self.selected_track {
                    let cur = self.project.as_ref().map(|p| p.tracks[t].name.clone()).unwrap_or_default();
                    self.renaming = Some((t, cur));
                }
            }
            "Toggle Track Mute" | "Mute off for all" | "Toggle Track Solo" | "Solo off for all" => {
                if let Some(p) = self.project.as_mut() {
                    match name {
                        "Mute off for all" => p.tracks.iter_mut().for_each(|t| t.mute = false),
                        "Solo off for all" => p.tracks.iter_mut().for_each(|t| t.solo = false),
                        _ => {
                            if let Some(t) = self.selected_track.and_then(|t| p.tracks.get_mut(t)) {
                                if name.ends_with("Mute") {
                                    t.mute = !t.mute;
                                } else {
                                    t.solo = !t.solo;
                                }
                            }
                        }
                    }
                }
                self.dirty = true;
                self.sync_mix();
            }
            "Engage Solo Tracks (Channel Strips) of Selected Regions/Cells" => {
                let tracks: std::collections::HashSet<usize> = self.region_selection().into_iter().map(|x| x.0).collect();
                if let Some(p) = self.project.as_mut() {
                    let any = tracks.iter().any(|t| p.tracks[*t].solo);
                    for t in tracks {
                        p.tracks[t].solo = !any;
                    }
                }
                self.sync_mix();
            }
            // ---- locators, navigation
            "Set Locators by Regions/Events/Marquee" | "Set Locators by Selection and Enable Cycle" | "Set Rounded Locators by Selection and Enable Cycle" => {
                if let Some(m) = self.ta.regions.marquee.clone() {
                    self.set_cycle((m.from, m.to));
                    if !self.cycle_on() && name.contains("Enable Cycle") {
                        self.toggle_cycle();
                    }
                    return true;
                }
                let sel = self.region_selection();
                let p = self.project.as_ref();
                let a = sel.iter().filter_map(|(t, r)| p?.tracks.get(*t)?.regions.get(*r).map(|x| x.start)).min();
                let b = sel.iter().filter_map(|(t, r)| p?.tracks.get(*t)?.regions.get(*r).map(|x| x.start + x.length)).max();
                if let (Some(mut a), Some(mut b)) = (a, b) {
                    if name.contains("Rounded") {
                        a = (a / bar) * bar;
                        b = b.div_ceil(bar) * bar;
                    }
                    self.set_cycle((a, b));
                    if !self.cycle_on() && name.contains("Enable Cycle") {
                        self.toggle_cycle();
                    }
                }
            }
            "Go to Left Locator" => self.locate(self.cycle.0 as f64),
            "Go to Right Locator" => self.locate(self.cycle.1 as f64),
            "Go to Selection Start" | "Go to Selection End" => {
                let sel = self.region_selection();
                let p = self.project.as_ref();
                let at = if name.ends_with("Start") {
                    sel.iter().filter_map(|(t, r)| p?.tracks.get(*t)?.regions.get(*r).map(|x| x.start)).min()
                } else {
                    sel.iter().filter_map(|(t, r)| p?.tracks.get(*t)?.regions.get(*r).map(|x| x.start + x.length)).max()
                };
                if let Some(a) = at {
                    self.locate(a as f64);
                }
            }
            "Play from Selection" => {
                self.edit_command(ctx, "Go to Selection Start");
                if !self.playing() {
                    self.play_stop();
                }
            }
            "Skip Cycle" => {
                let (a, b) = self.cycle;
                let len = b.saturating_sub(a);
                self.set_cycle((a + len, b + len));
            }
            "Pause" => {
                if self.playing() {
                    let p = self.position();
                    self.stop();
                    self.locate(p);
                }
            }
            n if n.starts_with("Go to Marker Number ") => {
                let k: usize = n["Go to Marker Number ".len()..].trim().parse().unwrap_or(0);
                if let Some(m) = k.checked_sub(1).and_then(|i| self.markers.get(i)) {
                    let at = m.0;
                    self.locate(at as f64);
                }
            }
            "Set Locators by Marker and Enable Cycle" | "Set Locators by Next Marker and Enable Cycle" | "Set Locators by Previous Marker and Enable Cycle" | "Go to Next Marker And Set Locators" | "Go to Previous Marker And Set Locators" => {
                let ms: Vec<u32> = self.markers.iter().map(|m| m.0).collect();
                let cur = ms.iter().rposition(|&m| m <= pos);
                let idx = match name {
                    n if n.contains("Next") => cur.map_or(0, |i| i + 1),
                    n if n.contains("Previous") => cur.and_then(|i| i.checked_sub(1)).unwrap_or(0),
                    _ => cur.unwrap_or(0),
                };
                if let Some(&a) = ms.get(idx) {
                    let b = ms.get(idx + 1).copied().unwrap_or(a + 4 * bar);
                    self.set_cycle((a, b));
                    if !self.cycle_on() {
                        self.toggle_cycle();
                    }
                    if name.starts_with("Go to") {
                        self.locate(a as f64);
                    }
                }
            }
            "Rename Marker" => {
                if let Some(i) = self.markers.iter().rposition(|m| m.0 <= pos) {
                    self.renaming_marker = Some((i, self.markers[i].1.clone()));
                }
            }
            "Create Marker for Region/Marquee Selection" | "Create Marker without rounding" => {
                if name.ends_with("rounding") {
                    return self.key_command(ctx, "Create Marker without Rounding");
                }
                for (t, r) in self.region_selection() {
                    if let Some(rg) = self.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)) {
                        let (s, n) = (rg.start, rg.name.clone());
                        if !self.markers.iter().any(|m| m.0 == s) {
                            self.markers.push(lg_core::markers::Marker::new(s, n));
                        }
                    }
                }
                self.markers.sort_by_key(|m| m.0);
                self.dirty = true;
            }
            // ---- modes
            "Toggle Replace" => self.replace_mode = !self.replace_mode,
            "Toggle Count-in" => self.count_in = !self.count_in,
            "Toggle Snap to Grid" => self.snap_on = !self.snap_on,
            "Toggle Autopunch Mode" => self.autopunch = !self.autopunch,
            // ---- views
            "Show/Hide Smart Controls" => self.bottom = if self.bottom == Bottom::SmartControls { Bottom::None } else { Bottom::SmartControls },
            "Show/Hide List Editors" => self.right_pane = if self.right_pane == Some(RightPane::Lists) { None } else { Some(RightPane::Lists) },
            "Show/Hide Note Pad" => self.right_pane = if self.right_pane == Some(RightPane::Notes) { None } else { Some(RightPane::Notes) },
            "Show/Hide Project Files" | "Show/Hide Loop Browser" | "Show/Hide Apple Loops" => self.right_pane = if self.right_pane == Some(RightPane::Loops) { None } else { Some(RightPane::Loops) },
            "Show/Hide All Files Browser" | "Show/Hide Browsers" => self.right_pane = if self.right_pane == Some(RightPane::Files) { None } else { Some(RightPane::Files) },
            "Show/Hide Quick Help" => self.quick_help = !self.quick_help,
            "Show/Hide Toolbar" => self.show_toolbar = !self.show_toolbar,
            "Show/Hide Play Surface" => return self.key_command(ctx, "Show/Hide Musical Typing"),
            "Open Event List…" => self.right_pane = Some(RightPane::Lists),
            "Open Tempo List…" => {
                self.right_pane = Some(RightPane::Lists);
                self.list_tab = 2;
            }
            "Enter/Exit Full Screen" => {
                let f = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!f));
            }
            "Zoom Window" => {
                let m = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!m));
            }
            "Minimize Window" | "Hide Logic Pro" => crate::chrome::minimize(ctx),
            "Quit" | "Close Window" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            "New Empty Project" | "New from Template…" => {
                self.stop();
                self.set_project(crate::demo::empty(), true);
                crate::screensets::new_project(self);
            }
            "Open…" => self.open_dialog(),
            "Close Project" => {
                self.stop();
                self.set_project(crate::demo::song(), true);
            }
            "Import…" | "Import Audio File…" => {
                if let Some(f) = kn_ui::files::FileDialog::new().set_title("Import").add_filter("Audio and MIDI", &["wav", "aif", "aiff", "caf", "mp3", "flac", "ogg", "m4a", "mid", "midi"]).pick_file() {
                    self.import_file(&f);
                }
            }
            "Export Selection as MIDI File…" => self.export_midi(),
            "Toggle Zoom to fit Selection or All Contents" | "Zoom to fit Locators, store Navigation Snapshot" => {
                let (a, b) = if name.contains("Locators") {
                    self.cycle
                } else {
                    let sel = self.region_selection();
                    let p = self.project.as_ref();
                    let a = sel.iter().filter_map(|(t, r)| p?.tracks.get(*t)?.regions.get(*r).map(|x| x.start)).min();
                    let b = sel.iter().filter_map(|(t, r)| p?.tracks.get(*t)?.regions.get(*r).map(|x| x.start + x.length)).max();
                    match (a, b) {
                        (Some(a), Some(b)) => (a, b),
                        _ => (0, p.and_then(|p| p.tracks.iter().flat_map(|t| t.regions.iter().map(|r| r.start + r.length)).max()).unwrap_or(16 * bar)),
                    }
                };
                let bars = ((b.saturating_sub(a)) as f32 / bar as f32).max(1.0);
                self.bar_w = (self.tracks_w / (bars + 0.5)).clamp(8.0, 1200.0);
                self.scroll_x = a as f32 / bar as f32;
            }
            "Page Left" | "Page Right" => {
                let page = self.tracks_w / self.bar_w;
                self.scroll_x = (self.scroll_x + if name.ends_with("Right") { page } else { -page }).max(0.0);
            }
            "Catch Playhead Position" | "Scroll to Selection" => {
                let at = if name.starts_with("Catch") {
                    pos
                } else {
                    self.region_selection().first().and_then(|(t, r)| self.project.as_ref()?.tracks.get(*t)?.regions.get(*r).map(|x| x.start)).unwrap_or(pos)
                };
                self.scroll_x = (at as f32 / bar as f32 - 1.0).max(0.0);
                self.catch_off = false;
            }
            "Show Tool Menu" => self.tool_menu_open = true,
            "Show/Hide Automation" | "Show Automation" | "Hide Automation" => self.show_automation = !self.show_automation,
            "Set All Tracks to Automation Off" | "Set All Tracks to Automation Read" | "Set All Tracks to Automation Touch" | "Set All Tracks to Automation Latch" => {
                use crate::automation::AutoMode;
                let m = match name.rsplit(' ').next().unwrap_or("") { "Off" => AutoMode::Off, "Touch" => AutoMode::Touch, "Latch" => AutoMode::Latch, _ => AutoMode::Read };
                for t in 0..self.project.as_ref().map_or(0, |p| p.tracks.len()) {
                    self.auto_mode.insert(t, m);
                }
                self.load_engine();
            }
            "Toggle Selected Tracks or Channel Strips Automation Off/Read" | "Toggle Selected Tracks or Channel Strips Automation Latch/Read" => {
                use crate::automation::AutoMode;
                if let Some(t) = self.selected_track {
                    let cur = crate::automation::mode(self, t);
                    let other = if name.contains("Latch") { AutoMode::Latch } else { AutoMode::Off };
                    self.auto_mode.insert(t, if cur == other { AutoMode::Read } else { other });
                    self.load_engine();
                }
            }
            "Delete All Automation on Selected Tracks" | "Delete Visible Automation on Selected Tracks" | "Delete All Automation on Selected Track" | "Delete Visible Automation on Selected Track" => {
                if let Some(t) = self.selected_track {
                    self.checkpoint("Delete Automation");
                    let only = (name.contains("Visible")).then(|| crate::automation::shown_param(self, t));
                    if let Some(tr) = self.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                        tr.automation.retain(|l| only.is_some_and(|o| o != l.param));
                    }
                    self.after_edit();
                }
            }
            // ---- the Tracks area's tracks: stacks, folders, hide, zoom, alternatives (trackarea.rs)
            "Create Track Stack…" | "Create Folder Stack" | "Create Summing Stack" | "Pack Folder" | "Flatten Stack" | "Unpack Folder to New Tracks" | "Unpack Folder to Existing Tracks" | "Convert Folder Stack to Summing Stack"
            | "Open Track Stack" | "Close Track Stack" | "Open/Close Track Stack" | "Open/Close All Track Stacks" | "Go Into Folder or Region" | "Go Out of Folder or Region"
            | "Select Summing Stack Tracks" | "Select Folder Stack Tracks" | "Select Folder Tracks" | "Summing Stack" | "Folder Stack" | "Folder"
            | "Hide Selected Track" | "Hide Unselected Track" | "Unhide All Tracks" | "Toggle Hide View"
            | "Individual Track Zoom In" | "Individual Track Zoom Out" | "Toggle Individual Track Zoom" | "Reset Individual Track Zoom" | "Individual Track Zoom Reset for All Tracks" | "Zoom Vertical In" | "Zoom Vertical Out" | "Toggle Zoom Focused Track or Row" | "Zoom Focused Track"
            | "New Track Alternative" | "Duplicate Track Alternative" | "Delete Inactive Track Alternatives" | "Show Track Alternatives" | "Track Alternatives" | "Show Transposition Track" => return crate::trackarea::command(self, name),
            // ---- regions: reverse, gain, fades, crossfades, bounce in place, the marquee (regionops.rs)
            "Reverse On/Off" | "Reverse" | "Region Gain +1 dB" | "Region Gain -1 dB" | "Region Gain +0.1 dB" | "Region Gain -0.1 dB" | "Normalize Region/Cell Gain…" | "Normalize Region Gain…" | "Apply Normalize Region/Cell Gain Again" | "Apply Normalize Region Gain Again"
            | "Apply Default Crossfade" | "Apply Last Edited Fade Again" | "Remove Fades" | "Remove Fade Ins" | "Remove Fade-In" | "Remove Fade Outs" | "Remove Fade-Out" | "Remove Crossfades" | "Remove Crossfade"
            | "Bounce Regions/Cells in Place…" | "Bounce In Place…" | "Bounce Regions in Place…" | "Bounce Tracks in Place" | "Set Marquee to Selection" => return crate::regionops::command(self, name),
            // ---- automation (automation.rs)
            "Create 1 Track Automation Point at Region Borders" | "Create 2 Track Automation Points at Region Borders" | "Create 1 Automation Point at Region Borders" | "Create 2 Automation Points at Region Borders"
            | "Create 1 Track Automation Point for Visible Parameter" | "Create 2 Track Automation Points for Visible Parameter" | "Create 1 Automation Point for Visible Parameter" | "Create 2 Automation Points for Visible Parameter"
            | "Create 1 Track Automation Point each for Volume, Pan, Sends" | "Create 2 Track Automation Points each for Volume, Pan, Sends" | "Create 1 Automation Point each for Volume, Pan, Sends" | "Create 2 Automation Points each for Volume, Pan, Sends"
            | "Create 1 Track Automation Point at Every Region Border" | "Create 2 Track Automation Points at Every Region Border"
            | "Nudge Automation Up 1 Step" | "Nudge Automation Down 1 Step" | "Nudge Automation Up 10 Steps" | "Nudge Automation Down 10 Steps"
            | "Delete Orphaned Automation on Selected Tracks" | "Delete Orphaned Automation on Selected Track" | "Delete All Track Automation" | "Delete Redundant Automation Points"
            | "Automation: Cycle Through Used Parameters" | "Automation: Display Off" | "Show Track Automation" | "Set Automation Select Tool" | "Set Automation Curve Tool" | "Set Fade Tool" | "Set Marquee Tool" => return crate::automation::command(self, name),
            // Delete: selected automation points, the marquee's range, else the selected regions
            "Delete" => {
                if crate::scoreinsp::delete_selected(self) {
                    return true;
                }
                if crate::automation::delete_selected(self) || crate::regionops::apply_marquee(self, crate::regionops::Cut::Delete) {
                    return true;
                }
                // an editor with the key focus: its notes only, never the tracks' regions
                if crate::focus::editor(self) {
                    if self.notes_focus() && !self.selected_notes.is_empty() {
                        self.delete_selection();
                    }
                    return true;
                }
                if !crate::focus::tracks(self) {
                    return false;
                }
                let mut sel = self.region_selection();
                if sel.is_empty() {
                    return false;
                }
                self.checkpoint("Delete Region");
                sel.sort_unstable();
                if let Some(p) = self.project.as_mut() {
                    for &(t, r) in sel.iter().rev() {
                        if let Some(tr) = p.tracks.get_mut(t) {
                            if r < tr.regions.len() {
                                tr.regions.remove(r);
                            }
                        }
                    }
                }
                for &(t, r) in sel.iter().rev() {
                    crate::stepseq::region_removed(&self.ctx, t, r);
                }
                self.selected_region = None;
                self.selected_regions.clear();
                self.after_edit();
            }
            "Convert to Pattern Region" | "Open Step Sequencer…" | "Show/Hide Step Sequencer" => {
                if let Some((t, r)) = self.selected_region {
                    crate::stepseq::convert_to_pattern(self, ctx, t, r);
                    self.bottom = Bottom::PianoRoll;
                }
            }
            "Create Pattern Region" => {
                if let Some(t) = self.selected_track {
                    let at = pos;
                    crate::stepseq::new_pattern_region(self, ctx, t, at);
                    self.bottom = Bottom::PianoRoll;
                }
            }
            "Open Score Editor…" | "Show/Hide Score Editor" => {
                crate::score::set_editor_tab(ctx, crate::score::EditorTab::Score);
                self.bottom = Bottom::PianoRoll;
            }
            "Open Piano Roll…" => {
                crate::score::set_editor_tab(ctx, crate::score::EditorTab::PianoRoll);
                self.bottom = Bottom::PianoRoll;
            }
            "New Tracks…" | "New Tracks" | "New Track…" => self.new_track_open = true,
            "New Bass Player Track" => crate::players::new_track(self, false),
            "New Keyboard Player Track" => crate::players::new_track(self, true),
            "Show/Hide Global Tracks" | "Show/Hide Marker Track" | "Show/Hide Tempo Track" | "Show/Hide Marker Track Only" | "Global Tracks" => self.global_tracks = !self.global_tracks,
            _ => return false,
        }
        true
    }
}
