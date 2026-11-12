//! Voice management shared by the instruments: polyphony with voice stealing, sustain pedal,
//! pitch bend, mod wheel and the usual MIDI controllers.

use crate::dsp::semis;

/// Live controller state every voice can read.
#[derive(Clone, Copy, Debug)]
pub struct Ctl {
    pub rate: f32,
    /// Current pitch bend in semitones (already scaled by the instrument's bend range).
    pub bend: f32,
    /// Frequency multiplier for `bend`.
    pub bend_ratio: f32,
    /// Mod wheel 0..1.
    pub mod_wheel: f32,
    /// Channel aftertouch 0..1.
    pub aftertouch: f32,
    /// CC74 brightness 0..1 (0.5 = neutral).
    pub brightness: f32,
    pub sustain: bool,
}

impl Ctl {
    pub fn new(rate: f32) -> Ctl {
        Ctl {
            rate,
            bend: 0.0,
            bend_ratio: 1.0,
            mod_wheel: 0.0,
            aftertouch: 0.0,
            brightness: 0.5,
            sustain: false,
        }
    }
}

/// One sounding note. `S` is the instrument's shared (derived) settings.
pub trait Voice<S>: Send {
    fn start(&mut self, note: u8, velocity: u8, shared: &S, ctl: &Ctl);
    /// Key released (after the sustain pedal is taken into account).
    fn release(&mut self, shared: &S, ctl: &Ctl);
    /// Fade out quickly (stolen); the voice must go idle within a few milliseconds.
    fn kill(&mut self, rate: f32);
    /// Adds the voice into `l`/`r`.
    fn render(&mut self, l: &mut [f32], r: &mut [f32], shared: &S, ctl: &Ctl);
    fn active(&self) -> bool;
    /// Rough current loudness, to pick which voice to steal.
    fn level(&self) -> f32;
    /// Called when the sustain pedal state changes while the key is still down (pianos use it).
    fn pedal(&mut self, _down: bool, _shared: &S, _ctl: &Ctl) {}
}

#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    note: u8,
    held: bool,
    sustained: bool,
    released: bool,
    killed: bool,
    age: u64,
}

/// A fixed pool of voices (allocated once). `max` voices may sound; the pool holds extra slots
/// so a stolen voice can fade out while its replacement starts.
pub struct Poly<V> {
    pub voices: Vec<V>,
    slots: Vec<Slot>,
    pub max: usize,
    counter: u64,
    /// When true a new note on a key that is still sounding fades the old voice out.
    pub retrigger_kills: bool,
    /// The next note-ons add voices to the key (layers, unison) instead of replacing it.
    pub stacking: bool,
    /// Which notes to keep when the polyphony is exceeded: 0 Newest (steal the oldest),
    /// 1 Oldest (drop the new note), 2 Lowest, 3 Highest.
    pub priority: u8,
}

impl<V> Poly<V> {
    pub fn new(max: usize, mut make: impl FnMut() -> V) -> Poly<V> {
        let n = max + max / 2 + 4;
        Poly {
            voices: (0..n).map(|_| make()).collect(),
            slots: vec![Slot::default(); n],
            max,
            counter: 0,
            retrigger_kills: true,
            stacking: false,
            priority: 0,
        }
    }

    pub fn set_max(&mut self, max: usize) {
        let cap = self.voices.len().saturating_sub(4).max(1);
        self.max = max.clamp(1, cap);
    }

    pub fn note_on<S>(&mut self, note: u8, vel: u8, shared: &S, ctl: &Ctl)
    where
        V: Voice<S>,
    {
        self.counter += 1;
        // A key still sounding: let the old voice go.
        for i in 0..self.voices.len() {
            if self.stacking {
                break;
            }
            if self.voices[i].active() && self.slots[i].note == note && !self.slots[i].killed {
                if self.retrigger_kills {
                    self.voices[i].kill(ctl.rate);
                    self.slots[i].killed = true;
                } else {
                    self.voices[i].release(shared, ctl);
                    self.slots[i].released = true;
                }
                self.slots[i].held = false;
                self.slots[i].sustained = false;
            }
        }
        // Over the polyphony: steal (oldest released first, then quietest/oldest held).
        let live = (0..self.voices.len())
            .filter(|&i| self.voices[i].active() && !self.slots[i].killed)
            .count();
        if live >= self.max && self.priority != 0 && !(0..self.voices.len()).any(|i| self.voices[i].active() && !self.slots[i].killed && self.slots[i].released) {
            // all voices held: keep the oldest / lowest / highest notes
            let held = (0..self.voices.len()).filter(|&i| self.voices[i].active() && !self.slots[i].killed);
            let victim = match self.priority {
                2 => held.max_by_key(|&i| self.slots[i].note).filter(|&i| self.slots[i].note > note),
                3 => held.min_by_key(|&i| self.slots[i].note).filter(|&i| self.slots[i].note < note),
                _ => None,
            };
            match victim {
                Some(i) => {
                    self.voices[i].kill(ctl.rate);
                    self.slots[i].killed = true;
                }
                None => return,
            }
        } else if live >= self.max {
            let mut best: Option<usize> = None;
            let mut best_score = f32::MAX;
            for i in 0..self.voices.len() {
                if !self.voices[i].active() || self.slots[i].killed {
                    continue;
                }
                let s = &self.slots[i];
                // released voices are cheaper to steal; within a class, older and quieter first
                let class = if s.released {
                    0.0
                } else if s.sustained {
                    1.0
                } else {
                    2.0
                };
                let score = class * 1e6 + (s.age as f32) * 1e-3 + self.voices[i].level() * 10.0;
                if score < best_score {
                    best_score = score;
                    best = Some(i);
                }
            }
            if let Some(i) = best {
                self.voices[i].kill(ctl.rate);
                self.slots[i].killed = true;
            }
        }
        // A free slot, or the quietest dying one.
        let idx = (0..self.voices.len())
            .find(|&i| !self.voices[i].active())
            .unwrap_or_else(|| {
                let mut b = 0;
                let mut lv = f32::MAX;
                for i in 0..self.voices.len() {
                    let l = self.voices[i].level() + if self.slots[i].killed { 0.0 } else { 100.0 };
                    if l < lv {
                        lv = l;
                        b = i;
                    }
                }
                b
            });
        self.voices[idx].start(note, vel, shared, ctl);
        self.slots[idx] = Slot {
            note,
            held: true,
            sustained: false,
            released: false,
            killed: false,
            age: self.counter,
        };
    }

    /// Mono / legato: moves the sounding voice of `from` to `note` without a new voice (the
    /// voice decides from `shared` whether to retrigger). False when nothing is sounding.
    pub fn legato<S>(&mut self, from: u8, note: u8, vel: u8, shared: &S, ctl: &Ctl) -> bool
    where
        V: Voice<S>,
    {
        let Some(i) = (0..self.voices.len()).find(|&i| self.voices[i].active() && !self.slots[i].killed && !self.slots[i].released && self.slots[i].note == from) else {
            return false;
        };
        self.counter += 1;
        self.voices[i].start(note, vel, shared, ctl);
        self.slots[i].note = note;
        self.slots[i].held = true;
        self.slots[i].sustained = false;
        true
    }

    pub fn note_off<S>(&mut self, note: u8, shared: &S, ctl: &Ctl)
    where
        V: Voice<S>,
    {
        for i in 0..self.voices.len() {
            let s = &mut self.slots[i];
            if s.held && s.note == note && !s.killed {
                s.held = false;
                if ctl.sustain {
                    s.sustained = true;
                } else {
                    s.released = true;
                    self.voices[i].release(shared, ctl);
                }
            }
        }
    }

    pub fn sustain<S>(&mut self, down: bool, shared: &S, ctl: &Ctl)
    where
        V: Voice<S>,
    {
        for i in 0..self.voices.len() {
            if !self.voices[i].active() || self.slots[i].killed {
                continue;
            }
            self.voices[i].pedal(down, shared, ctl);
            if !down && self.slots[i].sustained {
                self.slots[i].sustained = false;
                self.slots[i].released = true;
                self.voices[i].release(shared, ctl);
            }
        }
    }

    pub fn all_notes_off<S>(&mut self, shared: &S, ctl: &Ctl)
    where
        V: Voice<S>,
    {
        for i in 0..self.voices.len() {
            if self.voices[i].active() && !self.slots[i].released {
                self.voices[i].release(shared, ctl);
                self.slots[i].released = true;
                self.slots[i].held = false;
                self.slots[i].sustained = false;
            }
        }
    }

    pub fn render<S>(&mut self, l: &mut [f32], r: &mut [f32], shared: &S, ctl: &Ctl)
    where
        V: Voice<S>,
    {
        for v in &mut self.voices {
            if v.active() {
                v.render(l, r, shared, ctl);
            }
        }
    }

    pub fn active_count<S>(&self) -> usize
    where
        V: Voice<S>,
    {
        self.voices.iter().filter(|v| v.active()).count()
    }

    /// Notes currently held (key down) — for mono/legato instruments and the organ.
    pub fn is_held(&self, note: u8) -> bool {
        self.slots.iter().any(|s| s.held && s.note == note)
    }
}

/// Controller handling common to all instruments (volume, pan, expression, bend, wheel).
#[derive(Clone, Copy, Debug)]
pub struct Channel {
    pub ctl: Ctl,
    pub bend_up: f32,
    pub bend_down: f32,
    pub volume: f32,
    pub expression: f32,
    pub pan: f32,
    bend_raw: i16,
}

impl Channel {
    pub fn new(rate: f32) -> Channel {
        Channel {
            ctl: Ctl::new(rate),
            bend_up: 2.0,
            bend_down: 2.0,
            volume: 1.0,
            expression: 1.0,
            pan: 0.0,
            bend_raw: 0,
        }
    }
    pub fn set_bend(&mut self, v: i16) {
        self.bend_raw = v;
        self.update_bend();
    }
    pub fn update_bend(&mut self) {
        let x = self.bend_raw as f32 / 8192.0;
        let st = if x >= 0.0 {
            x * self.bend_up
        } else {
            x * self.bend_down
        };
        self.ctl.bend = st;
        self.ctl.bend_ratio = semis(st);
    }
    /// Returns Some(sustain state) when CC64 changed the pedal.
    pub fn cc(&mut self, cc: u8, value: u8) -> Option<bool> {
        let v = value as f32 / 127.0;
        match cc {
            1 => self.ctl.mod_wheel = v,
            7 => self.volume = v * v,
            10 => self.pan = v * 2.0 - 1.0,
            11 => self.expression = v,
            74 => self.ctl.brightness = v,
            64 => {
                let down = value >= 64;
                if down != self.ctl.sustain {
                    self.ctl.sustain = down;
                    return Some(down);
                }
            }
            _ => {}
        }
        None
    }
    /// Output gains (left, right) from volume/expression/pan (constant-power pan).
    pub fn out_gains(&self) -> (f32, f32) {
        let g = self.volume * self.expression;
        let p = (self.pan + 1.0) * 0.25 * std::f32::consts::PI;
        (
            g * p.cos() * std::f32::consts::SQRT_2,
            g * p.sin() * std::f32::consts::SQRT_2,
        )
    }
    pub fn reset(&mut self) {
        let r = self.ctl.rate;
        let (u, d) = (self.bend_up, self.bend_down);
        *self = Channel::new(r);
        self.bend_up = u;
        self.bend_down = d;
    }
}

/// Parameter storage over a static table of `Param`s (ids may be sparse).
pub struct Params {
    pub defs: &'static [lg_dsp::Param],
    values: Vec<f32>,
    index: Vec<i32>,
}

impl Params {
    pub fn new(defs: &'static [lg_dsp::Param]) -> Params {
        let max_id = defs.iter().map(|p| p.id).max().unwrap_or(0) as usize;
        let mut index = vec![-1; max_id + 1];
        for (i, p) in defs.iter().enumerate() {
            index[p.id as usize] = i as i32;
        }
        Params {
            defs,
            values: defs.iter().map(|p| p.default).collect(),
            index,
        }
    }
    pub fn get(&self, id: u32) -> f32 {
        match self.index.get(id as usize) {
            Some(&i) if i >= 0 => self.values[i as usize],
            _ => 0.0,
        }
    }
    /// A parameter's definition by id.
    pub fn def(&self, id: u32) -> Option<&lg_dsp::Param> {
        match self.index.get(id as usize) {
            Some(&i) if i >= 0 => Some(&self.defs[i as usize]),
            _ => None,
        }
    }
    /// Sets (clamped); returns false for an unknown id.
    pub fn set(&mut self, id: u32, v: f32) -> bool {
        match self.index.get(id as usize) {
            Some(&i) if i >= 0 => {
                let p = &self.defs[i as usize];
                let v = if v.is_finite() {
                    v.clamp(p.min, p.max)
                } else {
                    p.default
                };
                self.values[i as usize] = v;
                true
            }
            _ => false,
        }
    }
    pub fn reset_defaults(&mut self) {
        for (i, p) in self.defs.iter().enumerate() {
            self.values[i] = p.default;
        }
    }
}
