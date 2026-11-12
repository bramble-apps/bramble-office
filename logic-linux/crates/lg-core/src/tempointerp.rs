//! Logic's Tempo Interpreter: taps (Tap Tempo) steer the tempo. Each tap stands for one Tap
//! Step (a note value); the first Tap Count-In taps only set the tempo; after that a tap
//! counts when it falls inside the Window (ticks, 1/3840 note, around where the next step is
//! due; 0: every tap counts), the tempo follows it as fast as Tempo Response says, never by
//! more than Max. Tempo Change per tap, and Smoothing evens out jumps.

/// The window's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// Ticks (at 960 per quarter note) per tap: 960 = a quarter note.
    pub tap_step: u32,
    /// Ticks in 1/3840 notes (Logic's unit): taps further than half of it from the expected
    /// step are ignored. 0: all taps count.
    pub window: u32,
    /// 1 (slow) .. 8 (fast); Logic's default 4.
    pub response: u8,
    /// The largest change a tap may make, BPM.
    pub max_change: f64,
    /// Taps that only count in.
    pub count_in: u8,
    pub smoothing: bool,
    /// In record mode, accepted taps write tempo changes.
    pub recording: bool,
    /// The display shows every tap (Pre) or only accepted ones (Post).
    pub pre: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params { tap_step: 960, window: 480, response: 4, max_change: 20.0, count_in: 4, smoothing: true, recording: false, pre: true }
    }
}

/// The note values the Tap Step pop-up offers: (name, ticks at 960 PPQ).
pub const STEPS: [(&str, u32); 6] = [("1/1", 3840), ("1/2", 1920), ("1/4", 960), ("1/8", 480), ("1/16", 240), ("1/4 Triplet", 640)];

/// What a tap did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tap {
    /// Counting in (the n-th tap); the tempo once two are in.
    CountIn(u8, Option<f64>),
    /// Inside the window: the new tempo.
    Accepted(f64),
    /// Outside the window: ignored.
    Rejected,
}

#[derive(Clone, Debug, Default)]
pub struct Interpreter {
    taps: Vec<f64>,
    last: Option<f64>,
    pub tempo: f64,
    counted: u8,
    /// Recent tap results for the display: (seconds, accepted).
    pub shown: Vec<(f64, bool)>,
}

impl Interpreter {
    pub fn new(tempo: f64) -> Self {
        Interpreter { tempo, ..Default::default() }
    }

    pub fn reset(&mut self) {
        let t = self.tempo;
        *self = Interpreter::new(t);
    }

    /// Whether count-in is over and taps steer the tempo.
    pub fn following(&self, p: &Params) -> bool {
        self.counted >= p.count_in.max(1)
    }

    /// The seconds a tap step lasts at `bpm`.
    fn step_secs(p: &Params, bpm: f64) -> f64 {
        60.0 / bpm.max(1.0) * p.tap_step as f64 / 960.0
    }

    /// A tap at `t` seconds.
    pub fn tap(&mut self, p: &Params, t: f64) -> Tap {
        let res = self.tap_inner(p, t);
        let ok = !matches!(res, Tap::Rejected);
        if p.pre || ok {
            self.shown.push((t, ok));
            if self.shown.len() > 16 {
                self.shown.remove(0);
            }
        }
        res
    }

    fn tap_inner(&mut self, p: &Params, t: f64) -> Tap {
        if !self.following(p) {
            self.counted += 1;
            self.taps.push(t);
            self.last = Some(t);
            let n = self.taps.len();
            let bpm = (n >= 2).then(|| {
                let span = (self.taps[n - 1] - self.taps[0]) / (n - 1) as f64;
                60.0 / span.max(1e-3) * p.tap_step as f64 / 960.0
            });
            if let Some(b) = bpm {
                self.tempo = b.clamp(5.0, 990.0);
            }
            return Tap::CountIn(self.counted, bpm.map(|b| b.clamp(5.0, 990.0)));
        }
        let Some(last) = self.last else {
            self.last = Some(t);
            return Tap::Rejected;
        };
        let expected = Self::step_secs(p, self.tempo);
        let dt = t - last;
        // the error in 1/3840 notes (four per 960-PPQ tick)
        let err_ticks = (dt - expected).abs() / expected * p.tap_step as f64 * 4.0;
        if p.window > 0 && err_ticks > p.window as f64 / 2.0 {
            return Tap::Rejected;
        }
        self.last = Some(t);
        let measured = 60.0 / dt.max(1e-3) * p.tap_step as f64 / 960.0;
        let k = (p.response.clamp(1, 8) as f64) / 8.0;
        let mut target = self.tempo + (measured - self.tempo) * k;
        if p.smoothing {
            target = (target + self.tempo) / 2.0;
        }
        let d = (target - self.tempo).clamp(-p.max_change, p.max_change);
        self.tempo = (self.tempo + d).clamp(5.0, 990.0);
        Tap::Accepted(self.tempo)
    }

    /// With Window 0 Logic halts when the taps stop: true when no tap came for two steps.
    pub fn stalled(&self, p: &Params, now: f64) -> bool {
        p.window == 0 && self.following(p) && self.last.is_some_and(|l| now - l > 2.0 * Self::step_secs(p, self.tempo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_in_then_follows_within_the_window() {
        let p = Params::default();
        let mut i = Interpreter::new(120.0);
        // count-in at 100 BPM (0.6 s per quarter)
        for n in 0..4 {
            let r = i.tap(&p, n as f64 * 0.6);
            assert!(matches!(r, Tap::CountIn(..)));
        }
        assert!((i.tempo - 100.0).abs() < 0.01);
        assert!(i.following(&p));
        // a bit faster: accepted, the tempo moves toward it
        let r = i.tap(&p, 1.8 + 0.58);
        let Tap::Accepted(b) = r else { panic!("{r:?}") };
        assert!(b > 100.0 && b < 103.5);
        // far off (half a step late): rejected
        assert_eq!(i.tap(&p, 2.38 + 0.9), Tap::Rejected);
        assert_eq!(i.shown.len(), 6);
    }

    #[test]
    fn max_change_window_zero_and_stall() {
        let p = Params { window: 0, max_change: 2.0, smoothing: false, response: 8, count_in: 2, ..Default::default() };
        let mut i = Interpreter::new(120.0);
        i.tap(&p, 0.0);
        i.tap(&p, 0.5);
        assert!((i.tempo - 120.0).abs() < 0.01);
        // twice as fast: accepted (window 0), but only +2 BPM
        assert_eq!(i.tap(&p, 0.75), Tap::Accepted(122.0));
        assert!(!i.stalled(&p, 1.0));
        assert!(i.stalled(&p, 2.0));
        let post = Params { pre: false, ..Params::default() };
        let mut j = Interpreter::new(120.0);
        for n in 0..4 {
            j.tap(&post, n as f64 * 0.5);
        }
        j.tap(&post, 1.5 + 1.0);
        assert_eq!(j.shown.len(), 4);
    }
}
