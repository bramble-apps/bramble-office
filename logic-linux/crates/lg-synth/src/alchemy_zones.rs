//! Alchemy's sample zones and groups — the content of the source edit window's main page and the
//! Sampler element. Each source (A–D) holds groups; a group holds zones (one sample each, its root
//! key, key and velocity range, tune, level, pan, key / velocity fades, start / end and loop points)
//! and its trigger settings: Trigger (Attack: zones fire on note on; Release: on note off, fading in
//! over Fade), Fade, Poly (the most zones of the group sounding at once) and the Starts rules
//! (Always, Round Robin with its Order, Random Round Robin, Keyswitch by Transform pad snapshot,
//! the source Keyswitch knob or MIDI notes, Controller ranges), combined with And / Or / Not.
//! `hits` picks the zones a note plays (at most two per group, with their fade gains); `Player`
//! plays one zone through the source's loop mode (None, Continuous, Sustain, Forward/Back,
//! Start/End, Start/Length, All), Reverse and Stereo settings.

use std::path::PathBuf;
use std::sync::Arc;

use lg_dsp::{Param, Unit};

pub const LOOP_MODES: &[&str] = &["None", "Continuous", "Sustain", "Forward/Back", "Start/End", "Start/Length", "All"];
pub const L_NONE: u8 = 0;
pub const L_CONT: u8 = 1;
pub const L_SUSTAIN: u8 = 2;
pub const L_FWDBACK: u8 = 3;
pub const L_STARTEND: u8 = 4;
pub const L_STARTLEN: u8 = 5;
pub const L_ALL: u8 = 6;
pub const TRIGGERS: &[&str] = &["Attack", "Release"];
pub const ZONE_FADES: &[&str] = &["Linear", "Power"];
/// The rule pop-up menu (Delete removes the rule).
pub const RULES: &[&str] = &["Always", "Round Robin", "Random Round Robin", "Keyswitch", "Controller", "Delete"];
pub const CONDS: &[&str] = &["And", "Or", "Not"];
/// The group Poly menu: 0 is Max (no limit), else 1–16.
pub const POLY_MAX: u8 = 16;
/// At most this many zones of one group play for a note.
pub const GROUP_ZONES: usize = 2;
pub const MAX_HITS: usize = 8;

/// Parameters (per source s = 0–3): the Sampler element On, Volume, Reverse; the source's Stereo
/// button, Loop Mode, Loop Start, Loop End / Length and Keyswitch knob.
pub const ZP: u32 = 32000;
pub const SAMP_ON: u32 = ZP;
pub const SAMP_VOL: u32 = ZP + 4;
pub const SAMP_REV: u32 = ZP + 8;
pub const STEREO: u32 = ZP + 12;
pub const LOOP_MODE: u32 = ZP + 16;
pub const LOOP_START: u32 = ZP + 20;
pub const LOOP_END: u32 = ZP + 24;
pub const KEYSW: u32 = ZP + 28;

pub fn params() -> Vec<Param> {
    let leak = |x: String| -> &'static str { Box::leak(x.into_boxed_str()) };
    let mut v = Vec::new();
    for (s, l) in ["A", "B", "C", "D"].iter().enumerate() {
        let s = s as u32;
        v.push(Param::new(SAMP_ON + s, leak(format!("Sampler On {l}")), 0.0, 1.0, 0.0, Unit::OnOff));
        v.push(Param::new(SAMP_VOL + s, leak(format!("Sampler Volume {l}")), 0.0, 1.0, 1.0, Unit::Ratio));
        v.push(Param::new(SAMP_REV + s, leak(format!("Sampler Reverse {l}")), 0.0, 1.0, 0.0, Unit::OnOff));
        v.push(Param::new(STEREO + s, leak(format!("Stereo {l}")), 0.0, 1.0, 1.0, Unit::OnOff));
        v.push(Param::menu(LOOP_MODE + s, leak(format!("Loop Mode {l}")), LOOP_MODES, 0.0));
        v.push(Param::new(LOOP_START + s, leak(format!("Loop Start {l}")), 0.0, 1.0, 0.0, Unit::Ratio));
        v.push(Param::new(LOOP_END + s, leak(format!("Loop End {l}")), 0.0, 1.0, 1.0, Unit::Ratio));
        v.push(Param::new(KEYSW + s, leak(format!("Keyswitch {l}")), 1.0, 10.0, 1.0, Unit::None));
    }
    v
}

/// A keyswitch value: a Transform pad snapshot (1–8), a Keyswitch knob position (1–10) or a note.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ks {
    Snap(u8),
    Keysw(u8),
    Note(u8),
}

impl Ks {
    pub fn name(self) -> String {
        match self {
            Ks::Snap(k) => format!("SNAP {k}"),
            Ks::Keysw(k) => format!("KEYSW{k}"),
            Ks::Note(n) => crate::sampler_doc::note_name(n),
        }
    }
    /// Every value the keyswitch menus offer, in order.
    pub fn all() -> Vec<Ks> {
        let mut v: Vec<Ks> = (1..=8).map(Ks::Snap).collect();
        v.extend((1..=10).map(Ks::Keysw));
        v.extend((0..=127).map(Ks::Note));
        v
    }
    fn same_kind(self, o: Ks) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&o)
    }
}

/// A Controller rule's type: Control 1–8, an X/Y pad axis, a Perform envelope knob, a MIDI CC.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ctrl {
    Perform(u8),
    PadX(u8),
    PadY(u8),
    Env(u8),
    Cc(u8),
}

impl Ctrl {
    pub fn name(self) -> String {
        match self {
            Ctrl::Perform(k) => format!("Control {}", k + 1),
            Ctrl::PadX(p) => format!("X{} Axis", p + 1),
            Ctrl::PadY(p) => format!("Y{} Axis", p + 1),
            Ctrl::Env(k) => ["Env Attack", "Env Decay", "Env Sustain", "Env Release"][k.min(3) as usize].to_string(),
            Ctrl::Cc(c) => format!("CC#{c}"),
        }
    }
    pub fn all() -> Vec<Ctrl> {
        let mut v: Vec<Ctrl> = (0..8).map(Ctrl::Perform).collect();
        v.extend([Ctrl::PadX(0), Ctrl::PadY(0), Ctrl::PadX(1), Ctrl::PadY(1)]);
        v.extend((0..4).map(Ctrl::Env));
        v.extend((0..=127).map(Ctrl::Cc));
        v
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rule {
    Always,
    RoundRobin(i32),
    RandomRR,
    Keyswitch(Ks, Ks),
    /// Type and the range that triggers, in percent.
    Controller(Ctrl, f32, f32),
}

impl Rule {
    pub fn menu_index(&self) -> usize {
        match self {
            Rule::Always => 0,
            Rule::RoundRobin(_) => 1,
            Rule::RandomRR => 2,
            Rule::Keyswitch(..) => 3,
            Rule::Controller(..) => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Cond {
    #[default]
    And,
    Or,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RuleLine {
    pub rule: Rule,
    /// How this rule combines with the next one.
    pub cond: Cond,
}

/// A zone's audio (channels, rate).
#[derive(Clone, Debug, Default)]
pub struct ZAudio {
    pub ch: Vec<Vec<f32>>,
    pub rate: f32,
}

impl ZAudio {
    pub fn frames(&self) -> usize {
        self.ch.first().map(|c| c.len()).unwrap_or(0)
    }
    fn at(&self, c: usize, pos: f64) -> f32 {
        let d = &self.ch[c.min(self.ch.len() - 1)];
        if pos < 0.0 {
            return 0.0;
        }
        let i = pos as usize;
        if i + 1 >= d.len() {
            return if i < d.len() { d[i] } else { 0.0 };
        }
        let f = (pos - i as f64) as f32;
        d[i] + (d[i + 1] - d[i]) * f
    }
}

#[derive(Clone, Debug)]
pub struct Zone {
    pub name: String,
    pub file: Option<PathBuf>,
    pub audio: Option<Arc<ZAudio>>,
    /// Root key: the note that plays the sample at its own pitch.
    pub key: u8,
    pub coarse: f32,
    pub fine: f32,
    pub vol_db: f32,
    pub pan: f32,
    pub lo_key: u8,
    pub hi_key: u8,
    pub lo_vel: u8,
    pub hi_vel: u8,
    /// Zone Fade: 0 Linear, 1 Power.
    pub curve: u8,
    /// Fade ranges: Left / Right in semitones from Lo / Hi Key, Bottom / Top in velocity steps.
    pub left: u8,
    pub right: u8,
    pub bottom: u8,
    pub top: u8,
    /// Playback start / end and loop start / end, in frames.
    pub start: u32,
    pub end: u32,
    pub loop_start: u32,
    pub loop_end: u32,
    pub xfade_on: bool,
    pub xfade: u32,
}

impl Zone {
    pub fn new(name: &str, file: Option<PathBuf>, audio: Option<Arc<ZAudio>>, key: u8) -> Zone {
        let n = audio.as_ref().map(|a| a.frames()).unwrap_or(0) as u32;
        Zone {
            name: name.to_string(),
            file,
            audio,
            key,
            coarse: 0.0,
            fine: 0.0,
            vol_db: 0.0,
            pan: 0.0,
            lo_key: 0,
            hi_key: 127,
            lo_vel: 1,
            hi_vel: 127,
            curve: 0,
            left: 0,
            right: 0,
            bottom: 0,
            top: 0,
            start: 0,
            end: n,
            loop_start: 0,
            loop_end: n,
            xfade_on: false,
            xfade: 0,
        }
    }
    pub fn frames(&self) -> u32 {
        self.audio.as_ref().map(|a| a.frames()).unwrap_or(0) as u32
    }
    pub fn seconds(&self) -> f32 {
        self.audio.as_ref().map(|a| a.frames() as f32 / a.rate.max(1.0)).unwrap_or(0.0)
    }
    /// The zone's key × velocity fade gain for a note (0 outside its ranges).
    pub fn gain(&self, note: u8, vel: u8) -> f32 {
        if note < self.lo_key || note > self.hi_key || vel < self.lo_vel || vel > self.hi_vel {
            return 0.0;
        }
        let ramp = |d: f32, w: u8| if w == 0 { 1.0 } else { (d / w as f32).clamp(0.0, 1.0) };
        let t = ramp((note - self.lo_key) as f32, self.left)
            * ramp((self.hi_key - note) as f32, self.right)
            * ramp((vel - self.lo_vel) as f32, self.bottom)
            * ramp((self.hi_vel - vel) as f32, self.top);
        let a = [
            ramp((note - self.lo_key) as f32, self.left),
            ramp((self.hi_key - note) as f32, self.right),
            ramp((vel - self.lo_vel) as f32, self.bottom),
            ramp((self.hi_vel - vel) as f32, self.top),
        ];
        if self.curve == 1 {
            // equal power: each ramp along a quarter sine
            a.iter().map(|x| (x * std::f32::consts::FRAC_PI_2).sin()).product()
        } else {
            t
        }
    }
    /// Loop start can't pass loop end (nor the playback bounds).
    pub fn set_loop(&mut self, start: u32, end: u32) {
        let n = self.frames();
        self.loop_end = end.min(n);
        self.loop_start = start.min(self.loop_end);
    }
}

#[derive(Clone, Debug)]
pub struct Group {
    pub name: String,
    /// 0 Attack, 1 Release.
    pub trigger: u8,
    /// Fade, 0–100: at note off a fade out (Attack) or fade in (Release).
    pub fade: f32,
    /// Poly: 0 Max, else the most zones of this group sounding at once.
    pub poly: u8,
    pub rules: Vec<RuleLine>,
    pub zones: Vec<Zone>,
}

impl Group {
    pub fn new(name: &str) -> Group {
        Group { name: name.to_string(), trigger: 0, fade: 0.0, poly: 0, rules: vec![RuleLine { rule: Rule::Always, cond: Cond::And }], zones: Vec::new() }
    }
    pub fn round_robin(&self) -> Option<i32> {
        self.rules.iter().find_map(|r| if let Rule::RoundRobin(o) = r.rule { Some(o) } else { None })
    }
    fn random_rr(&self) -> bool {
        self.rules.iter().any(|r| r.rule == Rule::RandomRR)
    }
    /// Note keyswitch ranges in the rules.
    pub fn note_switches(&self) -> impl Iterator<Item = (u8, u8)> + '_ {
        self.rules.iter().filter_map(|r| match r.rule {
            Rule::Keyswitch(Ks::Note(a), Ks::Note(b)) => Some((a.min(b), a.max(b))),
            _ => None,
        })
    }
    /// Whether a rule uses the source Keyswitch knob (KEYSW values): it shows then.
    pub fn uses_knob(&self) -> bool {
        self.rules.iter().any(|r| matches!(r.rule, Rule::Keyswitch(Ks::Keysw(_), _) | Rule::Keyswitch(_, Ks::Keysw(_))))
    }
}

/// Fade (0–100) as seconds: higher is slower.
pub fn fade_seconds(f: f32) -> f32 {
    (f.clamp(0.0, 100.0) / 100.0) * 4.0
}

#[derive(Clone, Debug, Default)]
pub struct SrcZones {
    pub groups: Vec<Group>,
}

impl SrcZones {
    pub fn zone_count(&self) -> usize {
        self.groups.iter().map(|g| g.zones.len()).sum()
    }
    pub fn first_zone(&self) -> Option<&Zone> {
        self.groups.iter().flat_map(|g| g.zones.iter()).next()
    }
    fn next_name(&self) -> String {
        let mut k = self.groups.len() + 1;
        while self.groups.iter().any(|g| g.name == format!("Group {k}")) {
            k += 1;
        }
        format!("Group {k}")
    }
    /// Group (+) ▸ Empty: a new empty group below the others, numbered on.
    pub fn add_empty(&mut self) -> usize {
        let n = self.next_name();
        self.groups.push(Group::new(&n));
        self.groups.len() - 1
    }
    fn next_order(&self) -> i32 {
        self.groups.iter().filter_map(|g| g.round_robin()).max().map(|o| o + 1).unwrap_or(1)
    }
    /// Imports samples as a new group (Group 1, Group 2 …), mapped by `mapping`; a round-robin
    /// import gets a Round Robin rule with the next Order instead of Always.
    pub fn import(&mut self, files: Vec<ImportFile>, mapping: Mapping, round_robin: bool) -> usize {
        let n = self.next_name();
        let mut g = Group::new(&n);
        if round_robin {
            g.rules = vec![RuleLine { rule: Rule::RoundRobin(self.next_order()), cond: Cond::And }];
        }
        g.zones = map_zones(files, mapping);
        self.groups.push(g);
        self.groups.len() - 1
    }
    /// Sets rule `i` of group `g` from the rule menu (index into RULES): Delete removes it;
    /// choosing a rule on the last line adds a new line below; Round Robin takes the next Order
    /// and adds an Always rule below.
    pub fn set_rule(&mut self, g: usize, i: usize, menu: usize) {
        let order = self.next_order();
        let Some(gr) = self.groups.get_mut(g) else { return };
        if menu == 5 {
            if i < gr.rules.len() {
                gr.rules.remove(i);
            }
            return;
        }
        let rule = match menu {
            1 => Rule::RoundRobin(order),
            2 => Rule::RandomRR,
            3 => Rule::Keyswitch(Ks::Snap(1), Ks::Snap(1)),
            4 => Rule::Controller(Ctrl::Perform(0), 0.0, 100.0),
            _ => Rule::Always,
        };
        if i < gr.rules.len() {
            gr.rules[i].rule = rule;
        } else {
            gr.rules.push(RuleLine { rule, cond: Cond::And });
        }
        if menu == 1 && !gr.rules[i + 1..].iter().any(|r| r.rule == Rule::Always) {
            gr.rules.insert(i + 1, RuleLine { rule: Rule::Always, cond: Cond::And });
        }
    }
    /// The keyswitch range's first field: the second follows when it is of another kind or lies
    /// below the new first value.
    pub fn set_ks_first(&mut self, g: usize, i: usize, v: Ks) {
        if let Some(RuleLine { rule: Rule::Keyswitch(a, b), .. }) = self.groups.get_mut(g).and_then(|gr| gr.rules.get_mut(i)) {
            *a = v;
            let below = match (v, *b) {
                (Ks::Snap(x), Ks::Snap(y)) | (Ks::Keysw(x), Ks::Keysw(y)) | (Ks::Note(x), Ks::Note(y)) => y < x,
                _ => true,
            };
            if !v.same_kind(*b) || below {
                *b = v;
            }
        }
    }
    pub fn zone(&self, g: usize, z: usize) -> Option<&Zone> {
        self.groups.get(g)?.zones.get(z)
    }
    pub fn zone_mut(&mut self, g: usize, z: usize) -> Option<&mut Zone> {
        self.groups.get_mut(g)?.zones.get_mut(z)
    }
    /// Removes zones (group, zone) and returns them.
    pub fn take(&mut self, sel: &[(usize, usize)]) -> Vec<Zone> {
        let mut s = sel.to_vec();
        s.sort();
        s.dedup();
        let mut out = Vec::new();
        for &(g, z) in s.iter().rev() {
            if let Some(gr) = self.groups.get_mut(g)
                && z < gr.zones.len()
            {
                out.push(gr.zones.remove(z));
            }
        }
        out.reverse();
        out
    }
    pub fn copy(&self, sel: &[(usize, usize)]) -> Vec<Zone> {
        sel.iter().filter_map(|&(g, z)| self.zone(g, z).cloned()).collect()
    }
    /// Paste (or assign) zones into group `g`, or a new group when None; returns the group.
    pub fn paste(&mut self, zones: Vec<Zone>, g: Option<usize>) -> usize {
        let g = match g {
            Some(g) if g < self.groups.len() => g,
            _ => self.add_empty(),
        };
        self.groups[g].zones.extend(zones);
        g
    }
    /// Assign to existing / new group: the zones move.
    pub fn assign(&mut self, sel: &[(usize, usize)], g: Option<usize>) -> usize {
        // a target group keeps its identity when groups before it lose nothing (zones only move)
        let z = self.take(sel);
        self.paste(z, g)
    }
    /// Drags a zone edge: 0 Lo Key, 1 Hi Key, 2 Lo Vel, 3 Hi Vel; every selected zone moves by
    /// the same amount (each kept inside its other edge).
    pub fn move_edge(&mut self, sel: &[(usize, usize)], edge: u8, delta: i32) {
        for &(g, z) in sel {
            if let Some(zn) = self.zone_mut(g, z) {
                let c = |v: u8, lo: i32, hi: i32| (v as i32 + delta).clamp(lo, hi) as u8;
                match edge {
                    0 => zn.lo_key = c(zn.lo_key, 0, zn.hi_key as i32),
                    1 => zn.hi_key = c(zn.hi_key, zn.lo_key as i32, 127),
                    2 => zn.lo_vel = c(zn.lo_vel, 1, zn.hi_vel as i32),
                    _ => zn.hi_vel = c(zn.hi_vel, zn.lo_vel as i32, 127),
                }
            }
        }
    }
    /// Select MIDI: the zones a note at a velocity would play (any group).
    pub fn zones_at(&self, note: u8, vel: u8) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for (g, gr) in self.groups.iter().enumerate() {
            for (z, zn) in gr.zones.iter().enumerate() {
                if note >= zn.lo_key && note <= zn.hi_key && vel >= zn.lo_vel && vel <= zn.hi_vel {
                    v.push((g, z));
                }
            }
        }
        v
    }
}

#[derive(Clone, Debug, Default)]
pub struct ZoneDoc {
    pub src: [SrcZones; 4],
}

/// One file to import: its name, path, audio and root key (from the filename or the analysis;
/// None: unknown).
pub struct ImportFile {
    pub name: String,
    pub path: Option<PathBuf>,
    pub audio: Arc<ZAudio>,
    pub root: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Mapping {
    #[default]
    Pitch,
    Drum,
}

/// Maps imported samples to zones. One sample spans the whole keyboard and every velocity. Pitch
/// mapping: samples with a root sit at the top of their zone (pitched down below it), the zones
/// reaching down to the previous root and the last up to G8; samples without pitch are spread
/// evenly by order with the root mid-zone. Drum mapping: one key each from C1.
pub fn map_zones(files: Vec<ImportFile>, mapping: Mapping) -> Vec<Zone> {
    let n = files.len();
    let mut zones: Vec<Zone> = Vec::new();
    if mapping == Mapping::Drum {
        for (k, f) in files.into_iter().enumerate() {
            let key = (36 + k).min(127) as u8;
            let mut z = Zone::new(&f.name, f.path, Some(f.audio), key);
            z.lo_key = key;
            z.hi_key = key;
            zones.push(z);
        }
        return zones;
    }
    if n == 1 {
        let f = files.into_iter().next().unwrap();
        return vec![Zone::new(&f.name, f.path, Some(f.audio), f.root.unwrap_or(60))];
    }
    if files.iter().all(|f| f.root.is_some()) {
        let mut fs = files;
        fs.sort_by_key(|f| f.root);
        let mut lo = 0u8;
        let last = fs.len() - 1;
        for (i, f) in fs.into_iter().enumerate() {
            let r = f.root.unwrap();
            let mut z = Zone::new(&f.name, f.path, Some(f.audio), r);
            z.lo_key = lo.min(r);
            z.hi_key = if i == last { 127 } else { r };
            lo = r.saturating_add(1);
            zones.push(z);
        }
        return zones;
    }
    // no pitch: even spread by order, root mid-zone
    let w = 128.0 / n as f32;
    for (i, f) in files.into_iter().enumerate() {
        let lo = (i as f32 * w).round() as u8;
        let hi = (((i + 1) as f32 * w).round() as i32 - 1).clamp(lo as i32, 127) as u8;
        let mut z = Zone::new(&f.name, f.path, Some(f.audio), ((lo as u16 + hi as u16) / 2) as u8);
        z.lo_key = lo;
        z.hi_key = hi;
        zones.push(z);
    }
    zones
}

/// What the rules read when a note starts.
pub struct TrigCtx<'a> {
    pub note: u8,
    pub vel: u8,
    /// The Transform pad's nearest snapshot (1–8).
    pub snap: u8,
    /// The four sources' Keyswitch knobs (1–10).
    pub keysw: [f32; 4],
    pub perform: [f32; 8],
    /// X1, Y1, X2, Y2.
    pub pads: [f32; 4],
    pub env: [f32; 4],
    pub cc: &'a [f32; 128],
    pub morph: bool,
}

/// Rule state: round-robin counters per note, the last note keyswitch per source, a random seed.
#[derive(Clone, Debug)]
pub struct ZState {
    pub rr: [[u32; 128]; 4],
    pub ks_note: [Option<u8>; 4],
    pub seed: u32,
    pub serial: u64,
}

impl Default for ZState {
    fn default() -> Self {
        ZState { rr: [[0; 128]; 4], ks_note: [None; 4], seed: 0x9e37_79b9, serial: 0 }
    }
}

impl ZState {
    fn rand(&mut self, n: usize) -> usize {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed as usize) % n.max(1)
    }
}

/// The zones a note plays per source: (group, zone, gain).
#[derive(Clone, Copy, Debug)]
pub struct Hits {
    pub n: [u8; 4],
    pub h: [[(u8, u16, f32); MAX_HITS]; 4],
}

impl Default for Hits {
    fn default() -> Self {
        Hits { n: [0; 4], h: [[(0, 0, 0.0); MAX_HITS]; 4] }
    }
}

impl Hits {
    pub fn of(&self, s: usize) -> &[(u8, u16, f32)] {
        &self.h[s][..self.n[s] as usize]
    }
}

/// A note keyswitch: notes inside a group's note keyswitch range switch the source to it (they
/// don't play). Returns whether the note was one.
pub fn keyswitch_note(doc: &ZoneDoc, st: &mut ZState, note: u8) -> bool {
    let mut hit = false;
    for (s, src) in doc.src.iter().enumerate() {
        if src.groups.iter().any(|g| g.note_switches().any(|(a, b)| note >= a && note <= b)) {
            st.ks_note[s] = Some(note);
            hit = true;
        }
    }
    hit
}

fn rule_true(src: &SrcZones, s: usize, g: usize, r: Rule, ctx: &TrigCtx, st: &ZState, rr_now: Option<i32>, rnd: Option<usize>) -> bool {
    match r {
        Rule::Always => true,
        Rule::RoundRobin(o) => rr_now.is_none_or(|c| c == o),
        Rule::RandomRR => rnd.is_none_or(|k| k == g),
        Rule::Keyswitch(a, b) => match (a, b) {
            (Ks::Snap(x), Ks::Snap(y)) => (x.min(y)..=x.max(y)).contains(&ctx.snap),
            (Ks::Keysw(x), Ks::Keysw(y)) => (x.min(y)..=x.max(y)).contains(&(ctx.keysw[s].round().clamp(1.0, 10.0) as u8)),
            (Ks::Note(x), Ks::Note(y)) => match st.ks_note[s] {
                Some(n) => (x.min(y)..=x.max(y)).contains(&n),
                // before any keyswitch: the first note-keyswitch group plays
                None => src.groups.iter().position(|gr| gr.note_switches().next().is_some()) == Some(g),
            },
            _ => false,
        },
        Rule::Controller(c, lo, hi) => {
            let v = match c {
                Ctrl::Perform(k) => ctx.perform[k.min(7) as usize],
                Ctrl::PadX(p) => ctx.pads[(p.min(1) * 2) as usize],
                Ctrl::PadY(p) => ctx.pads[(p.min(1) * 2 + 1) as usize],
                Ctrl::Env(k) => ctx.env[k.min(3) as usize],
                Ctrl::Cc(n) => ctx.cc[n.min(127) as usize],
            } * 100.0;
            v >= lo.min(hi) - 1e-3 && v <= lo.max(hi) + 1e-3
        }
    }
}

/// Whether a group's Starts rules let it trigger (rules combined left to right: And, Or, Not =
/// the first and not the second).
pub fn group_on(src: &SrcZones, s: usize, g: usize, ctx: &TrigCtx, st: &ZState, rr_now: Option<i32>, rnd: Option<usize>) -> bool {
    let mut acc: Option<bool> = None;
    let mut pending = Cond::And;
    for line in &src.groups[g].rules {
        let v = rule_true(src, s, g, line.rule, ctx, st, rr_now, rnd);
        acc = Some(match acc {
            None => v,
            Some(a) => match pending {
                Cond::And => a && v,
                Cond::Or => a || v,
                Cond::Not => a && !v,
            },
        });
        pending = line.cond;
    }
    acc.unwrap_or(true)
}

/// The zones a note plays (Attack groups at note on, Release groups at note off). Round robin
/// (two or more Round Robin groups) steps through the Orders on each note on of the same note,
/// groups with the same Order together; Random Round Robin picks one of its groups per note.
/// With morphing on, only a source's first zone plays, without fades.
pub fn hits(doc: &ZoneDoc, release: bool, ctx: &TrigCtx, st: &mut ZState, out: &mut Hits) {
    for (s, src) in doc.src.iter().enumerate() {
        out.n[s] = 0;
        if ctx.morph {
            if let Some((g, gr)) = src.groups.iter().enumerate().find(|(_, g)| !g.zones.is_empty())
                && (gr.trigger == 1) == release
            {
                out.h[s][0] = (g as u8, 0, 1.0);
                out.n[s] = 1;
            }
            continue;
        }
        let trig = if release { 1 } else { 0 };
        let mut orders: Vec<i32> = src.groups.iter().filter(|g| g.trigger == trig).filter_map(|g| g.round_robin()).collect();
        let rr_groups = orders.len();
        orders.sort();
        orders.dedup();
        let note = ctx.note as usize;
        let rr_now = if rr_groups >= 2 {
            let c = orders[st.rr[s][note] as usize % orders.len()];
            if !release {
                st.rr[s][note] = st.rr[s][note].wrapping_add(1);
            }
            Some(c)
        } else {
            None
        };
        let randoms: Vec<usize> = src.groups.iter().enumerate().filter(|(_, g)| g.trigger == trig && g.random_rr()).map(|(i, _)| i).collect();
        let rnd = if randoms.is_empty() { None } else { Some(randoms[st.rand(randoms.len())]) };
        for (g, gr) in src.groups.iter().enumerate() {
            if gr.trigger != trig || !group_on(src, s, g, ctx, st, rr_now, rnd) {
                continue;
            }
            let mut k = 0;
            for (z, zn) in gr.zones.iter().enumerate() {
                let gain = zn.gain(ctx.note, ctx.vel);
                if gain > 0.0 && zn.audio.is_some() && k < GROUP_ZONES && (out.n[s] as usize) < MAX_HITS {
                    out.h[s][out.n[s] as usize] = (g as u8, z as u16, gain);
                    out.n[s] += 1;
                    k += 1;
                }
            }
        }
    }
}

/// The source settings a player follows.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayCfg {
    pub mode: u8,
    pub reverse: bool,
    pub stereo: bool,
    /// Loop Start and Loop End / Length (0–1 of the sound), for Start/End and Start/Length.
    pub ls: f32,
    pub le: f32,
    /// Position: the start offset (0–1 between start and end).
    pub position: f32,
}

/// One zone sounding.
#[derive(Clone, Copy, Debug, Default)]
pub struct Player {
    pub on: bool,
    pub group: u8,
    pub zone: u16,
    pub pos: f64,
    pub dir: f64,
    pub gain: f32,
    /// The group fade (0–1) and its change per frame.
    pub fade: f32,
    pub fade_step: f32,
    pub release: bool,
    pub serial: u64,
}

impl Player {
    pub fn start(z: &Zone, cfg: &PlayCfg, group: u8, zone: u16, gain: f32, serial: u64) -> Player {
        let len = z.frames() as f64;
        let (s, e) = (z.start.min(z.end) as f64, z.end as f64);
        let (pos, dir) = match cfg.mode {
            L_STARTEND => {
                let (a, b) = ((cfg.ls as f64 * len).round(), (cfg.le as f64 * len).round());
                (a, if b < a { -1.0 } else { 1.0 })
            }
            L_STARTLEN => ((cfg.ls as f64 * len).round(), 1.0),
            _ if cfg.reverse => ((e - 1.0).max(s), -1.0),
            _ => (s + cfg.position.clamp(0.0, 1.0) as f64 * (e - s), 1.0),
        };
        Player { on: true, group, zone, pos, dir, gain, fade: 1.0, fade_step: 0.0, release: false, serial }
    }

    /// The next frame (left, right) at `inc` source frames per output frame; `held`: the key is
    /// down (Sustain loops only while held). Turns itself off at the end.
    pub fn next(&mut self, z: &Zone, cfg: &PlayCfg, held: bool, inc: f64) -> (f32, f32) {
        let Some(a) = z.audio.as_deref() else {
            self.on = false;
            return (0.0, 0.0);
        };
        if !self.on {
            return (0.0, 0.0);
        }
        let len = a.frames() as f64;
        let (s, e) = (z.start.min(z.end) as f64, (z.end as f64).min(len));
        let (ls, le) = (z.loop_start as f64, (z.loop_end as f64).min(e));
        let read = |p: f64| -> (f32, f32) {
            let l = a.at(0, p);
            let r = if cfg.stereo && a.ch.len() > 1 { a.at(1, p) } else { l };
            (l, r)
        };
        let (mut l, mut r) = read(self.pos);
        // loop crossfade into the loop start before the loop end
        let looping = matches!(cfg.mode, L_CONT) || (cfg.mode == L_SUSTAIN && held);
        if looping && z.xfade_on && z.xfade > 0 && self.dir > 0.0 && le > ls {
            let xf = (z.xfade as f64).min(le - ls);
            if self.pos >= le - xf && self.pos < le {
                let w = ((self.pos - (le - xf)) / xf) as f32;
                let (l2, r2) = read(self.pos - (le - ls));
                l = l * (1.0 - w) + l2 * w;
                r = r * (1.0 - w) + r2 * w;
            }
        }
        // advance
        self.pos += inc * self.dir;
        let wrap = |pos: &mut f64, dir: &mut f64, a: f64, b: f64, bounce: bool| {
            if b - a < 1.0 {
                return;
            }
            if *dir > 0.0 && *pos >= b {
                if bounce {
                    *pos = b - (*pos - b);
                    *dir = -1.0;
                } else {
                    *pos -= b - a;
                }
            } else if *dir < 0.0 && *pos < a {
                if bounce {
                    *pos = a + (a - *pos);
                    *dir = 1.0;
                } else {
                    *pos += b - a;
                }
            }
        };
        match cfg.mode {
            L_CONT => wrap(&mut self.pos, &mut self.dir, ls, le, false),
            L_SUSTAIN if held => wrap(&mut self.pos, &mut self.dir, ls, le, false),
            L_FWDBACK => {
                // in the loop region (or on the way into it) it bounces
                let inside = self.pos >= ls - inc && self.pos <= le + inc;
                if inside || (self.dir > 0.0 && self.pos >= le) || (self.dir < 0.0 && self.pos < ls) {
                    wrap(&mut self.pos, &mut self.dir, ls, le, true);
                }
            }
            L_STARTEND => {
                let (x, y) = ((cfg.ls as f64 * len).round(), (cfg.le as f64 * len).round());
                // runs from x to y (backward when y < x), then again from x
                if self.dir > 0.0 && self.pos >= y {
                    self.pos = x + (self.pos - y);
                } else if self.dir < 0.0 && self.pos <= y {
                    self.pos = x - (y - self.pos);
                }
                return (l, r);
            }
            L_STARTLEN => {
                let x = (cfg.ls as f64 * len).round();
                let y = x + (cfg.le as f64 * len).round();
                if y - x >= 1.0 && self.pos >= y {
                    self.pos = x + (self.pos - y);
                }
                // past the sample's end: silence (read gives 0)
                return (l, r);
            }
            L_ALL => wrap(&mut self.pos, &mut self.dir, s, e, false),
            _ => {}
        }
        if self.pos >= e || self.pos < s {
            self.on = false;
        }
        (l, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize) -> Arc<ZAudio> {
        Arc::new(ZAudio { ch: vec![(0..n).map(|i| i as f32).collect()], rate: 48000.0 })
    }
    fn file(name: &str, root: Option<u8>) -> ImportFile {
        ImportFile { name: name.into(), path: None, audio: ramp(100), root }
    }
    fn ctx(cc: &[f32; 128], note: u8, vel: u8) -> TrigCtx<'_> {
        TrigCtx { note, vel, snap: 1, keysw: [1.0; 4], perform: [0.0; 8], pads: [0.0; 4], env: [0.0; 4], cc, morph: false }
    }

    #[test]
    fn empty_groups_are_numbered_and_start_always() {
        let mut s = SrcZones::default();
        s.import(vec![file("a", Some(60))], Mapping::Pitch, false);
        assert_eq!(s.groups[0].name, "Group 1");
        let g = s.add_empty();
        assert_eq!(s.groups[g].name, "Group 2");
        assert_eq!(s.groups[g].rules, vec![RuleLine { rule: Rule::Always, cond: Cond::And }]);
        // a single sample spans the whole keyboard and every velocity
        let z = &s.groups[0].zones[0];
        assert_eq!((z.lo_key, z.hi_key, z.lo_vel, z.hi_vel, z.key), (0, 127, 1, 127, 60));
    }

    #[test]
    fn pitch_and_drum_mapping() {
        let z = map_zones(vec![file("a", Some(48)), file("b", Some(60)), file("c", Some(72))], Mapping::Pitch);
        assert_eq!((z[0].lo_key, z[0].hi_key), (0, 48));
        assert_eq!((z[1].lo_key, z[1].hi_key), (49, 60));
        assert_eq!((z[2].lo_key, z[2].hi_key), (61, 127));
        let d = map_zones(vec![file("k", None), file("s", None)], Mapping::Drum);
        assert_eq!((d[0].lo_key, d[0].hi_key, d[1].lo_key), (36, 36, 37));
        // without pitch: spread evenly, root mid-zone
        let e = map_zones(vec![file("x", None), file("y", None)], Mapping::Pitch);
        assert_eq!((e[0].lo_key, e[0].hi_key, e[1].lo_key, e[1].hi_key), (0, 63, 64, 127));
        assert_eq!(e[0].key, 31);
    }

    #[test]
    fn zone_fades() {
        let mut z = Zone::new("z", None, Some(ramp(10)), 60);
        z.lo_key = 36;
        z.left = 12;
        assert_eq!(z.gain(35, 100), 0.0);
        assert!((z.gain(42, 100) - 0.5).abs() < 1e-6);
        assert_eq!(z.gain(48, 100), 1.0);
        z.curve = 1;
        // equal power: louder than linear mid-way
        assert!((z.gain(42, 100) - (0.5f32 * std::f32::consts::FRAC_PI_2).sin()).abs() < 1e-6);
        let mut v = Zone::new("v", None, Some(ramp(10)), 60);
        v.lo_vel = 20;
        v.bottom = 15;
        v.hi_vel = 90;
        v.top = 30;
        assert!(v.gain(60, 20) == 0.0 && v.gain(60, 35) == 1.0);
        assert!((v.gain(60, 75) - 0.5).abs() < 1e-6 && v.gain(60, 90) == 0.0);
    }

    #[test]
    fn round_robin_cycles_by_order() {
        let mut d = ZoneDoc::default();
        let s = &mut d.src[0];
        let g1 = s.import(vec![file("a", Some(60))], Mapping::Pitch, true);
        let g2 = s.import(vec![file("b", Some(60))], Mapping::Pitch, true);
        let g3 = s.import(vec![file("c", Some(60))], Mapping::Pitch, true);
        assert_eq!(s.groups[g2].round_robin(), Some(2));
        // two groups with the same Order sound together
        s.groups[g3].rules[0].rule = Rule::RoundRobin(1);
        let cc = [0.0; 128];
        let mut st = ZState::default();
        let mut h = Hits::default();
        let mut seq = Vec::new();
        for _ in 0..4 {
            hits(&d, false, &ctx(&cc, 60, 100), &mut st, &mut h);
            seq.push(h.of(0).iter().map(|x| x.0 as usize).collect::<Vec<_>>());
        }
        assert_eq!(seq, vec![vec![g1, g3], vec![g2], vec![g1, g3], vec![g2]]);
        // only one RR group: no effect, it always plays
        let mut one = ZoneDoc::default();
        one.src[0].import(vec![file("a", Some(60))], Mapping::Pitch, true);
        for _ in 0..2 {
            hits(&one, false, &ctx(&cc, 60, 100), &mut st, &mut h);
            assert_eq!(h.n[0], 1);
        }
    }

    #[test]
    fn random_round_robin_picks_one_group() {
        let mut d = ZoneDoc::default();
        for _ in 0..3 {
            let g = d.src[0].import(vec![file("a", Some(60))], Mapping::Pitch, false);
            d.src[0].groups[g].rules[0].rule = Rule::RandomRR;
        }
        let cc = [0.0; 128];
        let mut st = ZState::default();
        let mut h = Hits::default();
        let mut seen = [false; 3];
        for _ in 0..40 {
            hits(&d, false, &ctx(&cc, 60, 100), &mut st, &mut h);
            assert_eq!(h.n[0], 1);
            seen[h.of(0)[0].0 as usize] = true;
        }
        assert!(seen.iter().all(|x| *x));
    }

    #[test]
    fn keyswitch_rules() {
        let mut d = ZoneDoc::default();
        let a = d.src[0].import(vec![file("a", Some(60))], Mapping::Pitch, false);
        let b = d.src[0].import(vec![file("b", Some(60))], Mapping::Pitch, false);
        d.src[0].groups[a].rules[0].rule = Rule::Keyswitch(Ks::Snap(2), Ks::Snap(4));
        d.src[0].groups[b].rules[0].rule = Rule::Keyswitch(Ks::Keysw(3), Ks::Keysw(3));
        let cc = [0.0; 128];
        let mut st = ZState::default();
        let mut h = Hits::default();
        let mut c = ctx(&cc, 60, 100);
        c.snap = 3;
        hits(&d, false, &c, &mut st, &mut h);
        assert_eq!(h.of(0).iter().map(|x| x.0 as usize).collect::<Vec<_>>(), vec![a]);
        c.snap = 5;
        c.keysw[0] = 3.0;
        hits(&d, false, &c, &mut st, &mut h);
        assert_eq!(h.of(0).iter().map(|x| x.0 as usize).collect::<Vec<_>>(), vec![b]);
        // note keyswitches: the group stays active until another keyswitch is played
        let mut n = ZoneDoc::default();
        let x = n.src[0].import(vec![file("x", Some(60))], Mapping::Pitch, false);
        let y = n.src[0].import(vec![file("y", Some(60))], Mapping::Pitch, false);
        n.src[0].groups[x].rules[0].rule = Rule::Keyswitch(Ks::Note(24), Ks::Note(24));
        n.src[0].groups[y].rules[0].rule = Rule::Keyswitch(Ks::Note(25), Ks::Note(26));
        let c = ctx(&cc, 60, 100);
        hits(&n, false, &c, &mut st, &mut h);
        assert_eq!(h.of(0)[0].0 as usize, x);
        assert!(keyswitch_note(&n, &mut st, 26));
        assert!(!keyswitch_note(&n, &mut st, 60));
        for _ in 0..2 {
            hits(&n, false, &c, &mut st, &mut h);
            assert_eq!(h.of(0).iter().map(|x| x.0 as usize).collect::<Vec<_>>(), vec![y]);
        }
    }

    #[test]
    fn controller_rule_and_conditions() {
        let mut d = ZoneDoc::default();
        let g = d.src[0].import(vec![file("a", Some(60))], Mapping::Pitch, false);
        d.src[0].groups[g].rules = vec![RuleLine { rule: Rule::Controller(Ctrl::Perform(0), 30.0, 50.0), cond: Cond::And }];
        let mut cc = [0.0; 128];
        let st = ZState::default();
        let mut c = ctx(&cc, 60, 100);
        c.perform[0] = 0.4;
        assert!(group_on(&d.src[0], 0, g, &c, &st, None, None));
        c.perform[0] = 0.6;
        assert!(!group_on(&d.src[0], 0, g, &c, &st, None, None));
        // Control 1 in range NOT CC#1 above 50 %
        d.src[0].groups[g].rules = vec![
            RuleLine { rule: Rule::Controller(Ctrl::Perform(0), 30.0, 50.0), cond: Cond::Not },
            RuleLine { rule: Rule::Controller(Ctrl::Cc(1), 50.0, 100.0), cond: Cond::And },
        ];
        cc[1] = 0.8;
        let mut c = ctx(&cc, 60, 100);
        c.perform[0] = 0.4;
        assert!(!group_on(&d.src[0], 0, g, &c, &st, None, None));
        let cc0 = [0.0; 128];
        let mut c = ctx(&cc0, 60, 100);
        c.perform[0] = 0.4;
        assert!(group_on(&d.src[0], 0, g, &c, &st, None, None));
        // Or: either
        d.src[0].groups[g].rules[0].cond = Cond::Or;
        c.perform[0] = 0.9;
        assert!(!group_on(&d.src[0], 0, g, &c, &st, None, None));
        c.perform[0] = 0.35;
        assert!(group_on(&d.src[0], 0, g, &c, &st, None, None));
    }

    #[test]
    fn rule_menu_edits() {
        let mut s = SrcZones::default();
        let g = s.add_empty();
        s.set_rule(g, 0, 1);
        assert_eq!(s.groups[g].rules[0].rule, Rule::RoundRobin(1));
        assert_eq!(s.groups[g].rules[1].rule, Rule::Always);
        s.set_rule(g, 1, 5);
        assert_eq!(s.groups[g].rules.len(), 1);
        s.set_rule(g, 0, 3);
        s.set_ks_first(g, 0, Ks::Keysw(1));
        assert_eq!(s.groups[g].rules[0].rule, Rule::Keyswitch(Ks::Keysw(1), Ks::Keysw(1)));
        s.set_ks_first(g, 0, Ks::Snap(8));
        assert_eq!(s.groups[g].rules[0].rule, Rule::Keyswitch(Ks::Snap(8), Ks::Snap(8)));
        assert!(!s.groups[g].uses_knob());
        s.set_ks_first(g, 0, Ks::Keysw(2));
        assert!(s.groups[g].uses_knob());
    }

    #[test]
    fn at_most_two_zones_per_group_and_morph_first_only() {
        let mut d = ZoneDoc::default();
        let g = d.src[0].import(vec![file("a", None), file("b", None), file("c", None)], Mapping::Pitch, false);
        for z in &mut d.src[0].groups[g].zones {
            z.lo_key = 0;
            z.hi_key = 127;
        }
        let cc = [0.0; 128];
        let mut st = ZState::default();
        let mut h = Hits::default();
        hits(&d, false, &ctx(&cc, 60, 100), &mut st, &mut h);
        assert_eq!(h.n[0], 2);
        let mut c = ctx(&cc, 60, 100);
        c.morph = true;
        d.src[0].groups[g].zones[0].left = 100;
        hits(&d, false, &c, &mut st, &mut h);
        assert_eq!((h.n[0], h.of(0)[0].1, h.of(0)[0].2), (1, 0, 1.0));
    }

    #[test]
    fn keymap_edits() {
        let mut s = SrcZones::default();
        s.import(vec![file("a", Some(48)), file("b", Some(60))], Mapping::Pitch, false);
        s.move_edge(&[(0, 0), (0, 1)], 0, 2);
        assert_eq!((s.groups[0].zones[0].lo_key, s.groups[0].zones[1].lo_key), (2, 51));
        let g = s.assign(&[(0, 1)], None);
        assert_eq!((s.groups[0].zones.len(), s.groups[g].zones.len()), (1, 1));
        let c = s.copy(&[(0, 0)]);
        s.paste(c, Some(g));
        assert_eq!(s.groups[g].zones.len(), 2);
        let cut = s.take(&[(g, 0), (g, 1)]);
        assert_eq!((cut.len(), s.groups[g].zones.len()), (2, 0));
        assert_eq!(s.zones_at(10, 100), vec![(0, 0)]);
    }

    fn run(z: &Zone, cfg: &PlayCfg, held_for: usize, n: usize) -> Vec<f32> {
        let mut p = Player::start(z, cfg, 0, 0, 1.0, 0);
        (0..n).map(|i| if p.on { p.next(z, cfg, i < held_for, 1.0).0 } else { -1.0 }).collect()
    }

    #[test]
    fn loop_modes() {
        let mut z = Zone::new("z", None, Some(ramp(10)), 60);
        z.set_loop(4, 8);
        let cfg = |mode| PlayCfg { mode, stereo: true, le: 1.0, ..Default::default() };
        // None: once through
        assert_eq!(run(&z, &cfg(L_NONE), 99, 12)[9..], [9.0, -1.0, -1.0]);
        // Continuous: loops 4..8 even after release
        assert_eq!(run(&z, &cfg(L_CONT), 0, 12)[6..], [6.0, 7.0, 4.0, 5.0, 6.0, 7.0]);
        // Sustain: loops while held, then runs to the end
        let v = run(&z, &cfg(L_SUSTAIN), 10, 16);
        assert_eq!(v[8..10], [4.0, 5.0]);
        assert!(v.contains(&9.0) && v[15] == -1.0);
        // Forward/Back bounces
        let v = run(&z, &cfg(L_FWDBACK), 99, 12);
        assert_eq!(v[6..11], [6.0, 7.0, 8.0, 7.0, 6.0]);
        // All: the whole sound
        assert_eq!(run(&z, &cfg(L_ALL), 0, 12)[9..], [9.0, 0.0, 1.0]);
        // Start/End: 20 %..60 %, and reversed when the end lies before the start
        let se = PlayCfg { mode: L_STARTEND, ls: 0.2, le: 0.6, ..Default::default() };
        assert_eq!(run(&z, &se, 0, 6), vec![2.0, 3.0, 4.0, 5.0, 2.0, 3.0]);
        let rev = PlayCfg { mode: L_STARTEND, ls: 0.6, le: 0.2, ..Default::default() };
        assert_eq!(run(&z, &rev, 0, 5), vec![6.0, 5.0, 4.0, 3.0, 6.0]);
        // Start/Length past the end: silence
        let sl = PlayCfg { mode: L_STARTLEN, ls: 0.8, le: 0.4, ..Default::default() };
        assert_eq!(run(&z, &sl, 0, 5), vec![8.0, 9.0, 0.0, 0.0, 8.0]);
        // Reverse plays backward
        let r = PlayCfg { reverse: true, ..Default::default() };
        assert_eq!(run(&z, &r, 0, 3), vec![9.0, 8.0, 7.0]);
        // Position: start offset
        let p = PlayCfg { position: 0.5, ..Default::default() };
        assert_eq!(run(&z, &p, 0, 1), vec![5.0]);
        // loop start can't pass loop end
        z.set_loop(9, 3);
        assert_eq!((z.loop_start, z.loop_end), (3, 3));
    }

    #[test]
    fn stereo_off_plays_left_only() {
        let a = Arc::new(ZAudio { ch: vec![vec![1.0; 4], vec![-1.0; 4]], rate: 48000.0 });
        let z = Zone::new("s", None, Some(a), 60);
        let mut p = Player::start(&z, &PlayCfg::default(), 0, 0, 1.0, 0);
        assert_eq!(p.next(&z, &PlayCfg::default(), true, 1.0), (1.0, 1.0));
        let st = PlayCfg { stereo: true, ..Default::default() };
        assert_eq!(p.next(&z, &st, true, 1.0), (1.0, -1.0));
    }

    #[test]
    fn loop_crossfade_blends_toward_loop_start() {
        let mut z = Zone::new("z", None, Some(ramp(10)), 60);
        z.set_loop(2, 8);
        z.xfade_on = true;
        z.xfade = 2;
        let cfg = PlayCfg { mode: L_CONT, ..Default::default() };
        let v = run(&z, &cfg, 99, 9);
        // at 7: halfway between 7 and 1
        assert_eq!(v[6], 6.0);
        assert!((v[7] - 4.0).abs() < 1e-6);
    }
}
