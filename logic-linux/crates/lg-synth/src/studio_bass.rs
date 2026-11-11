//! Studio Bass stand-in: five electric basses and an upright, played the way a bassist does.
//! Each of the four strings (E A D G) is a voice of its own (the bundled sampled bass when the
//! sample library is installed, the modelled one otherwise), and every note is given a string
//! and a fret: near the hand's position, with open strings used, avoided or preferred, and with
//! the MIDI channel asking for a string or a place on the neck. On top: playing styles (finger,
//! pick, slap), Mute and Definition, each bass's own pickup controls, the upright's Growl,
//! twelve articulations with keyswitches from C-1 (dead notes, harmonics, legato, slides, body
//! and pickup hits), portamento, voice modes and the noises of a played instrument.

use std::sync::LazyLock;

use crate::bass::StudioBass;
use crate::dsp::{self, Biquad, CHUNK, OnePole, Rng};
use crate::sampled::SampledInstrument;
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const INSTRUMENTS: &[&str] = &["Classic", "Sixties", "Rock", "Session", "Modern", "American Upright"];
pub const STYLES: &[&str] = &["Finger", "Pick", "Slap"];
pub const PICKUPS: &[&str] = &["Neck", "Both", "Bridge", "Stereo"];
pub const VOICE_MODES: &[&str] = &["Polyphonic", "Monophonic", "Mono per String"];
pub const OPEN_STRINGS: &[&str] = &["Use", "Avoid", "Prefer"];
/// The articulations (keyswitches from C-1 = MIDI 12 in this order) with Logic's IDs.
pub const ARTS: &[(&str, u8)] = &[("All (Auto)", 1), ("Standard (Auto)", 2), ("First Finger Only", 44), ("Legato", 3), ("Second Finger Only", 45), ("Dead Note", 4), ("Dead Note Harmonic", 6), ("Harmonic", 5), ("Pickup Hit Single", 56), ("Slide by Velocity", 12), ("Pickup Hit All", 57), ("Slide In +2", 37)];
/// Open strings E A D G.
pub const OPEN: [u8; 4] = [28, 33, 38, 43];
pub const FRETS: u8 = 20;
/// Portamento lengths when synced to the tempo: (beats, name).
pub const SLIDE_SYNC: &[(f32, &str)] = &[(0.0625, "1/64"), (0.125, "1/32"), (0.25, "1/16"), (0.5, "1/8"), (1.0, "1/4"), (2.0, "1/2")];

fn build_params() -> Vec<Param> {
    let arts: Vec<&'static str> = ARTS.iter().map(|a| a.0).collect();
    vec![
        Param::menu(0, "Instrument", INSTRUMENTS, 0.0),
        Param::menu(1, "Playing Style", STYLES, 0.0),
        Param::new(2, "Mute", 0.0, 1.0, 0.0, Unit::Ratio),
        Param::new(3, "Definition", 0.0, 1.0, 0.4, Unit::Ratio),
        Param::new(4, "Main Volume", -40.0, 12.0, 0.0, Unit::Db),
        Param::new(5, "Noises", 0.0, 1.0, 0.5, Unit::Ratio),
        Param::new(10, "Pickup Volume", 0.0, 1.0, 1.0, Unit::Ratio),
        Param::new(11, "Pickup Tone", 0.0, 1.0, 0.7, Unit::Ratio),
        Param::new(12, "Neck Volume", 0.0, 1.0, 1.0, Unit::Ratio),
        Param::new(13, "Bridge Volume", 0.0, 1.0, 0.7, Unit::Ratio),
        Param::new(14, "Neck Tone", 0.0, 1.0, 0.7, Unit::Ratio),
        Param::new(15, "Bridge Tone", 0.0, 1.0, 0.7, Unit::Ratio),
        Param::menu(16, "Rhythm/Solo", &["Rhythm", "Solo"], 0.0),
        Param::new(17, "Bass Switch", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(18, "Treble Switch", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::menu(19, "Pickup", PICKUPS, 1.0),
        Param::new(20, "Bass EQ", -12.0, 12.0, 0.0, Unit::Db),
        Param::new(21, "Treble EQ", -12.0, 12.0, 0.0, Unit::Db),
        Param::new(22, "Growl", 0.0, 1.0, 0.3, Unit::Ratio),
        Param::new(23, "Growl Attack", 0.0, 1.0, 0.4, Unit::Ratio),
        Param::new(30, "Portamento", 0.0, 1.0, 0.0, Unit::Ratio),
        Param::new(31, "Portamento Tempo Sync", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(32, "Scrapes", 0.0, 1.0, 0.3, Unit::Ratio),
        Param::new(33, "String Noise", 0.0, 1.0, 0.3, Unit::Ratio),
        Param::new(34, "Hum & Hiss", 0.0, 1.0, 0.1, Unit::Ratio),
        Param::new(35, "Release Noise", 0.0, 1.0, 0.3, Unit::Ratio),
        Param::new(36, "Rattles", 0.0, 1.0, 0.3, Unit::Ratio),
        Param::new(37, "Releases", 0.0, 1.0, 0.3, Unit::Ratio),
        Param::new(38, "Handling", 0.0, 1.0, 0.3, Unit::Ratio),
        Param::new(39, "Pitchbend Up", 0.0, 12.0, 2.0, Unit::Semitones),
        Param::new(40, "Pitchbend Down", 0.0, 12.0, 2.0, Unit::Semitones),
        Param::menu(41, "Voice Mode", VOICE_MODES, 0.0),
        Param::menu(42, "Open Strings", OPEN_STRINGS, 0.0),
        Param::new(43, "Mute Controller", 0.0, 120.0, 0.0, Unit::None),
        Param::menu(44, "Articulation", Box::leak(arts.into_boxed_slice()), 0.0),
        Param::new(45, "Keyswitch Octave", -1.0, 3.0, 0.0, Unit::Octaves),
        Param::new(46, "Keyswitches", 0.0, 1.0, 1.0, Unit::OnOff),
    ]
}

static PARAMS: LazyLock<Vec<Param>> = LazyLock::new(build_params);

pub fn params() -> &'static [Param] {
    &PARAMS
}

/// What the playing style and the bass allow: Slap only on the Classic and the Modern; the
/// upright has no playing style, Mute or Definition.
pub fn style_allowed(instrument: usize, style: usize) -> bool {
    instrument != 5 && (style != 2 || matches!(instrument, 0 | 4))
}

/// The portamento time in seconds (free: up to a second; synced: a note value at the tempo).
pub fn slide_time(amount: f32, sync: bool, bpm: f32) -> (f32, &'static str) {
    if amount <= 0.0 {
        return (0.0, "Off");
    }
    if !sync {
        return (amount * amount, "");
    }
    let k = ((amount * SLIDE_SYNC.len() as f32) as usize).min(SLIDE_SYNC.len() - 1);
    (SLIDE_SYNC[k].0 * 60.0 / bpm.max(20.0), SLIDE_SYNC[k].1)
}

/// What a MIDI channel (1-based) asks for: a string, a hand position (fret), or a pair of strings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ask {
    Auto,
    String(usize),
    Position(u8),
    Strings(usize, usize),
}

pub fn ask_for_channel(channel: u8) -> Ask {
    match channel {
        2..=5 => Ask::String(channel as usize - 2),
        7 => Ask::Position(0),
        8 => Ask::Position(3),
        9 => Ask::Position(5),
        10 => Ask::Position(7),
        11 => Ask::Position(9),
        12 => Ask::Position(12),
        13 => Ask::Strings(0, 1),
        14 => Ask::Strings(2, 3),
        _ => Ask::Auto,
    }
}

/// Chooses (string, fret) for a note: the place nearest the hand that is free, as asked.
/// `open`: 0 use open strings when near, 1 avoid them, 2 prefer them. `busy`: strings that
/// can't take the note (Mono per String keeps a sounding string only when nothing else fits).
pub fn place(note: u8, hand: u8, open: usize, ask: Ask, busy: [bool; 4]) -> Option<(usize, u8)> {
    let hand = match ask {
        Ask::Position(p) => p,
        _ => hand,
    };
    let mut best: Option<(f32, usize, u8)> = None;
    for s in 0..4 {
        if note < OPEN[s] || note - OPEN[s] > FRETS {
            continue;
        }
        let fret = note - OPEN[s];
        // the hand covers four frets from its position; open strings need no hand at all
        let reach = if fret == 0 { 0.0 } else { (fret as f32 - hand as f32 - 1.5).abs() };
        let mut cost = reach;
        if fret == 0 {
            cost += match open {
                1 => 50.0,
                2 => -10.0,
                _ => if hand <= 5 { -0.5 } else { 3.0 },
            };
        }
        match ask {
            Ask::String(a) if a == s => cost -= 100.0,
            Ask::Strings(a, b) if a == s || b == s => cost -= 100.0,
            _ => {}
        }
        if busy[s] {
            cost += 1000.0;
        }
        if best.is_none_or(|b| cost < b.0) {
            best = Some((cost, s, fret));
        }
    }
    best.map(|b| (b.1, b.2))
}

struct StringVoice {
    inst: Box<dyn Instrument>,
    sampled: bool,
    /// The incoming notes it sounds and the note it plays for each.
    notes: [(u8, u8); 6],
    count: usize,
    fret: u8,
    age: f32,
    art: usize,
    /// A slide still to run: semitones left, and its time constant.
    slide: f32,
    slide_tau: f32,
    sent_bend: i16,
    gain: f32,
    auto_off: Option<f32>,
}

pub struct StudioBassInst {
    params: crate::poly::Params,
    strings: Vec<StringVoice>,
    instrument: usize,
    rate: f32,
    use_samples: bool,
    hand: u8,
    last: Option<(usize, u8)>,
    last_art: usize,
    held: [Option<u8>; 128],
    sustain: bool,
    sustained: [bool; 128],
    bend: f32,
    mute_ctl: f32,
    tempo: f32,
    volume_cc: f32,
    expression: f32,
    // tone
    neck: [OnePole; 2],
    bridge_hp: [OnePole; 2],
    bridge_lp: [OnePole; 2],
    eq_lo: [Biquad; 2],
    eq_hi: [Biquad; 2],
    growl_hp: [OnePole; 2],
    // noises
    rng: Rng,
    click: f32,
    click_f: Biquad,
    thump: f32,
    thump_f: Biquad,
    scrape: f32,
    scrape_f: Biquad,
    squeak: f32,
    squeak_f: Biquad,
    hum_phase: f32,
    growl_env: f32,
}

impl StudioBassInst {
    pub fn new(rate: f32) -> StudioBassInst {
        Self::with_samples(rate, true)
    }
    pub fn with_samples(rate: f32, use_samples: bool) -> StudioBassInst {
        let mut b = StudioBassInst {
            params: crate::poly::Params::new(params()),
            strings: Vec::new(),
            instrument: usize::MAX,
            rate,
            use_samples,
            hand: 0,
            last: None,
            last_art: 0,
            held: [None; 128],
            sustain: false,
            sustained: [false; 128],
            bend: 0.0,
            mute_ctl: 0.0,
            tempo: 120.0,
            volume_cc: 1.0,
            expression: 1.0,
            neck: [OnePole::default(); 2],
            bridge_hp: [OnePole::default(); 2],
            bridge_lp: [OnePole::default(); 2],
            eq_lo: [Biquad::default(); 2],
            eq_hi: [Biquad::default(); 2],
            growl_hp: [OnePole::default(); 2],
            rng: Rng::new(9001),
            click: 0.0,
            click_f: Biquad::default(),
            thump: 0.0,
            thump_f: Biquad::default(),
            scrape: 0.0,
            scrape_f: Biquad::default(),
            squeak: 0.0,
            squeak_f: Biquad::default(),
            hum_phase: 0.0,
            growl_env: 0.0,
        };
        b.click_f.bandpass(2800.0, 1.0, rate);
        b.thump_f.bandpass(140.0, 1.2, rate);
        b.scrape_f.bandpass(4200.0, 2.0, rate);
        b.squeak_f.bandpass(3300.0, 6.0, rate);
        b.apply();
        b
    }

    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn on(&self, id: u32) -> bool {
        self.params.get(id) >= 0.5
    }
    fn upright(&self) -> bool {
        self.instrument == 5
    }
    fn style(&self) -> usize {
        let s = self.p(1).round() as usize;
        if style_allowed(self.instrument, s) { s } else { 0 }
    }
    fn mute(&self) -> f32 {
        if self.upright() { 0.0 } else { self.p(2).max(self.mute_ctl) }
    }

    /// The fretboard now: per string the fret being played (None: silent), and the hand's
    /// position (the window's fretboard area shows these).
    pub fn fretboard(&self) -> ([Option<u8>; 4], u8) {
        let mut f = [None; 4];
        for (s, v) in self.strings.iter().enumerate() {
            if v.count > 0 {
                f[s] = Some(v.fret);
            }
        }
        (f, self.hand)
    }
    /// The articulation played last (a keyswitch or the automatic choice can differ from the header).
    pub fn last_played(&self) -> &'static str {
        ARTS[self.last_art.min(ARTS.len() - 1)].0
    }

    fn apply(&mut self) {
        let rate = self.rate;
        let inst = (self.p(0).round() as usize).min(INSTRUMENTS.len() - 1);
        if inst != self.instrument {
            self.instrument = inst;
            let id = if inst == 5 { "bass-upright" } else { "bass-electric" };
            let samples = self.use_samples;
            self.strings = (0..4)
                .map(|_| {
                    let sampled = if samples { SampledInstrument::load(id, rate) } else { None };
                    let (inst, is_sampled): (Box<dyn Instrument>, bool) = match sampled {
                        Some(mut s) => {
                            s.set(12, 24.0);
                            (Box::new(s), true)
                        }
                        None => {
                            let mut b = StudioBass::new(rate);
                            b.set(8, 24.0);
                            (Box::new(b), false)
                        }
                    };
                    StringVoice { inst, sampled: is_sampled, notes: [(0, 0); 6], count: 0, fret: 0, age: 0.0, art: 0, slide: 0.0, slide_tau: 0.03, sent_bend: 0, gain: 1.0, auto_off: None }
                })
                .collect();
            self.held = [None; 128];
            self.sustained = [false; 128];
        }
        let style = self.style();
        let mute = self.mute();
        // the basses' characters, for the modelled strings: (model, tone, pluck position)
        let (model, tone, pluck): (f32, f32, f32) = match (inst, style) {
            (5, _) => (3.0, 0.45, 0.2),
            (_, 1) => (1.0, 0.6, 0.1),
            (_, 2) => (2.0, 0.7, 0.12),
            (1, _) => (6.0, 0.4, 0.2),
            (2, _) => (0.0, 0.65, 0.12),
            (3, _) => (0.0, 0.5, 0.18),
            (4, _) => (0.0, 0.7, 0.14),
            _ => (0.0, 0.5, 0.16),
        };
        for v in &mut self.strings {
            if v.sampled {
                // muted: the note dies at once
                v.inst.set(2, 1.0 - 0.9 * mute);
                v.inst.set(3, 1.0 - mute);
                v.inst.set(4, 1.0 - 0.6 * mute);
                v.inst.set(13, 6.0);
            } else {
                v.inst.set(0, model);
                v.inst.set(1, (tone - 0.3 * mute).clamp(0.0, 1.0));
                v.inst.set(2, (1.0 - 0.93 * mute).max(0.1));
                v.inst.set(3, pluck);
                v.inst.set(10, 0.0);
                v.inst.set(11, 6.0);
            }
        }
        // the pickups: a warm neck path and a tight, bright bridge path
        let (nt, bt) = match inst {
            0 | 3 => (self.p(11), self.p(11)),
            2 => (self.p(14), self.p(15)),
            _ => (0.7, 0.7),
        };
        for c in 0..2 {
            self.neck[c].lowpass_hz(dsp::lerp(350.0, 2600.0, nt), rate);
            self.bridge_hp[c].lowpass_hz(180.0, rate);
            self.bridge_lp[c].lowpass_hz(dsp::lerp(900.0, 7000.0, bt), rate);
            let (lo, hi) = match inst {
                // Sixties: Rhythm lifts the lows, Solo the highs
                1 => if self.on(16) { (-2.0, 5.0) } else { (5.0, -2.0) },
                4 => (self.p(20), self.p(21)),
                _ => (0.0, 0.0),
            };
            self.eq_lo[c].low_shelf(110.0, lo, rate);
            self.eq_hi[c].high_shelf(2200.0, hi, rate);
            self.growl_hp[c].lowpass_hz(500.0, rate);
        }
    }

    /// The neck and bridge pickups' levels for the bass in use, and whether they go left / right.
    fn pickups(&self) -> (f32, f32, bool) {
        match self.instrument {
            0 => (self.p(10), 0.0, false),
            1 => (self.p(12) * self.p(18), self.p(13) * self.p(17), false),
            2 => match self.p(19).round() as i32 {
                0 => (self.p(12), 0.0, false),
                2 => (0.0, self.p(13), false),
                3 => (self.p(12), self.p(13), true),
                _ => (self.p(12), self.p(13), false),
            },
            3 => (self.p(12), self.p(13), false),
            4 => (self.p(10) * 0.7, self.p(10) * 0.7, false),
            _ => (1.0, 0.25, false),
        }
    }

    fn noise_level(&self, id: u32) -> f32 {
        self.p(id) * self.p(5) * 2.0
    }

    fn start(&mut self, chan: u8, pitch: u8, velocity: u8) {
        let art = self.p(44).round() as usize;
        let v = velocity as f32 / 127.0;
        let style = self.style();
        // body and pickup hits: a knock, no string
        if matches!(art, 8 | 10) {
            self.last_art = art;
            self.thump += v * if art == 10 { 0.9 } else { 0.5 };
            self.click += v * 0.25;
            return;
        }
        let mode = self.p(41).round() as usize;
        let mut busy = [false; 4];
        if mode == 2 {
            for (s, sv) in self.strings.iter().enumerate() {
                busy[s] = sv.count > 0;
            }
        }
        if mode == 1 {
            // Monophonic: the last note is let go (or slid from)
            for n in 0..128u8 {
                if self.held[n as usize].is_some() && n != pitch {
                    self.stop(n, false);
                }
            }
        }
        // harmonics sound an octave up
        let sounding = if matches!(art, 6 | 7) { pitch.saturating_add(12).min(127) } else { pitch };
        let lowest = pitch.max(OPEN[0]).min(OPEN[3] + FRETS);
        let Some((s, fret)) = place(lowest, self.hand, self.p(42).round() as usize, ask_for_channel(chan + 1), busy) else { return };
        // the hand follows the fretted notes; moving it is heard
        if fret > 0 {
            let to = fret.saturating_sub(1);
            let moved = (to as i32 - self.hand as i32).abs();
            if moved >= 2 {
                let id = if self.upright() { 38 } else { 33 };
                self.squeak += self.noise_level(id) * (moved as f32 / 12.0).min(1.0) * 0.6;
                self.hand = to;
            }
        }
        // legato and slides come from the note before on the same string
        let from = self.last.filter(|(ls, _)| *ls == s).map(|(_, n)| n);
        let (slide_secs, _) = slide_time(self.p(30), self.on(31), self.tempo);
        let sv = &mut self.strings[s];
        let legato = art == 3 && sv.count > 0;
        if mode != 0 || legato {
            // one note per string: what it was playing ends
            while sv.count > 0 {
                let (inc, n) = sv.notes[sv.count - 1];
                sv.count -= 1;
                if !legato {
                    sv.inst.note_off(n);
                } else {
                    sv.inst.note_off(n);
                }
                self.held[inc as usize] = None;
            }
        }
        if sv.count == sv.notes.len() {
            let (inc, n) = sv.notes[0];
            sv.inst.note_off(n);
            sv.notes.copy_within(1.., 0);
            sv.count -= 1;
            self.held[inc as usize] = None;
        }
        sv.fret = fret;
        sv.age = 0.0;
        sv.art = art;
        sv.auto_off = None;
        sv.slide = 0.0;
        sv.gain = 1.0;
        let mut vel = velocity;
        match art {
            // hammer-on / pull-off: no new pluck to speak of
            3 => {
                vel = (velocity as f32 * 0.55) as u8;
                if let Some(f) = from {
                    sv.slide = (f as f32 - sounding as f32).clamp(-12.0, 12.0);
                    sv.slide_tau = 0.008;
                }
            }
            5 | 6 => {
                sv.auto_off = Some(0.045);
                sv.gain = 0.8;
            }
            7 => sv.gain = 0.6,
            // a slide up into the note, quicker the harder it is played
            9 => {
                sv.slide = -(2.0 + 5.0 * (1.0 - v));
                sv.slide_tau = 0.02 + 0.12 * (1.0 - v);
            }
            11 => {
                sv.slide = -2.0;
                sv.slide_tau = 0.04;
            }
            // one finger / stroke only: the other one's slightly different attack
            2 => sv.gain = 1.05,
            4 => sv.gain = 0.92,
            _ => {}
        }
        if slide_secs > 0.0 && sv.slide == 0.0 {
            if let Some(f) = from {
                sv.slide = (f as f32 - sounding as f32).clamp(-12.0, 12.0);
                sv.slide_tau = slide_secs / 3.0;
            }
        }
        sv.notes[sv.count] = (pitch, sounding);
        sv.count += 1;
        sv.inst.note_on(sounding, vel.max(1));
        self.held[pitch as usize] = Some(s as u8);
        self.last = Some((s, sounding));
        self.last_art = art;
        // the attack: Definition's transient, the pick's scrape, the slap's knock
        let definition = if self.upright() { 0.3 } else { self.p(3) };
        if art != 3 {
            self.click += definition * v * if style == 1 { 0.5 } else if style == 2 { 0.8 } else { 0.3 };
            if style == 1 {
                self.scrape += self.noise_level(32) * v * 0.4;
            }
            if style == 2 {
                self.thump += 0.5 * v;
            }
            if self.upright() && self.rng.unit() < self.p(5) * (0.2 + 0.8 * v) * if pitch < 40 { 1.0 } else { 0.5 } {
                // rattles, buzzes and crackles: likelier the harder and lower the note
                self.scrape += self.noise_level(36) * 0.5;
            }
        }
        self.growl_env = 0.0;
    }

    fn stop(&mut self, pitch: u8, noise: bool) {
        let Some(s) = self.held[pitch as usize].take() else { return };
        let sv = &mut self.strings[s as usize];
        if let Some(i) = sv.notes[..sv.count].iter().position(|n| n.0 == pitch) {
            let n = sv.notes[i].1;
            sv.notes.copy_within(i + 1..sv.count, i);
            sv.count -= 1;
            sv.inst.note_off(n);
            if noise {
                // the finger damping the string
                let id = if self.instrument == 5 { 37 } else { 35 };
                self.squeak += self.p(id) * self.p(5) * 2.0 * 0.15;
                self.thump += self.p(id) * self.p(5) * 0.1;
            }
        }
    }
}

impl Plugin for StudioBassInst {
    fn params(&self) -> &[Param] {
        params()
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            self.apply();
        }
    }
    fn reset(&mut self) {
        for v in &mut self.strings {
            v.inst.reset();
            v.count = 0;
            v.slide = 0.0;
            v.sent_bend = 0;
        }
        self.held = [None; 128];
        self.sustained = [false; 128];
        self.sustain = false;
        self.bend = 0.0;
        self.hand = 0;
        self.last = None;
        self.expression = 1.0;
        self.apply();
    }
    fn tail(&self) -> f32 {
        1.0
    }
    fn value_text(&self, id: u32, value: f32) -> Option<String> {
        match id {
            43 => Some(if value < 0.5 { "Off".into() } else { format!("CC {}", value.round() as i32) }),
            30 => {
                let (secs, name) = slide_time(value, self.on(31), self.tempo);
                Some(if name.is_empty() { format!("{:.0} ms", secs * 1000.0) } else { name.to_string() })
            }
            _ => None,
        }
    }
    fn load_floats(&mut self, _floats: &[f32]) {}
}

impl Instrument for StudioBassInst {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        self.note_on_ch(0, pitch, velocity);
    }
    fn note_off(&mut self, pitch: u8) {
        let pitch = pitch.min(127);
        if self.sustain && self.held[pitch as usize].is_some() {
            self.sustained[pitch as usize] = true;
            return;
        }
        self.stop(pitch, true);
    }
    fn note_on_ch(&mut self, chan: u8, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        let pitch = pitch.min(127);
        // keyswitches from C-1; the octave can be moved
        if self.on(46) {
            let k = pitch as i32 - (12 + 12 * self.p(45).round() as i32);
            if (0..ARTS.len() as i32).contains(&k) {
                self.params.set(44, k as f32);
                return;
            }
        }
        self.sustained[pitch as usize] = false;
        if self.held[pitch as usize].is_some() {
            self.stop(pitch, false);
        }
        self.start(chan, pitch, velocity);
    }
    fn note_off_ch(&mut self, _chan: u8, pitch: u8) {
        self.note_off(pitch);
    }
    fn controller(&mut self, cc: u8, value: u8) {
        let v = value as f32 / 127.0;
        if cc as i32 == self.p(43).round() as i32 && cc > 0 && !self.upright() {
            self.mute_ctl = v;
            self.apply();
            return;
        }
        match cc {
            120 | 123 => {
                for n in 0..128u8 {
                    self.stop(n, false);
                }
                self.sustained = [false; 128];
            }
            64 => {
                let down = value >= 64;
                if self.sustain && !down {
                    for n in 0..128u8 {
                        if self.sustained[n as usize] {
                            self.sustained[n as usize] = false;
                            self.stop(n, true);
                        }
                    }
                }
                self.sustain = down;
            }
            7 => self.volume_cc = v * v,
            11 => self.expression = v,
            _ => {}
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.bend = value as f32 / 8192.0;
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.tempo = bpm as f32;
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let rate = self.rate;
        let bend = if self.bend >= 0.0 { self.bend * self.p(39) } else { self.bend * self.p(40) };
        let (nv, bv, stereo) = self.pickups();
        let out = lg_dsp::db_to_gain(self.p(4)) * self.volume_cc * self.expression;
        let upright = self.upright();
        let growl = if upright { self.p(22) } else { 0.0 };
        // growl comes in over the note: later for long notes when Growl Attack is high
        let growl_rate = 1.0 / (0.03 + 1.2 * self.p(23));
        let hum = if upright { 0.0 } else { self.noise_level(34) * 0.004 };
        let playing = self.strings.iter().any(|s| s.count > 0);
        let mut bl = [0.0f32; CHUNK];
        let mut br = [0.0f32; CHUNK];
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            let dt = m as f32 / rate;
            bl[..m].fill(0.0);
            br[..m].fill(0.0);
            for sv in &mut self.strings {
                sv.age += dt;
                if let Some(t) = sv.auto_off {
                    if sv.age >= t {
                        sv.auto_off = None;
                        for k in 0..sv.count {
                            sv.inst.note_off(sv.notes[k].1);
                        }
                    }
                }
                sv.slide *= (-dt / sv.slide_tau.max(0.002)).exp();
                if sv.slide.abs() < 0.01 {
                    sv.slide = 0.0;
                }
                let raw = ((bend + sv.slide) / 24.0 * 8192.0).clamp(-8192.0, 8191.0) as i16;
                if raw != sv.sent_bend {
                    sv.sent_bend = raw;
                    sv.inst.pitch_bend(raw);
                }
                let (mut sl, mut sr) = ([0.0f32; CHUNK], [0.0f32; CHUNK]);
                sv.inst.render(&mut sl[..m], &mut sr[..m]);
                for i in 0..m {
                    bl[i] += sl[i] * sv.gain;
                    br[i] += sr[i] * sv.gain;
                }
            }
            self.growl_env = (self.growl_env + dt * growl_rate).min(1.0);
            let g = growl * self.growl_env;
            for i in 0..m {
                // noises of the playing: attack click, knocks, pick scrapes, fingers on the strings
                let w = self.rng.bipolar();
                let mut noise = 0.0;
                if self.click > 1e-4 {
                    noise += self.click_f.run(w) * self.click * 0.7;
                    self.click *= 0.996;
                }
                if self.thump > 1e-4 {
                    noise += self.thump_f.run(w) * self.thump * 0.5;
                    self.thump *= 0.9985;
                }
                if self.scrape > 1e-4 {
                    noise += self.scrape_f.run(w) * self.scrape * 0.15;
                    self.scrape *= 0.9992;
                }
                if self.squeak > 1e-4 {
                    noise += self.squeak_f.run(w) * self.squeak * 0.12;
                    self.squeak *= 0.9994;
                }
                if hum > 0.0 {
                    self.hum_phase = (self.hum_phase + 60.0 / rate).fract();
                    noise += hum * (dsp::sin_cycles(self.hum_phase) + 0.5 * dsp::sin_cycles((self.hum_phase * 3.0).fract()) + 0.6 * w);
                }
                let mut o = [0.0f32; 2];
                for (c, x) in [bl[i], br[i]].into_iter().enumerate() {
                    let mut x = x;
                    if g > 0.0 {
                        // the string against the fingerboard: upper harmonics
                        let hi = x - self.growl_hp[c].lp(x);
                        x += dsp::fast_tanh(hi * 14.0) * g * 0.25 * x.abs().min(1.0).sqrt();
                    }
                    let neck = self.neck[c].lp(x);
                    let b = x - self.bridge_hp[c].lp(x);
                    let bridge = self.bridge_lp[c].lp(b) * 1.6;
                    let y = if stereo {
                        if c == 0 { neck * nv * 1.4 } else { bridge * bv * 1.4 }
                    } else {
                        neck * nv + bridge * bv
                    };
                    o[c] = self.eq_hi[c].run(self.eq_lo[c].run(y)) + noise;
                }
                left[pos + i] += o[0] * out;
                right[pos + i] += o[1] * out;
            }
            pos += m;
        }
        let _ = playing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }
    fn bass() -> StudioBassInst {
        let mut b = StudioBassInst::with_samples(48000.0, false);
        b.set(5, 0.0);
        b.set(3, 0.0);
        b
    }
    fn play(b: &mut StudioBassInst, secs: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (48000.0 * secs) as usize;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        for (x, y) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            b.render(x, y);
        }
        (l, r)
    }
    fn level(set: &[(u32, f32)], note: u8) -> (f32, f32) {
        let mut b = bass();
        for (id, v) in set {
            b.set(*id, *v);
        }
        b.note_on(note, 100);
        let (l, r) = play(&mut b, 0.5);
        assert!(l.iter().chain(r.iter()).all(|x| x.is_finite()));
        (rms(&l), rms(&r))
    }

    #[test]
    fn notes_find_a_string_and_a_fret() {
        // the low E: the open E string; a fifth-fret A or the open A string by the setting
        assert_eq!(place(28, 0, 0, Ask::Auto, [false; 4]), Some((0, 0)));
        assert_eq!(place(33, 0, 1, Ask::Auto, [false; 4]), Some((0, 5)), "Avoid: fretted");
        assert_eq!(place(33, 7, 2, Ask::Auto, [false; 4]), Some((1, 0)), "Prefer: open, wherever the hand is");
        assert_eq!(place(33, 0, 0, Ask::Auto, [false; 4]), Some((1, 0)), "Use: open when near");
        // high on the neck the same note is played there
        assert_eq!(place(45, 9, 0, Ask::Auto, [false; 4]), Some((1, 12)));
        assert_eq!(place(45, 0, 0, Ask::Auto, [false; 4]), Some((3, 2)));
        // channels ask for a string or a position
        assert_eq!(ask_for_channel(3), Ask::String(1));
        assert_eq!(ask_for_channel(9), Ask::Position(5));
        assert_eq!(ask_for_channel(14), Ask::Strings(2, 3));
        assert_eq!(place(45, 0, 0, Ask::String(0), [false; 4]), Some((0, 17)), "the E string when asked");
        assert_eq!(place(45, 0, 0, Ask::Position(12), [false; 4]).unwrap().1, 12);
        // out of the string's reach: another one
        assert_eq!(place(60, 0, 0, Ask::String(0), [false; 4]), Some((3, 17)));
    }

    #[test]
    fn the_fretboard_follows_the_playing() {
        let mut b = bass();
        b.note_on(45, 100);
        let (frets, _) = b.fretboard();
        assert_eq!(frets[3], Some(2));
        b.note_off(45);
        assert_eq!(b.fretboard().0, [None; 4]);
        // channel 12: the hand goes to the 9th fret
        b.note_on_ch(10, 45, 100);
        let (frets, hand) = b.fretboard();
        assert_eq!(frets[1], Some(12));
        assert!(hand >= 9, "{hand}");
    }

    #[test]
    fn voice_modes() {
        let mut b = bass();
        b.set(41, 1.0);
        b.note_on(40, 100);
        b.note_on(47, 100);
        assert_eq!(b.fretboard().0.iter().flatten().count(), 1, "Monophonic");
        let mut b = bass();
        b.set(41, 2.0);
        for n in [31, 36, 41, 46, 50] {
            b.note_on(n, 100);
        }
        assert_eq!(b.fretboard().0.iter().flatten().count(), 4, "Mono per String: up to four notes");
        let mut b = bass();
        b.note_on(40, 100);
        b.note_on(41, 100);
        b.note_on(42, 100);
        assert_eq!(b.held.iter().flatten().count(), 3, "Polyphonic");
    }

    #[test]
    fn keyswitches_and_articulations() {
        let mut b = bass();
        b.note_on(17, 100); // F-1: Dead Note
        assert_eq!(b.get(44), 5.0);
        b.note_on(40, 100);
        assert_eq!(b.last_played(), "Dead Note");
        play(&mut b, 0.1);
        let dead = rms(&play(&mut b, 0.3).0);
        b.note_on(12, 100);
        b.note_on(40, 100);
        play(&mut b, 0.1);
        let open = rms(&play(&mut b, 0.3).0);
        assert!(dead < open * 0.3, "a dead note is short: {dead} vs {open}");
        // harmonic: an octave up
        b.note_off(40);
        b.note_on(19, 100);
        b.note_on(40, 100);
        assert_eq!(b.strings.iter().find(|s| s.count > 0).unwrap().notes[0], (40, 52));
        // a hit makes a knock without a string
        let mut b = bass();
        b.note_on(22, 100);
        b.note_on(40, 100);
        assert_eq!(b.fretboard().0, [None; 4]);
        assert!(rms(&play(&mut b, 0.1).0) > 1e-4);
        // slide in: it starts below the pitch
        let mut b = bass();
        b.note_on(23, 100);
        b.note_on(40, 100);
        assert_eq!(b.strings.iter().find(|s| s.count > 0).unwrap().slide, -2.0);
        // the octave of the keyswitches moves
        b.set(45, 1.0);
        b.note_on(24, 100);
        assert_eq!(b.get(44), 0.0);
    }

    #[test]
    fn playing_styles_mute_and_pickups() {
        assert!(style_allowed(0, 2) && style_allowed(4, 2));
        assert!(!style_allowed(1, 2) && !style_allowed(5, 1));
        let open = level(&[], 40).0;
        let muted = level(&[(2, 1.0)], 40).0;
        assert!(muted < open * 0.6, "{muted} vs {open}");
        let pick = level(&[(1, 1.0)], 40).0;
        assert!((pick - open).abs() > open * 0.01);
        // the upright ignores Mute
        let up = level(&[(0, 5.0)], 40).0;
        let up_muted = level(&[(0, 5.0), (2, 1.0)], 40).0;
        assert!((up - up_muted).abs() < up * 0.01);
        // Rock: Stereo puts the neck pickup left and the bridge pickup right
        let (l, r) = level(&[(0, 2.0), (19, 3.0), (13, 0.0)], 40);
        assert!(r < l * 0.05, "bridge volume down: the right side is silent ({l} {r})");
        let (l, r) = level(&[(0, 2.0), (19, 0.0)], 40);
        assert!((l - r).abs() < l * 0.01, "Neck: the same on both sides");
        // Sixties: the switches turn the pickups off
        let both = level(&[(0, 1.0)], 40).0;
        let neck_off = level(&[(0, 1.0), (18, 0.0)], 40).0;
        assert!(neck_off < both * 0.8);
        // Modern: its EQ
        let flat = level(&[(0, 4.0)], 40).0;
        let bassy = level(&[(0, 4.0), (20, 12.0)], 40).0;
        assert!(bassy > flat * 1.3);
        // the upright's growl adds harmonics
        let plain = level(&[(0, 5.0), (22, 0.0)], 36).0;
        let growl = level(&[(0, 5.0), (22, 1.0), (23, 0.0)], 36).0;
        assert!((growl - plain).abs() > plain * 0.005, "{plain} {growl}");
    }

    #[test]
    fn noises_follow_their_sliders_and_the_main_one() {
        let hum = |main: f32, level: f32| {
            let mut b = StudioBassInst::with_samples(48000.0, false);
            b.set(5, main);
            b.set(34, level);
            rms(&play(&mut b, 0.3).0)
        };
        assert!(hum(1.0, 1.0) > 1e-4);
        assert_eq!(hum(0.0, 1.0), 0.0, "the Main view's Noises scales them all");
        assert_eq!(hum(1.0, 0.0), 0.0);
        // Definition: the attack's transient
        let attack = |def: f32| {
            let mut b = bass();
            b.set(3, def);
            b.note_on(40, 100);
            b.click
        };
        assert!(attack(1.0) > 0.1 && attack(0.0) == 0.0);
    }

    #[test]
    fn portamento_and_mute_controller() {
        assert_eq!(slide_time(0.0, false, 120.0).0, 0.0);
        assert!((slide_time(0.5, false, 120.0).0 - 0.25).abs() < 1e-6);
        let (secs, name) = slide_time(0.7, true, 120.0);
        assert_eq!(name, "1/4");
        assert!((secs - 0.5).abs() < 1e-6, "a quarter note at 120 bpm");
        let mut b = bass();
        b.set(30, 0.5);
        b.set(41, 1.0);
        b.note_on(45, 100);
        b.note_on(47, 100);
        let s = b.strings.iter().find(|s| s.count > 0).unwrap();
        assert_eq!(s.slide, -2.0, "it slides from the last note on the string");
        // the mute controller
        b.set(43, 20.0);
        b.controller(20, 127);
        assert_eq!(b.mute(), 1.0);
        b.set(0, 5.0);
        assert_eq!(b.mute(), 0.0, "not on the upright");
    }
}
