//! Staff styles as the Staff Style window edits them (Logic: Layout ▸ Staff Styles): a style's
//! staffs (clef, transposition, key, size, space above and below) and its voices (which staff
//! each sits on, rests, stems, ties, tuplets, beams, colour, heads, the split pitch and MIDI
//! channel that assign notes to it), with the style's type, range and bracket / bar line
//! connections. Logic's predefined styles are built from lg-notation's table by lg-app; a
//! project keeps edited copies (same name) and its own styles here, saved with the score.

/// The Clef pop-up's choices.
pub const CLEFS: [&str; 30] = [
    "Treble", "Treble 8va", "Treble 8vb", "Treble 15ma", "Bass", "Bass 8va", "Bass 8vb", "Alto", "Tenor", "Drum.0", "Drum.1", "Drum.2", "Drum.3", "Drum.4", "Drum.5", "Drum.6", "Drum.7", "Drum.8", "no clef.0", "no clef.1",
    "no clef.2", "no clef.3", "no clef.4", "no clef.5", "no clef.6", "no clef.7", "no clef.8", "TAB Guitar", "TAB Bass", "Percussion",
];
pub const RESTS: [&str; 3] = ["Normal", "Hide", "Slash"];
pub const STEMS: [&str; 4] = ["Auto", "Up", "Down", "Hide"];
pub const STEM_POS: [&str; 3] = ["Auto", "Center", "Side"];
pub const TIES: [&str; 3] = ["Auto", "Up", "Down"];
pub const TUPLETS: [&str; 4] = ["Auto", "Up", "Down", "Hide"];
pub const BEAMS: [&str; 3] = ["Slant", "Horiz.", "Vocal"];
pub const HEADS: [&str; 4] = ["Normal", "Note Names", "Finger Names", "Hide"];
/// Colour pop-up: Black, Pitch, Velocity, then the 16 palette colours ("Color 1" …).
pub const COLOR_MODES: usize = 19;
pub const BRACKETS: [&str; 3] = ["None", "Brace", "Bracket"];
pub const BARLINES: [&str; 3] = ["None", "Every Bar", "Line Starts"];
/// Logic's limits: a staff holds up to 16 voices, a style up to 16 voices.
pub const MAX_VOICES: usize = 16;

/// One staff of a style (a row of the Staff parameters).
#[derive(Clone, Debug, PartialEq)]
pub struct StaffDef {
    /// Index into `CLEFS`.
    pub clef: u8,
    /// Display transposition in semitones (sounding → written).
    pub transpose: i8,
    /// Show the key signature.
    pub key: bool,
    /// Staff size in percent of the score's staff space.
    pub size: u8,
    /// Space above and below the staff, in half staff spaces added to the default.
    pub above: i8,
    pub below: i8,
}

impl Default for StaffDef {
    fn default() -> StaffDef {
        StaffDef { clef: 0, transpose: 0, key: true, size: 100, above: 0, below: 0 }
    }
}

impl StaffDef {
    pub fn clef_name(&self) -> &'static str {
        CLEFS.get(self.clef as usize).copied().unwrap_or("Treble")
    }
    /// Number of staff lines (Drum.N and no clef.N have N; tablature six (bass four); five).
    pub fn lines(&self) -> u8 {
        let n = self.clef_name();
        if let Some(d) = n.strip_prefix("Drum.").or_else(|| n.strip_prefix("no clef.")) {
            return d.parse().unwrap_or(5);
        }
        match n {
            "TAB Guitar" => 6,
            "TAB Bass" => 4,
            _ => 5,
        }
    }
    pub fn clef_index(name: &str) -> u8 {
        CLEFS.iter().position(|c| *c == name).unwrap_or(0) as u8
    }
}

/// One voice of a style (a row of the Voice and Assign parameters).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct VoiceDef {
    /// The staff it sits on (index into `staffs`).
    pub staff: u8,
    /// Indices into `RESTS`, `STEMS`, `STEM_POS`, `TIES`, `TUPLETS`, `BEAMS`, `HEADS`.
    pub rest: u8,
    pub stem: u8,
    pub stem_pos: u8,
    pub tie: u8,
    pub tuplet: u8,
    pub beam: u8,
    pub head: u8,
    /// 0 Black, 1 Pitch, 2 Velocity, 3–18 the palette's colours 1–16.
    pub color: u8,
    /// Split pitch: notes from it up go to this voice (0 = no split).
    pub split: u8,
    /// MIDI channel 1–16 the voice takes (0 = all).
    pub channel: u8,
}

/// A staff style.
#[derive(Clone, Debug, PartialEq)]
pub struct StaffStyleDef {
    pub name: String,
    /// A mapped (drum) style: no Transpose or Key, its name shown with a leading #.
    pub mapped: bool,
    /// The range of notes shown (Lo, Hi).
    pub lo: u8,
    pub hi: u8,
    /// 0 none, 1 brace, 2 bracket over its staffs; bar lines 0 apart, 1 every bar, 2 line starts.
    pub bracket: u8,
    pub barline: u8,
    pub staffs: Vec<StaffDef>,
    pub voices: Vec<VoiceDef>,
}

impl Default for StaffStyleDef {
    fn default() -> StaffStyleDef {
        StaffStyleDef::single("Single Staff")
    }
}

impl StaffStyleDef {
    /// New ▸ Single Staff Style.
    pub fn single(name: &str) -> StaffStyleDef {
        StaffStyleDef { name: name.into(), mapped: false, lo: 0, hi: 127, bracket: 0, barline: 0, staffs: vec![StaffDef::default()], voices: vec![VoiceDef::default()] }
    }
    /// New ▸ Dual Staff Style: treble over bass, split at C3 (60), braced, bar lines joined.
    pub fn dual(name: &str) -> StaffStyleDef {
        StaffStyleDef {
            name: name.into(),
            mapped: false,
            lo: 0,
            hi: 127,
            bracket: 1,
            barline: 1,
            staffs: vec![StaffDef::default(), StaffDef { clef: StaffDef::clef_index("Bass"), ..Default::default() }],
            voices: vec![VoiceDef { staff: 0, split: 60, ..Default::default() }, VoiceDef { staff: 1, ..Default::default() }],
        }
    }
    /// New ▸ Mapped Style: one five-line drum staff.
    pub fn mapped_style(name: &str) -> StaffStyleDef {
        StaffStyleDef { mapped: true, staffs: vec![StaffDef { clef: StaffDef::clef_index("Percussion"), ..Default::default() }], ..StaffStyleDef::single(name) }
    }
    /// The name as the Staff Style window lists it (mapped styles start with #).
    pub fn shown_name(&self) -> String {
        if self.mapped && !self.name.starts_with('#') { format!("#{}", self.name) } else { self.name.clone() }
    }
    /// The overview's type: Single, Dual, Multi, Mapped.
    pub fn type_name(&self) -> &'static str {
        if self.mapped {
            return "Mapped";
        }
        match self.staffs.len() {
            0 | 1 => if self.voices.len() > 1 { "Poly" } else { "Single" },
            2 => "Dual",
            _ => "Multi",
        }
    }
    /// The overview's Size and Transpose: the staffs' common value, None (a dash) when they differ.
    pub fn common_size(&self) -> Option<u8> {
        let f = self.staffs.first()?.size;
        self.staffs.iter().all(|s| s.size == f).then_some(f)
    }
    pub fn common_transpose(&self) -> Option<i8> {
        let f = self.staffs.first()?.transpose;
        self.staffs.iter().all(|s| s.transpose == f).then_some(f)
    }
    /// Sets every staff's size / transposition (the overview's values).
    pub fn set_size(&mut self, v: u8) {
        self.staffs.iter_mut().for_each(|s| s.size = v.clamp(25, 250));
    }
    pub fn set_transpose(&mut self, v: i8) {
        self.staffs.iter_mut().for_each(|s| s.transpose = v);
    }

    /// The voice a note goes to: voices top to bottom, the first taking its channel whose split
    /// it is at or above (a voice without a split takes everything left); else the last voice.
    pub fn voice_for(&self, pitch: u8, channel: u8) -> usize {
        let takes = |v: &VoiceDef| v.channel == 0 || v.channel == channel + 1;
        self.voices.iter().position(|v| takes(v) && (v.split == 0 || pitch >= v.split)).or_else(|| self.voices.iter().position(takes)).unwrap_or(self.voices.len().saturating_sub(1))
    }
    /// The staff a note goes on.
    pub fn staff_for(&self, pitch: u8, channel: u8) -> usize {
        self.voices.get(self.voice_for(pitch, channel)).map_or(0, |v| v.staff as usize).min(self.staffs.len().saturating_sub(1))
    }
    /// Whether a pitch is inside the style's range.
    pub fn in_range(&self, pitch: u8) -> bool {
        pitch >= self.lo && pitch <= self.hi
    }
    /// The voices on a staff, in order.
    pub fn voices_on(&self, staff: usize) -> Vec<usize> {
        (0..self.voices.len()).filter(|&k| self.voices[k].staff as usize == staff).collect()
    }

    /// Add Staff (+): below the selected staff, else above the first; it gets a voice of its own
    /// (a style has at least as many voices as staffs). Returns the new staff's index.
    pub fn add_staff(&mut self, after: Option<usize>, def: StaffDef) -> usize {
        let at = after.map_or(0, |k| (k + 1).min(self.staffs.len()));
        self.staffs.insert(at, def);
        for v in &mut self.voices {
            if v.staff as usize >= at {
                v.staff += 1;
            }
        }
        if self.voices.len() < MAX_VOICES {
            let vi = self.voices.iter().position(|v| v.staff as usize > at).unwrap_or(self.voices.len());
            self.voices.insert(vi, VoiceDef { staff: at as u8, ..Default::default() });
        }
        at
    }
    /// Add Voice (+): below the selected voice (on its staff), else above the first; None at 16.
    pub fn add_voice(&mut self, after: Option<usize>, staff: Option<usize>) -> Option<usize> {
        if self.voices.len() >= MAX_VOICES {
            return None;
        }
        let (at, st) = match (after, staff) {
            (Some(k), _) if k < self.voices.len() => (k + 1, self.voices[k].staff),
            // a staff selected: below that staff's last voice
            (None, Some(s)) => (self.voices.iter().rposition(|v| v.staff as usize == s).map_or(self.voices.len(), |k| k + 1), s as u8),
            _ => (0, self.voices.first().map_or(0, |v| v.staff)),
        };
        self.voices.insert(at, VoiceDef { staff: st, ..Default::default() });
        Some(at)
    }
    /// Delete a staff and all its voices (the last staff stays).
    pub fn delete_staff(&mut self, k: usize) -> bool {
        if self.staffs.len() <= 1 || k >= self.staffs.len() {
            return false;
        }
        self.staffs.remove(k);
        self.voices.retain(|v| v.staff as usize != k);
        for v in &mut self.voices {
            if v.staff as usize > k {
                v.staff -= 1;
            }
        }
        self.keep_voices();
        true
    }
    /// Delete a voice (a staff keeps at least one).
    pub fn delete_voice(&mut self, k: usize) -> bool {
        let Some(v) = self.voices.get(k) else { return false };
        if self.voices.iter().filter(|x| x.staff == v.staff).count() <= 1 {
            return false;
        }
        self.voices.remove(k);
        true
    }
    /// Paste a copied staff above the selected one, else below the last.
    pub fn paste_staff(&mut self, sel: Option<usize>, def: StaffDef) -> usize {
        match sel {
            Some(k) if k < self.staffs.len() => self.add_staff(if k == 0 { None } else { Some(k - 1) }, def),
            _ => self.add_staff(Some(self.staffs.len().saturating_sub(1)), def),
        }
    }
    /// Paste a copied voice above the selected voice, below the selected staff's last voice,
    /// or below the last voice.
    pub fn paste_voice(&mut self, sel_voice: Option<usize>, sel_staff: Option<usize>, mut def: VoiceDef) -> Option<usize> {
        if self.voices.len() >= MAX_VOICES {
            return None;
        }
        let at = match (sel_voice, sel_staff) {
            (Some(k), _) if k < self.voices.len() => {
                def.staff = self.voices[k].staff;
                k
            }
            (None, Some(s)) if s < self.staffs.len() => {
                def.staff = s as u8;
                self.voices.iter().rposition(|v| v.staff as usize == s).map_or(self.voices.len(), |k| k + 1)
            }
            _ => {
                def.staff = self.voices.last().map_or(0, |v| v.staff);
                self.voices.len()
            }
        };
        self.voices.insert(at, def);
        Some(at)
    }
    fn keep_voices(&mut self) {
        for s in 0..self.staffs.len() {
            if !self.voices.iter().any(|v| v.staff as usize == s) {
                self.voices.push(VoiceDef { staff: s as u8, ..Default::default() });
            }
        }
        self.voices.sort_by_key(|v| v.staff);
    }

    /// The style as lines (Studio's score file).
    pub fn to_lines(&self, esc: &dyn Fn(&str) -> String) -> String {
        let mut out = format!("ss\t{}\t{}\t{}\t{}\t{}\t{}\n", esc(&self.name), self.mapped as u8, self.lo, self.hi, self.bracket, self.barline);
        for s in &self.staffs {
            out += &format!("ssstaff\t{}\t{}\t{}\t{}\t{}\t{}\n", s.clef, s.transpose, s.key as u8, s.size, s.above, s.below);
        }
        for v in &self.voices {
            out += &format!("ssvoice\t{} {} {} {} {} {} {} {} {} {} {}\n", v.staff, v.rest, v.stem, v.stem_pos, v.tie, v.tuplet, v.beam, v.head, v.color, v.split, v.channel);
        }
        out
    }

    /// Reads one line into a list of styles (an `ss` line starts a style); true when it was one.
    pub fn read_line(list: &mut Vec<StaffStyleDef>, w: &[&str], unesc: &dyn Fn(&str) -> String) -> bool {
        let n = |k: usize| w.get(k).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
        match w.first().copied() {
            Some("ss") if w.len() >= 7 => {
                list.push(StaffStyleDef { name: unesc(w[1]), mapped: w[2] == "1", lo: n(3) as u8, hi: n(4) as u8, bracket: n(5) as u8, barline: n(6) as u8, staffs: Vec::new(), voices: Vec::new() });
                true
            }
            Some("ssstaff") if w.len() >= 7 => {
                if let Some(s) = list.last_mut() {
                    s.staffs.push(StaffDef { clef: n(1) as u8, transpose: n(2) as i8, key: w[3] == "1", size: n(4) as u8, above: n(5) as i8, below: n(6) as i8 });
                }
                true
            }
            Some("ssvoice") if w.len() >= 2 => {
                let v: Vec<i64> = w[1].split(' ').map(|x| x.parse().unwrap_or(0)).collect();
                let g = |k: usize| v.get(k).copied().unwrap_or(0) as u8;
                if let Some(s) = list.last_mut() {
                    s.voices.push(VoiceDef { staff: g(0), rest: g(1), stem: g(2), stem_pos: g(3), tie: g(4), tuplet: g(5), beam: g(6), head: g(7), color: g(8), split: g(9), channel: g(10) });
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voices_assign_by_split_and_channel() {
        let d = StaffStyleDef::dual("Piano");
        assert_eq!((d.staff_for(72, 0), d.staff_for(48, 0), d.staff_for(60, 0)), (0, 1, 0));
        let mut s = StaffStyleDef::single("x");
        s.add_voice(Some(0), None);
        s.voices[0].channel = 2;
        // channel 2 (index 1) to the first voice, others to the second
        assert_eq!((s.voice_for(60, 1), s.voice_for(60, 0)), (0, 1));
    }

    #[test]
    fn staffs_and_voices_add_delete_paste() {
        let mut s = StaffStyleDef::single("x");
        assert_eq!(s.add_staff(Some(0), StaffDef { clef: 4, ..Default::default() }), 1);
        assert_eq!((s.staffs.len(), s.voices.len(), s.voices[1].staff), (2, 2, 1));
        assert_eq!(s.type_name(), "Dual");
        // no staff selected: above the first
        assert_eq!(s.add_staff(None, StaffDef::default()), 0);
        assert_eq!(s.voices.iter().map(|v| v.staff).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(s.add_voice(None, Some(2)), Some(3));
        assert!(s.delete_staff(2));
        assert_eq!(s.voices.len(), 2, "a staff's voices go with it");
        assert!(!s.delete_voice(0), "a staff keeps one voice");
        let at = s.paste_voice(None, None, VoiceDef { stem: 2, ..Default::default() }).unwrap();
        assert_eq!((at, s.voices[at].staff), (2, 1));
        s.staffs[0].size = 80;
        assert_eq!(s.common_size(), None);
        s.set_size(120);
        assert_eq!(s.common_size(), Some(120));
        assert_eq!(s.paste_staff(None, StaffDef::default()), 2);
    }

    #[test]
    fn style_lines_round_trip() {
        let mut d = StaffStyleDef::dual("My \"Piano\"");
        d.voices[1].color = 7;
        d.staffs[1].above = -3;
        let mut m = StaffStyleDef::mapped_style("Kit");
        m.voices[0].rest = 2;
        let e = |s: &str| s.replace('\t', " ");
        let text = d.to_lines(&e) + &m.to_lines(&e);
        let mut back = Vec::new();
        for l in text.lines() {
            assert!(StaffStyleDef::read_line(&mut back, &l.split('\t').collect::<Vec<_>>(), &|s| s.to_string()));
        }
        assert_eq!(back, vec![d, m.clone()]);
        assert_eq!(m.shown_name(), "#Kit");
        assert_eq!(StaffDef { clef: StaffDef::clef_index("Drum.3"), ..Default::default() }.lines(), 3);
    }
}
