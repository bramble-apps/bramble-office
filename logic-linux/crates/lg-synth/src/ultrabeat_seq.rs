//! Ultrabeat's step data: one `Cell` is the sequence of one drum sound in one pattern (up to 32
//! steps with trigger, accent, velocity and gate sections, and up to four lanes of per-step
//! parameter offsets). A cell packs into 64 small whole numbers ("words") so it can live in the
//! plug-in's parameter list and be saved with the project and with settings; an empty cell is
//! all zeros.

use crate::dsp::Rng;

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    /// Per step: bit 0 trigger, bit 1 accent, bits 2..5 inverted gate sections, bits 6..11
    /// velocity (relative to the default).
    pub steps: [u16; 32],
    /// Per lane: automated parameter number + 1 (0: unused).
    pub lane: [u8; 4],
    /// Per lane and step: offset −32…31 (steps of 1/31 of the parameter's range).
    pub offs: [[i8; 32]; 4],
    /// Bits 0..3: lane muted, bits 4..7: lane soloed.
    pub flags: u8,
}

impl Default for Cell {
    fn default() -> Cell {
        Cell { steps: [0; 32], lane: [0; 4], offs: [[0; 32]; 4], flags: 0 }
    }
}

const VEL_DEFAULT: i32 = 48;

impl Cell {
    pub fn is_empty(&self) -> bool {
        *self == Cell::default()
    }
    pub fn trig(&self, i: usize) -> bool {
        self.steps[i] & 1 != 0
    }
    pub fn set_trig(&mut self, i: usize, on: bool) {
        self.steps[i] = (self.steps[i] & !1) | on as u16;
    }
    pub fn accent(&self, i: usize) -> bool {
        self.steps[i] & 2 != 0
    }
    pub fn set_accent(&mut self, i: usize, on: bool) {
        self.steps[i] = (self.steps[i] & !2) | ((on as u16) << 1);
    }
    /// The active gate sections (bit k: section k of four).
    pub fn gate(&self, i: usize) -> u8 {
        (!(self.steps[i] >> 2) & 0xf) as u8
    }
    pub fn set_gate(&mut self, i: usize, mask: u8) {
        self.steps[i] = (self.steps[i] & !(0xf << 2)) | ((!(mask as u16) & 0xf) << 2);
    }
    /// The gate length as a fraction of the step.
    pub fn gate_len(&self, i: usize) -> f32 {
        (self.gate(i).count_ones().max(1)) as f32 / 4.0
    }
    /// Velocity 0…1 (the default step is 75 percent).
    pub fn vel(&self, i: usize) -> f32 {
        let rel = ((self.steps[i] >> 6) & 63) as i32;
        (((rel + VEL_DEFAULT) & 63) as f32 + 1.0) / 64.0
    }
    pub fn set_vel(&mut self, i: usize, v: f32) {
        let v6 = ((v.clamp(0.0, 1.0) * 64.0 - 1.0).round() as i32).clamp(0, 63);
        let rel = ((v6 - VEL_DEFAULT) & 63) as u16;
        self.steps[i] = (self.steps[i] & 0x3f) | (rel << 6);
    }
    /// Resets velocity and gate of every step to the default (triggers and accents stay).
    pub fn reset_vel_gate(&mut self) {
        for s in &mut self.steps {
            *s &= 3;
        }
    }
    pub fn lane_index(&self, k: u32) -> Option<usize> {
        self.lane.iter().position(|&l| l as u32 == k + 1)
    }
    pub fn count(&self, len: usize) -> usize {
        (0..len.min(32)).filter(|&i| self.trig(i)).count()
    }

    // ---- packing

    pub fn word(&self, w: usize) -> f32 {
        match w {
            0..=15 => (self.steps[2 * w] as u32 | (self.steps[2 * w + 1] as u32) << 12) as f32,
            16..=51 => {
                let l = (w - 16) / 9;
                let j = (w - 16) % 9;
                if j == 0 {
                    self.lane[l] as f32
                } else {
                    let mut x = 0u32;
                    for q in 0..4 {
                        x |= ((self.offs[l][(j - 1) * 4 + q] as u8 & 63) as u32) << (6 * q);
                    }
                    x as f32
                }
            }
            52 => self.flags as f32,
            _ => 0.0,
        }
    }
    pub fn set_word(&mut self, w: usize, v: f32) {
        let x = v.max(0.0) as u32;
        match w {
            0..=15 => {
                self.steps[2 * w] = (x & 0xfff) as u16;
                self.steps[2 * w + 1] = ((x >> 12) & 0xfff) as u16;
            }
            16..=51 => {
                let l = (w - 16) / 9;
                let j = (w - 16) % 9;
                if j == 0 {
                    self.lane[l] = x.min(255) as u8;
                } else {
                    for q in 0..4 {
                        let o = ((x >> (6 * q)) & 63) as u8;
                        self.offs[l][(j - 1) * 4 + q] = ((o << 2) as i8) >> 2;
                    }
                }
            }
            52 => self.flags = x as u8,
            _ => {}
        }
    }
    pub fn words(&self) -> [f32; 64] {
        let mut a = [0.0; 64];
        for (w, x) in a.iter_mut().enumerate() {
            *x = self.word(w);
        }
        a
    }
    pub fn from_words(a: &[f32]) -> Cell {
        let mut c = Cell::default();
        for (w, x) in a.iter().enumerate().take(64) {
            c.set_word(w, *x);
        }
        c
    }

    // ---- the Trigger shortcut menu

    pub fn clear_triggers(&mut self) {
        for i in 0..32 {
            self.set_trig(i, false);
        }
    }
    /// Adds triggers every `per` steps starting at `at` (existing ones stay).
    pub fn add_every(&mut self, len: usize, per: usize, at: usize) {
        let per = per.max(1);
        let mut i = at;
        while i < len.min(32) {
            self.set_trig(i, true);
            i += per;
        }
    }
    pub fn reverse(&mut self, len: usize) {
        let n = len.min(32);
        self.steps[..n].reverse();
        for l in 0..4 {
            self.offs[l][..n].reverse();
        }
    }
    /// Moves every step by `by` (positive: to the right), wrapping inside the length.
    pub fn shift(&mut self, len: usize, by: i32) {
        let n = len.clamp(1, 32);
        let k = by.rem_euclid(n as i32) as usize;
        self.steps[..n].rotate_right(k);
        for l in 0..4 {
            self.offs[l][..n].rotate_right(k);
        }
    }
    /// Puts the same number of triggers at new random places.
    pub fn alter_randomly(&mut self, len: usize, rng: &mut Rng) {
        let n = len.clamp(1, 32);
        let c = self.count(n);
        for i in 0..n {
            self.set_trig(i, false);
        }
        self.place_random(n, c, rng);
    }
    /// Erases the triggers and creates `count` new ones.
    pub fn create_randomly(&mut self, len: usize, count: usize, rng: &mut Rng) {
        let n = len.clamp(1, 32);
        for i in 0..n {
            self.set_trig(i, false);
        }
        self.place_random(n, count.min(n), rng);
    }
    fn place_random(&mut self, n: usize, count: usize, rng: &mut Rng) {
        let mut left = count;
        let mut guard = 0;
        while left > 0 && guard < 4000 {
            guard += 1;
            let i = (rng.next_u32() % n as u32) as usize;
            if !self.trig(i) {
                self.set_trig(i, true);
                left -= 1;
            }
        }
    }
    /// Alter Vel / Alter Gate: random changes to the values the steps already have.
    pub fn alter_vel(&mut self, len: usize, rng: &mut Rng) {
        for i in 0..len.min(32) {
            let v = (self.vel(i) + rng.bipolar() * 0.3).clamp(0.05, 1.0);
            self.set_vel(i, v);
        }
    }
    pub fn alter_gate(&mut self, len: usize, rng: &mut Rng) {
        for i in 0..len.min(32) {
            let c = (self.gate(i).count_ones() as i32 + (rng.bipolar() * 1.6).round() as i32).clamp(1, 4);
            self.set_gate(i, (1u8 << c) - 1);
        }
    }
    pub fn randomize_vel(&mut self, len: usize, rng: &mut Rng) {
        for i in 0..len.min(32) {
            self.set_vel(i, 0.1 + rng.unit() * 0.9);
        }
    }
    pub fn randomize_gate(&mut self, len: usize, rng: &mut Rng) {
        for i in 0..len.min(32) {
            let c = 1 + (rng.next_u32() % 4);
            self.set_gate(i, ((1u16 << c) - 1) as u8);
        }
    }

    // ---- step automation (the offset row)

    /// The lane of parameter `k`, made when it isn't there yet (None: all four lanes are used).
    pub fn lane_for(&mut self, k: u32) -> Option<usize> {
        if let Some(l) = self.lane_index(k) {
            return Some(l);
        }
        let l = self.lane.iter().position(|&x| x == 0)?;
        self.lane[l] = (k + 1) as u8;
        self.offs[l] = [0; 32];
        Some(l)
    }
    pub fn delete_lane(&mut self, l: usize) {
        self.lane[l] = 0;
        self.offs[l] = [0; 32];
        self.flags &= !((1 << l) | (1 << (l + 4)));
    }
    pub fn zero_lane(&mut self, l: usize) {
        self.offs[l] = [0; 32];
    }
    pub fn alter_lane(&mut self, l: usize, len: usize, rng: &mut Rng) {
        for i in 0..len.min(32) {
            self.offs[l][i] = (self.offs[l][i] as i32 + (rng.bipolar() * 10.0).round() as i32).clamp(-32, 31) as i8;
        }
    }
    pub fn randomize_lane(&mut self, l: usize, len: usize, rng: &mut Rng) {
        for i in 0..len.min(32) {
            self.offs[l][i] = ((rng.unit() * 63.0).round() as i32 - 32).clamp(-32, 31) as i8;
        }
    }
    /// Whether lane `l` is heard: not muted, and when some lane is soloed, only that one.
    pub fn lane_heard(&self, l: usize) -> bool {
        if self.lane[l] == 0 || self.flags & (1 << l) != 0 {
            return false;
        }
        let solo = self.flags >> 4;
        solo == 0 || solo & (1 << l) != 0
    }
}
