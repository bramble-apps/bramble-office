//! Animation state: keys morphing between layouts by id, press feedback (glass spring, fade,
//! invert), the "=" transition and small tweens. Times are egui input seconds.

use calc_core::calc::Seg;
use calc_core::layout::{KeyBox, Layout};
use egui::{pos2, Rect};
use std::collections::{HashMap, HashSet};

pub const MORPH: f64 = 0.3;
pub const FADE_IN: f64 = 0.25;
pub const FADE_OUT: f64 = 0.2;
pub const RELEASE: f64 = 0.25;
pub const EQ: f64 = 0.2;

pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::from_min_max(pos2(lerp(a.min.x, b.min.x, t), lerp(a.min.y, b.min.y, t)), pos2(lerp(a.max.x, b.max.x, t), lerp(a.max.y, b.max.y, t)))
}

fn progress(now: f64, t0: f64, dur: f64) -> f32 {
    (((now - t0) / dur) as f32).clamp(0.0, 1.0)
}

struct Slot {
    from: Rect,
    to: Rect,
    ff: f32,
    tf: f32,
    t0: f64,
    appear: bool,
}

/// Where a key is drawn now.
pub struct Shown {
    pub rect: Rect,
    pub font: f32,
    pub alpha: f32,
}

impl Slot {
    fn at(&self, now: f64) -> Shown {
        let p = ease_in_out(progress(now, self.t0, MORPH));
        let mut rect = lerp_rect(self.from, self.to, p);
        let mut alpha = 1.0;
        if self.appear {
            let q = ease_out(progress(now, self.t0, FADE_IN));
            alpha = q;
            rect = Rect::from_center_size(rect.center(), rect.size() * lerp(0.8, 1.0, q));
        }
        Shown { rect, font: lerp(self.ff, self.tf, p), alpha }
    }
    fn busy(&self, now: f64) -> bool {
        now - self.t0 < MORPH.max(FADE_IN)
    }
}

pub struct Leaving {
    pub key: KeyBox,
    pub rect: Rect,
    pub font: f32,
    pub t0: f64,
}

impl Leaving {
    pub fn alpha(&self, now: f64) -> f32 {
        1.0 - progress(now, self.t0, FADE_OUT)
    }
}

/// Keys keep their previous rects and glide to the new ones when the layout changes.
#[derive(Default)]
pub struct Morph {
    slots: HashMap<String, Slot>,
    pub leaving: Vec<Leaving>,
    last: Vec<KeyBox>,
    ready: bool,
}

fn same(a: Rect, b: Rect) -> bool {
    (a.min - b.min).length() < 0.25 && (a.max - b.max).length() < 0.25
}

impl Morph {
    /// `animate` false snaps (window drag-resizing, first frame).
    pub fn update(&mut self, lay: &Layout, now: f64, animate: bool) {
        let animate = animate && self.ready;
        let mut seen = HashSet::new();
        for k in &lay.keys {
            seen.insert(k.id.clone());
            let to = crate::draw::rect(k.rect);
            match self.slots.get_mut(&k.id) {
                Some(s) => {
                    if !same(s.to, to) || (s.tf - k.font).abs() > 0.05 {
                        if animate {
                            let cur = s.at(now);
                            s.from = cur.rect;
                            s.ff = cur.font;
                            s.t0 = now;
                            s.appear = false;
                        } else {
                            s.from = to;
                            s.ff = k.font;
                        }
                        s.to = to;
                        s.tf = k.font;
                    }
                }
                None => {
                    self.slots.insert(k.id.clone(), Slot { from: to, to, ff: k.font, tf: k.font, t0: now, appear: animate });
                }
            }
        }
        let gone: Vec<String> = self.slots.keys().filter(|id| !seen.contains(*id)).cloned().collect();
        for id in gone {
            if let Some(s) = self.slots.remove(&id) {
                if animate {
                    if let Some(k) = self.last_keys_get(&id) {
                        let cur = s.at(now);
                        self.leaving.push(Leaving { key: k, rect: cur.rect, font: cur.font, t0: now });
                    }
                }
            }
        }
        self.leaving.retain(|l| now - l.t0 < FADE_OUT);
        self.last = lay.keys.clone();
        self.ready = true;
    }

    fn last_keys_get(&self, id: &str) -> Option<KeyBox> {
        self.last.iter().find(|k| k.id == id).cloned()
    }

    pub fn shown(&self, id: &str, now: f64) -> Option<Shown> {
        self.slots.get(id).map(|s| s.at(now))
    }

    pub fn busy(&self, now: f64) -> bool {
        !self.leaving.is_empty() || self.slots.values().any(|s| s.busy(now))
    }
}

/// Key press feedback.
#[derive(Default)]
pub struct Press {
    /// the key held now and when it went down
    pub down: Option<(String, f64)>,
    released: HashMap<String, f64>,
    springs: HashMap<String, (f32, f32)>,
    /// a long-press already fired for the held key
    pub long_fired: bool,
}

impl Press {
    pub fn set_down(&mut self, id: Option<&str>, now: f64) {
        let cur = self.down.as_ref().map(|d| d.0.clone());
        if cur.as_deref() == id {
            return;
        }
        if let Some(c) = cur {
            self.released.insert(c, now);
        }
        self.down = id.map(|i| (i.to_string(), now));
        if let Some(i) = id {
            self.released.remove(i);
            self.springs.entry(i.to_string()).or_insert((1.0, 0.0));
        }
        self.long_fired = false;
    }

    pub fn is_down(&self, id: &str) -> bool {
        self.down.as_ref().is_some_and(|d| d.0 == id)
    }

    /// Steps the Liquid Glass springs (target 1.06 while held).
    pub fn step(&mut self, dt: f32) {
        let held = self.down.as_ref().map(|d| d.0.clone());
        let n = ((dt / (1.0 / 240.0)).ceil() as usize).clamp(1, 24);
        let h = dt / n as f32;
        for (id, (x, v)) in self.springs.iter_mut() {
            let target = if held.as_deref() == Some(id.as_str()) { 1.06 } else { 1.0 };
            for _ in 0..n {
                let a = -420.0 * (*x - target) - 22.0 * *v;
                *v += a * h;
                *x += *v * h;
            }
        }
        self.springs.retain(|id, (x, v)| held.as_deref() == Some(id.as_str()) || (*x - 1.0).abs() > 0.0005 || v.abs() > 0.001);
    }

    pub fn scale(&self, id: &str) -> f32 {
        self.springs.get(id).map(|s| s.0).unwrap_or(1.0)
    }

    /// Fade: 1 while held, easing back to 0 after release.
    pub fn mix(&self, id: &str, now: f64) -> f32 {
        if self.is_down(id) {
            return 1.0;
        }
        match self.released.get(id) {
            Some(t) => 1.0 - ease_out(progress(now, *t, RELEASE)),
            None => 0.0,
        }
    }

    pub fn busy(&mut self, now: f64) -> bool {
        self.released.retain(|_, t| now - *t < RELEASE);
        !self.springs.is_empty() || !self.released.is_empty()
    }
}

/// "=": the typed expression rises into the small line while the result fades in.
pub struct EqAnim {
    pub t0: f64,
    pub from: Vec<Seg>,
    pub from_size: f32,
}

impl EqAnim {
    pub fn p(&self, now: f64) -> f32 {
        ease_out(progress(now, self.t0, EQ))
    }
    pub fn done(&self, now: f64) -> bool {
        now - self.t0 >= EQ
    }
}

/// A value easing from one number to another.
#[derive(Default)]
pub struct Tween {
    from: f32,
    to: f32,
    t0: f64,
    set: bool,
}

impl Tween {
    pub fn go(&mut self, to: f32, now: f64, animate: bool) -> f32 {
        if !self.set || !animate {
            *self = Tween { from: to, to, t0: now, set: true };
        } else if (to - self.to).abs() > 0.01 {
            let cur = self.at(now);
            *self = Tween { from: cur, to, t0: now, set: true };
        }
        self.at(now)
    }
    pub fn at(&self, now: f64) -> f32 {
        lerp(self.from, self.to, ease_in_out(progress(now, self.t0, MORPH)))
    }
    pub fn busy(&self, now: f64) -> bool {
        now - self.t0 < MORPH
    }
}

/// Rects that glide to a new position at a constant size (toolbar buttons).
#[derive(Default)]
pub struct Glide {
    slots: HashMap<&'static str, (Rect, Rect, f64)>,
}

impl Glide {
    pub fn update(&mut self, id: &'static str, to: Rect, now: f64, animate: bool) {
        match self.slots.get_mut(id) {
            Some(s) if !same(s.1, to) => {
                let from = if animate { Self::pos(s, now) } else { to };
                *s = (Rect::from_center_size(from.center(), to.size()), to, now);
            }
            Some(_) => {}
            None => {
                self.slots.insert(id, (to, to, now));
            }
        }
    }
    fn pos(s: &(Rect, Rect, f64), now: f64) -> Rect {
        let p = ease_in_out(progress(now, s.2, MORPH));
        let c = s.0.center() + (s.1.center() - s.0.center()) * p;
        Rect::from_center_size(c, s.1.size())
    }
    /// Where `id` is drawn now (`fallback` when it has no slot yet).
    pub fn at(&self, id: &str, fallback: Rect, now: f64) -> Rect {
        self.slots.get(id).map(|s| Self::pos(s, now)).unwrap_or(fallback)
    }
    pub fn busy(&self, now: f64) -> bool {
        self.slots.values().any(|s| now - s.2 < MORPH)
    }
}
