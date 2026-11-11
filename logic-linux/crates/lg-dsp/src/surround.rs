//! Surround and Spatial Audio, as Logic mixes them: the ten surround formats with their channel
//! orders and speaker positions, the meters' channel orders, the Surround Panner (angle,
//! diversity, elevation, spread, Center and LFE levels, separation, speakers turned off) and
//! Surround Balancer, the 3D Object Panner's objects, and the renderer the Dolby Atmos plug-in
//! runs: bed + objects to a monitoring format (7.1.4 … 2.0) with Logic's downmix algorithms and
//! trims, or to two binaural channels (our own head model: interaural delay, head shadow, near /
//! mid / far distance). All our own maths: no Dolby or Apple code.
//!
//! Positions: azimuth in degrees (0 front centre, positive to the right, ±180 behind), elevation
//! in degrees (0 ear level, 90 overhead). Objects use Dolby's room coordinates: x −1 (left) to 1
//! (right), y −1 (back) to 1 (front), z 0 (ear level) to 1 (ceiling).

use std::f32::consts::{FRAC_1_SQRT_2, PI};

/// One speaker channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ch {
    L,
    C,
    R,
    Lc,
    Rc,
    Lm,
    Rm,
    Ls,
    Rs,
    S,
    Lt,
    Rt,
    Lts,
    Rts,
    Ltm,
    Rtm,
    Lfe,
}

impl Ch {
    pub const ALL: [Ch; 17] = [Ch::L, Ch::C, Ch::R, Ch::Lc, Ch::Rc, Ch::Lm, Ch::Rm, Ch::Ls, Ch::Rs, Ch::S, Ch::Lt, Ch::Rt, Ch::Lts, Ch::Rts, Ch::Ltm, Ch::Rtm, Ch::Lfe];

    /// Logic's short label (meters, channel buttons, file extensions).
    pub fn label(self) -> &'static str {
        match self {
            Ch::L => "L",
            Ch::C => "C",
            Ch::R => "R",
            Ch::Lc => "Lc",
            Ch::Rc => "Rc",
            Ch::Lm => "Lm",
            Ch::Rm => "Rm",
            Ch::Ls => "Ls",
            Ch::Rs => "Rs",
            Ch::S => "S",
            Ch::Lt => "Lt",
            Ch::Rt => "Rt",
            Ch::Lts => "Lts",
            Ch::Rts => "Rts",
            Ch::Ltm => "Ltm",
            Ch::Rtm => "Rtm",
            Ch::Lfe => "LFE",
        }
    }

    /// The speaker's name (I/O Assignments).
    pub fn name(self) -> &'static str {
        match self {
            Ch::L => "Left",
            Ch::C => "Center",
            Ch::R => "Right",
            Ch::Lc => "Left center",
            Ch::Rc => "Right center",
            Ch::Lm => "Left mid",
            Ch::Rm => "Right mid",
            Ch::Ls => "Left surround",
            Ch::Rs => "Right surround",
            Ch::S => "Surround",
            Ch::Lt => "Left top",
            Ch::Rt => "Right top",
            Ch::Lts => "Left top surround",
            Ch::Rts => "Right top surround",
            Ch::Ltm => "Left top mid",
            Ch::Rtm => "Right top mid",
            Ch::Lfe => "LFE",
        }
    }

    pub fn from_label(s: &str) -> Option<Ch> {
        Ch::ALL.into_iter().find(|c| c.label().eq_ignore_ascii_case(s.trim()))
    }

    /// An overhead (height) speaker.
    pub fn height(self) -> bool {
        matches!(self, Ch::Lt | Ch::Rt | Ch::Lts | Ch::Rts | Ch::Ltm | Ch::Rtm)
    }

    /// Logic's default Bounce Extension for split files (".L", ".Ls", ".LFE").
    pub fn ext(self) -> String {
        format!(".{}", self.label())
    }

    /// The left/right twin (L ↔ R, Ls ↔ Rs…); centre channels are their own.
    pub fn mirror(self) -> Ch {
        match self {
            Ch::L => Ch::R,
            Ch::R => Ch::L,
            Ch::Lc => Ch::Rc,
            Ch::Rc => Ch::Lc,
            Ch::Lm => Ch::Rm,
            Ch::Rm => Ch::Lm,
            Ch::Ls => Ch::Rs,
            Ch::Rs => Ch::Ls,
            Ch::Lt => Ch::Rt,
            Ch::Rt => Ch::Lt,
            Ch::Lts => Ch::Rts,
            Ch::Rts => Ch::Lts,
            Ch::Ltm => Ch::Rtm,
            Ch::Rtm => Ch::Ltm,
            c => c,
        }
    }
}

/// A project surround format (File ▸ Project Settings ▸ Audio ▸ General ▸ Surround Format).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Format {
    Quad,
    Lcrs,
    #[default]
    S51,
    S61,
    S71,
    Sdds,
    S512,
    S514,
    S712,
    S714,
}

impl Format {
    /// In the order of Logic's Surround Format menu.
    pub const ALL: [Format; 10] = [Format::Quad, Format::Lcrs, Format::S51, Format::S61, Format::S71, Format::Sdds, Format::S512, Format::S514, Format::S712, Format::S714];
    /// The bed formats a Spatial Audio with Dolby Atmos project allows.
    pub const ATMOS_BEDS: [Format; 3] = [Format::S51, Format::S71, Format::S712];

    /// The menu's name.
    pub fn name(self) -> &'static str {
        match self {
            Format::Quad => "Quadraphonic",
            Format::Lcrs => "LCRS (Pro Logic)",
            Format::S51 => "5.1 (ITU 775)",
            Format::S61 => "6.1 (ES/EX)",
            Format::S71 => "7.1",
            Format::Sdds => "7.1 (SDDS)",
            Format::S512 => "5.1.2",
            Format::S514 => "5.1.4",
            Format::S712 => "7.1.2",
            Format::S714 => "7.1.4",
        }
    }

    /// Short name ("5.1", "Quad") for plug-in format menus and region badges.
    pub fn short(self) -> &'static str {
        match self {
            Format::Quad => "Quad",
            Format::Lcrs => "LCRS",
            Format::S51 => "5.1",
            Format::S61 => "6.1",
            Format::S71 => "7.1",
            Format::Sdds => "7.1 SDDS",
            Format::S512 => "5.1.2",
            Format::S514 => "5.1.4",
            Format::S712 => "7.1.2",
            Format::S714 => "7.1.4",
        }
    }

    pub fn from_name(s: &str) -> Option<Format> {
        let s = s.trim();
        Format::ALL.into_iter().find(|f| f.name() == s || f.short() == s)
    }

    /// The channels in Logic's order (the order of an interleaved surround file and the bus).
    pub fn channels(self) -> &'static [Ch] {
        use Ch::*;
        match self {
            Format::Quad => &[L, R, Ls, Rs],
            Format::Lcrs => &[L, C, R, S],
            Format::S51 => &[L, C, R, Ls, Rs, Lfe],
            Format::S61 => &[L, C, R, Ls, S, Rs, Lfe],
            Format::S71 => &[L, C, R, Lm, Rm, Ls, Rs, Lfe],
            Format::Sdds => &[L, Lc, C, Rc, R, Ls, Rs, Lfe],
            Format::S512 => &[L, C, R, Ls, Rs, Ltm, Rtm, Lfe],
            Format::S514 => &[L, C, R, Ls, Rs, Lt, Rt, Lts, Rts, Lfe],
            Format::S712 => &[L, C, R, Lm, Rm, Ls, Rs, Ltm, Rtm, Lfe],
            Format::S714 => &[L, C, R, Lm, Rm, Ls, Rs, Lt, Rt, Lts, Rts, Lfe],
        }
    }

    pub fn n(self) -> usize {
        self.channels().len()
    }

    pub fn index(self, c: Ch) -> Option<usize> {
        self.channels().iter().position(|x| *x == c)
    }

    pub fn has_height(self) -> bool {
        self.channels().iter().any(|c| c.height())
    }

    pub fn has_lfe(self) -> bool {
        self.index(Ch::Lfe).is_some()
    }

    /// The format whose channels are exactly `chans` (any order), if one is.
    pub fn of_channels(chans: &[Ch]) -> Option<Format> {
        Format::ALL.into_iter().find(|f| f.n() == chans.len() && chans.iter().all(|c| f.index(*c).is_some()))
    }
}

/// Where a speaker sits: (azimuth, elevation) in degrees, for this format (5.1's surrounds sit at
/// ±110°, 7.1's at ±135° behind the side speakers at ±90°).
pub fn position(f: Format, c: Ch) -> (f32, f32) {
    let side = matches!(f, Format::S71 | Format::S712 | Format::S714);
    match c {
        Ch::L => (if f == Format::Quad { -45.0 } else { -30.0 }, 0.0),
        Ch::R => (if f == Format::Quad { 45.0 } else { 30.0 }, 0.0),
        Ch::C => (0.0, 0.0),
        Ch::Lc => (-15.0, 0.0),
        Ch::Rc => (15.0, 0.0),
        Ch::Lm => (-90.0, 0.0),
        Ch::Rm => (90.0, 0.0),
        Ch::Ls => (if side || f == Format::Quad { -135.0 } else { -110.0 }, 0.0),
        Ch::Rs => (if side || f == Format::Quad { 135.0 } else { 110.0 }, 0.0),
        Ch::S => (180.0, 0.0),
        Ch::Lt => (-45.0, 45.0),
        Ch::Rt => (45.0, 45.0),
        Ch::Lts => (-135.0, 45.0),
        Ch::Rts => (135.0, 45.0),
        Ch::Ltm => (-90.0, 60.0),
        Ch::Rtm => (90.0, 60.0),
        Ch::Lfe => (0.0, -30.0),
    }
}

/// A speaker in Dolby's room coordinates (x, y, z), for rendering objects.
pub fn room_xyz(f: Format, c: Ch) -> [f32; 3] {
    match c {
        Ch::L => [-1.0, 1.0, 0.0],
        Ch::R => [1.0, 1.0, 0.0],
        Ch::C => [0.0, 1.0, 0.0],
        Ch::Lc => [-0.5, 1.0, 0.0],
        Ch::Rc => [0.5, 1.0, 0.0],
        Ch::Lm => [-1.0, 0.0, 0.0],
        Ch::Rm => [1.0, 0.0, 0.0],
        Ch::Ls | Ch::Rs => {
            let x = if c == Ch::Ls { -1.0 } else { 1.0 };
            // (without side speakers the surrounds sit a little forward of the back wall)
            if matches!(f, Format::S71 | Format::S712 | Format::S714 | Format::Quad) { [x, -1.0, 0.0] } else { [x, -0.6, 0.0] }
        }
        Ch::S => [0.0, -1.0, 0.0],
        Ch::Lt => [-1.0, 0.5, 1.0],
        Ch::Rt => [1.0, 0.5, 1.0],
        Ch::Lts => [-1.0, -0.5, 1.0],
        Ch::Rts => [1.0, -0.5, 1.0],
        Ch::Ltm => [-1.0, 0.0, 1.0],
        Ch::Rtm => [1.0, 0.0, 1.0],
        Ch::Lfe => [0.0, 0.0, -1.0],
    }
}

// ---- meters ---------------------------------------------------------------------------------

/// Settings ▸ View ▸ Mixer ▸ Level Meters ▸ Channel Order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MeterOrder {
    Dts,
    Smpte,
    #[default]
    Clockwise,
    Film,
}

impl MeterOrder {
    pub const ALL: [MeterOrder; 4] = [MeterOrder::Dts, MeterOrder::Smpte, MeterOrder::Clockwise, MeterOrder::Film];

    /// From the Settings pop-up's text ("DTS (L R Ls Rs C LFE)").
    pub fn from_setting(s: &str) -> MeterOrder {
        let s = s.trim_start();
        if s.starts_with("DTS") {
            MeterOrder::Dts
        } else if s.starts_with("SMPTE") {
            MeterOrder::Smpte
        } else if s.starts_with("Film") {
            MeterOrder::Film
        } else {
            MeterOrder::Clockwise
        }
    }
}

/// The meters' channel order for a format.
pub fn meter_order(f: Format, o: MeterOrder) -> Vec<Ch> {
    use Ch::*;
    let seq: &[Ch] = match o {
        MeterOrder::Dts => &[L, Lc, R, Rc, Ls, Rs, S, Lm, Rm, C, Lfe, Lt, Rt, Ltm, Rtm, Lts, Rts],
        MeterOrder::Smpte => &[L, R, C, Lfe, Ls, Rs, S, Lc, Rc, Lm, Rm, Lt, Rt, Ltm, Rtm, Lts, Rts],
        MeterOrder::Clockwise => &[Ls, Lm, L, Lc, C, Rc, R, Rm, Rs, S, Lfe, Lts, Ltm, Lt, Rt, Rtm, Rts],
        MeterOrder::Film => &[L, Lc, C, Rc, R, Lm, Rm, Ls, Rs, S, Lfe, Lt, Rt, Ltm, Rtm, Lts, Rts],
    };
    seq.iter().copied().filter(|c| f.index(*c).is_some()).collect()
}

// ---- the Surround Panner ---------------------------------------------------------------------

/// A Surround Panner (or, on a surround-input strip, a Surround Balancer) setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pan {
    /// Degrees, −180..180 (0: front centre).
    pub angle: f32,
    /// 0 (one or two adjacent speakers) to 1 (super mono: all speakers).
    pub diversity: f32,
    /// 0 (ear level) to 90 degrees (only the height speakers); formats with height only.
    pub elevation: f32,
    /// Stereo strips: degrees between left and right (0 mono, negative swaps them).
    pub spread: f32,
    /// Center and LFE level offsets, +6 to −82 dB (below −82: off).
    pub center_db: f32,
    pub lfe_db: f32,
    /// Separation (non-height formats, mono and stereo input): on, XF, XR, Y (1.0 = none).
    pub separation: bool,
    pub sep_xf: f32,
    pub sep_xr: f32,
    pub sep_y: f32,
    /// Speakers turned off in the panner window, as bits of `Ch as u32`.
    pub off: u32,
    /// The window's Spherical view (the puck's distance from the centre sets the elevation).
    pub spherical: bool,
    /// Surround Balancer: how far the level leans toward the puck (0 = unchanged).
    pub amount: f32,
}

pub const LEVEL_OFF: f32 = -82.0;

impl Default for Pan {
    fn default() -> Pan {
        Pan { angle: 0.0, diversity: 0.0, elevation: 0.0, spread: 60.0, center_db: 0.0, lfe_db: f32::NEG_INFINITY, separation: false, sep_xf: 1.0, sep_xr: 1.0, sep_y: 1.0, off: 0, spherical: false, amount: 0.0 }
    }
}

fn level(db: f32) -> f32 {
    if db <= LEVEL_OFF { 0.0 } else { 10f32.powf(db.min(6.0) / 20.0) }
}

/// Smallest signed difference between two angles in degrees (−180..180).
pub fn angle_diff(a: f32, b: f32) -> f32 {
    let mut d = (a - b) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    }
    if d < -180.0 {
        d += 360.0;
    }
    d
}

impl Pan {
    pub fn is_off(&self, c: Ch) -> bool {
        self.off & (1 << c as u32) != 0
    }

    pub fn toggle(&mut self, c: Ch) {
        self.off ^= 1 << c as u32;
    }

    /// The puck's position in the field (x right, y front, radius ≤ 1).
    pub fn puck(&self) -> (f32, f32) {
        let r = if self.spherical { 1.0 - self.elevation / 90.0 } else { 1.0 - self.diversity };
        let a = self.angle.to_radians();
        (a.sin() * r, a.cos() * r)
    }

    /// Sets angle and diversity (Planar) or elevation (Spherical) from a puck position;
    /// `lock` keeps diversity (1) or the angle (2) as Command / Control-Command drags do.
    pub fn set_puck(&mut self, x: f32, y: f32, lock: u8, has_height: bool) {
        let r = (x * x + y * y).sqrt().min(1.0);
        if lock != 2 && r > 1e-4 {
            self.angle = x.atan2(y).to_degrees();
        }
        if lock != 1 {
            if self.spherical && has_height {
                self.elevation = (1.0 - r) * 90.0;
            } else {
                self.diversity = 1.0 - r;
            }
        }
    }
}

/// Pairwise constant-power panning at `angle` over the given speakers (azimuths), as power.
fn pair_power(speakers: &[(usize, f32)], angle: f32, out: &mut [f32]) {
    match speakers.len() {
        0 => {}
        1 => out[speakers[0].0] += 1.0,
        _ => {
            let mut s: Vec<(usize, f32)> = speakers.to_vec();
            s.sort_by(|a, b| a.1.total_cmp(&b.1));
            // the pair of neighbours (going round) the angle falls between
            for k in 0..s.len() {
                let (a, b) = (s[k], s[(k + 1) % s.len()]);
                let span = {
                    let d = (b.1 - a.1).rem_euclid(360.0);
                    if d == 0.0 { 360.0 } else { d }
                };
                let off = (angle - a.1).rem_euclid(360.0);
                if off <= span + 1e-4 {
                    let t = (off / span).clamp(0.0, 1.0) * PI / 2.0;
                    out[a.0] += t.cos().powi(2);
                    out[b.0] += t.sin().powi(2);
                    return;
                }
            }
        }
    }
}

/// The Surround Panner's gain per channel of `f` (Logic's channel order) for a mono source at
/// `angle` (the panner's angle, or one side of a stereo spread).
fn mono_at(f: Format, p: &Pan, angle: f32) -> Vec<f32> {
    let chans = f.channels();
    let mut ear: Vec<(usize, f32)> = Vec::new();
    let mut top: Vec<(usize, f32)> = Vec::new();
    for (k, c) in chans.iter().enumerate() {
        if *c == Ch::Lfe || p.is_off(*c) || (*c == Ch::C && level(p.center_db) == 0.0) {
            continue;
        }
        let az = position(f, *c).0;
        if c.height() { top.push((k, az)) } else { ear.push((k, az)) }
    }
    let n = chans.len();
    let layer = |set: &[(usize, f32)], include_all: &[(usize, f32)]| -> Vec<f32> {
        let mut pow = vec![0f32; n];
        pair_power(set, angle, &mut pow);
        let d = p.diversity.clamp(0.0, 1.0);
        if d > 0.0 && !include_all.is_empty() {
            let u = 1.0 / include_all.len() as f32;
            for x in pow.iter_mut() {
                *x *= 1.0 - d;
            }
            for (k, _) in include_all {
                pow[*k] += d * u;
            }
        }
        pow
    };
    let has_top = !top.is_empty();
    let e = if has_top { p.elevation.clamp(0.0, 90.0).to_radians() } else { 0.0 };
    let mut all: Vec<(usize, f32)> = ear.clone();
    if has_top {
        // (Planar: a centred puck also feeds the height speakers)
        all.extend(top.iter().copied());
    }
    let ear_pow = if ear.is_empty() { vec![0.0; n] } else { layer(&ear, &all) };
    let top_pow = if has_top { layer(&top, &top) } else { vec![0.0; n] };
    let (ce, se) = (e.cos().powi(2), e.sin().powi(2));
    let mut g: Vec<f32> = (0..n).map(|k| (ce * ear_pow[k] + se * top_pow[k]).sqrt()).collect();
    if let Some(c) = f.index(Ch::C) {
        g[c] *= level(p.center_db);
    }
    if p.separation && !f.has_height() {
        separate(f, p, &mut g);
    }
    g
}

/// Separation XF / XR / Y: cross-feeds left with right (front, rear) and front with rear; at
/// 0.00 both sides get the same signal. 7.1's mid channels take the mean of XF and XR.
fn separate(f: Format, p: &Pan, g: &mut [f32]) {
    let mut mix = |a: Ch, b: Ch, s: f32| {
        if let (Some(i), Some(j)) = (f.index(a), f.index(b)) {
            let (x, y) = (g[i], g[j]);
            let (k1, k2) = ((1.0 + s) / 2.0, (1.0 - s) / 2.0);
            g[i] = k1 * x + k2 * y;
            g[j] = k2 * x + k1 * y;
        }
    };
    mix(Ch::L, Ch::R, p.sep_xf.clamp(0.0, 1.0));
    mix(Ch::Ls, Ch::Rs, p.sep_xr.clamp(0.0, 1.0));
    mix(Ch::Lm, Ch::Rm, ((p.sep_xf + p.sep_xr) / 2.0).clamp(0.0, 1.0));
    let y = p.sep_y.clamp(0.0, 1.0);
    mix(Ch::L, Ch::Ls, y);
    mix(Ch::R, Ch::Rs, y);
    mix(Ch::C, Ch::S, y);
}

/// A mono strip's Surround Panner: gain per channel (LFE included).
pub fn mono_gains(f: Format, p: &Pan) -> Vec<f32> {
    let mut g = mono_at(f, p, p.angle);
    if let Some(k) = f.index(Ch::Lfe) {
        g[k] = level(p.lfe_db);
    }
    g
}

/// A stereo strip's Surround Panner: gains for the left and the right input (each placed at
/// angle ∓ spread / 2; a negative spread swaps them). The LFE gets both at half each.
pub fn stereo_gains(f: Format, p: &Pan) -> (Vec<f32>, Vec<f32>) {
    let half = p.spread.clamp(-180.0, 180.0) / 2.0;
    let mut l = mono_at(f, p, p.angle - half);
    let mut r = mono_at(f, p, p.angle + half);
    if let Some(k) = f.index(Ch::Lfe) {
        l[k] = level(p.lfe_db) * 0.5;
        r[k] = level(p.lfe_db) * 0.5;
    }
    (l, r)
}

/// The Surround Balancer (surround input): each channel passes to its own output, only its level
/// changes. Amount leans toward the puck's angle; Elevation above 0 lowers the ear-level
/// channels; speakers turned off are muted.
pub fn balancer_gains(f: Format, p: &Pan) -> Vec<f32> {
    let a = p.amount.clamp(0.0, 1.0);
    let ear = if f.has_height() { (1.0 - p.elevation.clamp(0.0, 90.0) / 90.0).sqrt() } else { 1.0 };
    f.channels()
        .iter()
        .map(|c| {
            if p.is_off(*c) {
                return 0.0;
            }
            if *c == Ch::Lfe {
                return 1.0;
            }
            let d = angle_diff(position(f, *c).0, p.angle).to_radians();
            let lean = (1.0 - a) + a * (1.0 + d.cos()) / 2.0;
            let g = lean * if c.height() { 1.0 } else { ear };
            if *c == Ch::C { g * level(p.center_db) } else { g }
        })
        .collect()
}

// ---- 3D objects -------------------------------------------------------------------------------

/// An object's position (the 3D Object Panner): x −1..1 left/right, y −1..1 back/front, z 0..1
/// ear level/ceiling, size 0..1, spread 0..1 (stereo objects: how far apart L and R sit).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Object {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub size: f32,
    pub spread: f32,
}

impl Default for Object {
    fn default() -> Object {
        // (Option-click resets to centre front)
        Object { x: 0.0, y: 1.0, z: 0.0, size: 0.0, spread: 0.5 }
    }
}

impl Object {
    /// The two objects of a stereo track: left and right of the centre puck by the spread.
    pub fn stereo(&self) -> (Object, Object) {
        let half = self.spread.clamp(-1.0, 1.0);
        let l = Object { x: (self.x - half).clamp(-1.0, 1.0), ..*self };
        let r = Object { x: (self.x + half).clamp(-1.0, 1.0), ..*self };
        (l, r)
    }

    /// Direction from the listener: (azimuth, elevation) in degrees.
    pub fn direction(&self) -> (f32, f32) {
        let az = self.x.atan2(self.y).to_degrees();
        let horiz = (self.x * self.x + self.y * self.y).sqrt();
        let el = self.z.atan2(horiz.max(1e-3)).to_degrees();
        (if horiz < 1e-3 { 0.0 } else { az }, if horiz < 1e-3 && self.z > 0.0 { 90.0 } else { el })
    }
}

/// An object rendered on a speaker layout: gain per channel of `f` (never the LFE). Speakers
/// nearer the object get more (power normalised); Size spreads it over more speakers.
pub fn object_gains(f: Format, o: &Object) -> Vec<f32> {
    let chans = f.channels();
    let sharp = 3.0 * (1.0 - o.size.clamp(0.0, 1.0)).powi(2);
    let p = [o.x.clamp(-1.0, 1.0), o.y.clamp(-1.0, 1.0), o.z.clamp(0.0, 1.0)];
    let mut w: Vec<f32> = chans
        .iter()
        .map(|c| {
            if *c == Ch::Lfe {
                return 0.0;
            }
            let s = room_xyz(f, *c);
            // (a speaker above an ear-level object is further than one beside it)
            let d2 = (s[0] - p[0]).powi(2) + (s[1] - p[1]).powi(2) + 2.0 * (s[2] - p[2]).powi(2);
            (1.0 / (d2 + 0.01)).powf(sharp)
        })
        .collect();
    let sum: f32 = w.iter().sum();
    if sum > 0.0 {
        for x in w.iter_mut() {
            *x = (*x / sum).sqrt();
        }
    }
    w
}

/// An object on a stereo pair: constant power by x, wider with size.
pub fn object_stereo(o: &Object) -> (f32, f32) {
    let x = o.x.clamp(-1.0, 1.0) * (1.0 - 0.7 * o.size.clamp(0.0, 1.0));
    let t = (x + 1.0) * PI / 4.0;
    (t.cos(), t.sin())
}

// ---- the Dolby Atmos plug-in's renderer ------------------------------------------------------

/// The Dolby Atmos plug-in's Monitoring Format menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Monitoring {
    /// Binaural (our head model); the render modes apply.
    #[default]
    DolbyRenderer,
    /// Headphone virtualization in the manner of Apple's (Music / Movie modes).
    AppleRenderer,
    BuiltInSpeakers,
    DisplaySpeakers,
    Direct20,
    Downmix20,
    M51,
    M71,
    M512,
    M514,
    M714,
}

impl Monitoring {
    pub const ALL: [Monitoring; 11] = [Monitoring::DolbyRenderer, Monitoring::AppleRenderer, Monitoring::BuiltInSpeakers, Monitoring::DisplaySpeakers, Monitoring::Direct20, Monitoring::Downmix20, Monitoring::M51, Monitoring::M71, Monitoring::M512, Monitoring::M514, Monitoring::M714];

    pub fn name(self) -> &'static str {
        match self {
            Monitoring::DolbyRenderer => "Dolby Renderer",
            Monitoring::AppleRenderer => "Apple Renderer",
            Monitoring::BuiltInSpeakers => "Renderer for Built-in Speakers",
            Monitoring::DisplaySpeakers => "Renderer for Display Speakers",
            Monitoring::Direct20 => "2.0 Direct",
            Monitoring::Downmix20 => "2.0 from 5.1 Downmix",
            Monitoring::M51 => "5.1",
            Monitoring::M71 => "7.1",
            Monitoring::M512 => "5.1.2",
            Monitoring::M514 => "5.1.4",
            Monitoring::M714 => "7.1.4",
        }
    }

    pub fn from_name(s: &str) -> Option<Monitoring> {
        Monitoring::ALL.into_iter().find(|m| m.name() == s.trim())
    }

    /// Two binaural channels (headphones).
    pub fn binaural(self) -> bool {
        matches!(self, Monitoring::DolbyRenderer | Monitoring::AppleRenderer)
    }

    /// The speaker format rendered to (None: two channels, L and R).
    pub fn format(self) -> Option<Format> {
        match self {
            Monitoring::M51 => Some(Format::S51),
            Monitoring::M71 => Some(Format::S71),
            Monitoring::M512 => Some(Format::S512),
            Monitoring::M514 => Some(Format::S514),
            Monitoring::M714 => Some(Format::S714),
            _ => None,
        }
    }

    /// The output channels.
    pub fn layout(self) -> Vec<Ch> {
        self.format().map_or(vec![Ch::L, Ch::R], |f| f.channels().to_vec())
    }

    /// Which trim layout (Downmix & Trim's Channel Layout buttons) applies, if any.
    pub fn trim_layout(self) -> Option<usize> {
        match self {
            Monitoring::Direct20 => Some(0),
            Monitoring::Downmix20 | Monitoring::M51 => Some(1),
            Monitoring::M512 => Some(2),
            Monitoring::M514 => Some(3),
            Monitoring::M71 => Some(4),
            _ => None,
        }
    }
}

/// Downmix & Trim ▸ 5.1 and 5.1.x.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Down51 {
    #[default]
    LoRo,
    ProLogicIIx,
    DirectRoomBalance,
    Direct,
}

impl Down51 {
    pub const ALL: [Down51; 4] = [Down51::LoRo, Down51::ProLogicIIx, Down51::DirectRoomBalance, Down51::Direct];
    pub fn name(self) -> &'static str {
        match self {
            Down51::LoRo => "Lo/Ro",
            Down51::ProLogicIIx => "Dolby Pro Logic IIx",
            Down51::DirectRoomBalance => "Direct render with room balance",
            Down51::Direct => "Direct render",
        }
    }
}

/// Downmix & Trim ▸ 5.1 to 2.0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Down20 {
    #[default]
    LoRo,
    LtRt,
    LtRtPhase90,
}

impl Down20 {
    pub const ALL: [Down20; 3] = [Down20::LoRo, Down20::LtRt, Down20::LtRtPhase90];
    pub fn name(self) -> &'static str {
        match self {
            Down20::LoRo => "Lo/Ro",
            Down20::LtRt => "Lt/Rt (Dolby Pro Logic II)",
            Down20::LtRtPhase90 => "Lt/Rt (Dolby Pro Logic II) w/ Phase 90",
        }
    }
}

/// One Channel Layout's trims (applied before the downmix).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trim {
    pub manual: bool,
    /// dB, −∞..0 (−40 shown as −∞).
    pub surround_db: f32,
    pub height_db: f32,
    /// Front/back balance of the height and the ear-level content, −1 (back) .. 1 (front).
    pub overhead: f32,
    pub listener: f32,
}

/// The Channel Layout buttons.
pub const TRIM_LAYOUTS: [&str; 5] = ["2.0 Direct", "5.1 & 2.0", "5.1.2", "5.1.4", "7.1"];

impl Trim {
    /// Automatic mode's values for a layout (dimmed in the pane).
    pub fn automatic(layout: usize) -> Trim {
        let (s, h) = match layout {
            0 => (-3.0, -3.0),
            1 => (0.0, -3.0),
            4 => (0.0, -3.0),
            _ => (0.0, 0.0),
        };
        Trim { manual: false, surround_db: s, height_db: h, overhead: 0.0, listener: 0.0 }
    }

    /// What applies: the manual values, else Automatic's.
    pub fn effective(&self, layout: usize) -> Trim {
        if self.manual { *self } else { Trim::automatic(layout) }
    }
}

impl Default for Trim {
    fn default() -> Trim {
        Trim::automatic(1)
    }
}

/// Binaural render mode (Dolby Renderer only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum BinMode {
    Off,
    Near,
    #[default]
    Mid,
    Far,
}

impl BinMode {
    pub const ALL: [BinMode; 4] = [BinMode::Off, BinMode::Near, BinMode::Mid, BinMode::Far];
    pub fn name(self) -> &'static str {
        match self {
            BinMode::Off => "Off",
            BinMode::Near => "Near",
            BinMode::Mid => "Mid",
            BinMode::Far => "Far",
        }
    }
    pub fn from_name(s: &str) -> BinMode {
        BinMode::ALL.into_iter().find(|m| m.name() == s).unwrap_or_default()
    }
}

/// What the renderer is asked to do.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderCfg {
    pub monitoring: Monitoring,
    pub down51: Down51,
    pub down20: Down20,
    pub trims: [Trim; 5],
    /// Apple Renderer: Movie Mode (a roomier virtualization) rather than Music Mode.
    pub movie_mode: bool,
}

impl Default for RenderCfg {
    fn default() -> RenderCfg {
        RenderCfg { monitoring: Monitoring::M714, down51: Down51::LoRo, down20: Down20::LoRo, trims: [0, 1, 2, 3, 4].map(Trim::automatic), movie_mode: false }
    }
}

/// One object's signal for a block, where it is and its binaural mode.
pub struct ObjectIn<'a> {
    pub signal: &'a [f32],
    pub pos: Object,
    pub mode: BinMode,
}

/// The fold of a 7.1.4 mix into a smaller speaker format: (from, to, gain) entries.
fn fold_714(to: Format, cfg: &RenderCfg) -> Vec<(Ch, Ch, f32)> {
    use Ch::*;
    let db = |d: f32| 10f32.powf(d / 20.0);
    let mut m: Vec<(Ch, Ch, f32)> = Vec::new();
    let has = |c: Ch| to.index(c).is_some();
    for c in [L, C, R, Lfe] {
        m.push((c, c, 1.0));
    }
    // side and rear surrounds
    if has(Lm) {
        for c in [Lm, Rm, Ls, Rs] {
            m.push((c, c, 1.0));
        }
    } else {
        let (side, rear) = match cfg.down51 {
            Down51::ProLogicIIx => (db(-1.2), db(-6.2)),
            _ => (1.0, 1.0),
        };
        m.push((Lm, Ls, side));
        m.push((Rm, Rs, side));
        m.push((Ls, Ls, rear));
        m.push((Rs, Rs, rear));
    }
    // heights
    if has(Lt) {
        for c in [Lt, Rt, Lts, Rts] {
            m.push((c, c, 1.0));
        }
    } else if has(Ltm) {
        m.push((Lt, Ltm, FRAC_1_SQRT_2));
        m.push((Lts, Ltm, FRAC_1_SQRT_2));
        m.push((Rt, Rtm, FRAC_1_SQRT_2));
        m.push((Rts, Rtm, FRAC_1_SQRT_2));
    } else {
        // (no overhead speakers: the heights fold to the nearest ear-level channels)
        m.push((Lt, L, 1.0));
        m.push((Rt, R, 1.0));
        m.push((Lts, Ls, 1.0));
        m.push((Rts, Rs, 1.0));
    }
    m
}

/// Coefficients of a 90° phase-difference network (two allpass chains whose outputs are in
/// quadrature over most of the audio band).
struct Hilbert;

impl Hilbert {
    const A: [f32; 4] = [0.692_387_8, 0.936_065_43, 0.988_229_5, 0.998_748_8];
    const B: [f32; 4] = [0.402_192_1, 0.856_171_1, 0.972_290_95, 0.995_288_5];
}

#[derive(Clone, Default)]
struct Allpass2 {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Allpass2 {
    fn run(&mut self, c2: f32, x: f32) -> f32 {
        let y = c2 * (x + self.y2) - self.x2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

#[derive(Clone, Default)]
struct Quadrature {
    a: [Allpass2; 4],
    b: [Allpass2; 4],
    b_delay: f32,
}

impl Quadrature {
    fn run_a(&mut self, x: f32) -> f32 {
        let mut a = x;
        for (s, c) in self.a.iter_mut().zip(Hilbert::A) {
            a = s.run(c * c, a);
        }
        // (this chain is taken one sample later, as the classic design has it)
        let out = self.b_delay;
        self.b_delay = a;
        out
    }

    fn run_b(&mut self, x: f32) -> f32 {
        let mut b = x;
        for (s, c) in self.b.iter_mut().zip(Hilbert::B) {
            b = s.run(c * c, b);
        }
        b
    }

    #[cfg(test)]
    fn step(&mut self, x: f32) -> (f32, f32) {
        (self.run_a(x), self.run_b(x))
    }
}

/// One virtual speaker (or object) heard over headphones: per-ear delay and head shadow.
#[derive(Clone)]
struct Ear {
    buf: Vec<f32>,
    w: usize,
    /// delay in samples (fractional) for this ear
    delay: f32,
    /// head-shadow filter (one pole, one zero), state
    b0: f32,
    b1: f32,
    a1: f32,
    x1: f32,
    y1: f32,
    /// a gentle low-pass for sources behind (pinna shadow)
    back: f32,
    lp: f32,
}

impl Ear {
    fn new() -> Ear {
        Ear { buf: vec![0.0; 128], w: 0, delay: 0.0, b0: 1.0, b1: 0.0, a1: 0.0, x1: 0.0, y1: 0.0, back: 0.0, lp: 0.0 }
    }

    /// Sets up the ear for a source at (azimuth, elevation) degrees; `right` ear or left.
    fn aim(&mut self, az: f32, el: f32, right: bool, rate: f32) {
        const HEAD: f32 = 0.0875;
        const C: f32 = 343.0;
        let (a, e) = (az.to_radians(), el.to_radians());
        // lateral angle toward this ear, −90..90°
        let lat = (a.sin() * e.cos()).clamp(-1.0, 1.0).asin();
        let toward = if right { lat } else { -lat };
        // Woodworth: the far ear hears it later
        let itd = HEAD / C * (toward.abs() + toward.abs().sin());
        self.delay = if toward < 0.0 { itd * rate } else { 0.0 };
        // Brown–Duda head shadow: angle between the source and this ear's axis
        let ear_az: f32 = if right { PI / 2.0 } else { -PI / 2.0 };
        let cosang = (a.sin() * e.cos()) * ear_az.sin() + 0.0;
        let theta = cosang.clamp(-1.0, 1.0).acos();
        let alpha = 1.05 + 0.95 * (theta * 180.0 / 150.0).cos();
        let w0 = C / HEAD;
        // bilinear transform of (alpha s + w0) / (s + w0) with prewarp-free k = 2 rate
        let k = 2.0 * rate;
        let a0 = k + w0;
        self.b0 = (alpha * k + w0) / a0;
        self.b1 = (w0 - alpha * k) / a0;
        self.a1 = (w0 - k) / a0;
        // behind the head: a little darker
        self.back = ((a.cos() < 0.0) as u8 as f32) * (-a.cos()).min(1.0) * 0.5 * e.cos();
    }

    fn run(&mut self, x: f32) -> f32 {
        let n = self.buf.len();
        self.buf[self.w] = x;
        let d = self.delay.clamp(0.0, (n - 2) as f32);
        let i = d.floor() as usize;
        let fr = d - i as f32;
        let a = self.buf[(self.w + n - i) % n];
        let b = self.buf[(self.w + n - i - 1) % n];
        self.w = (self.w + 1) % n;
        let v = a + (b - a) * fr;
        let y = self.b0 * v + self.b1 * self.x1 - self.a1 * self.y1;
        self.x1 = v;
        self.y1 = y;
        self.lp += 0.35 * (y - self.lp);
        y * (1.0 - self.back) + self.lp * self.back
    }
}

/// A small room for a binaural distance: early reflections per ear (decorrelated), damped.
#[derive(Clone)]
struct Room {
    buf: Vec<f32>,
    w: usize,
    taps: Vec<(usize, f32, f32)>,
    damp: f32,
    lp: [f32; 2],
}

impl Room {
    fn new(mode: BinMode, rate: f32, roomy: f32) -> Room {
        let (amt, spread_ms) = match mode {
            BinMode::Off => (0.0, 0.0),
            BinMode::Near => (0.18, 9.0),
            BinMode::Mid => (0.32, 16.0),
            BinMode::Far => (0.5, 26.0),
        };
        let amt = amt * roomy;
        let ms = |m: f32| (m * rate / 1000.0) as usize;
        let shape = [(0.37, 0.9, 0.2), (0.53, 0.25, 0.85), (0.71, 0.7, 0.4), (0.89, 0.3, 0.75), (1.13, 0.6, 0.5), (1.31, 0.45, 0.6), (1.6, 0.5, 0.45), (1.9, 0.4, 0.5)];
        let taps = if amt == 0.0 { Vec::new() } else { shape.iter().enumerate().map(|(k, (t, l, r))| (ms(spread_ms * t) + 1, amt * l * 0.8f32.powi(k as i32), amt * r * 0.8f32.powi(k as i32))).collect() };
        let len = taps.iter().map(|t| t.0).max().unwrap_or(1) + 2;
        Room { buf: vec![0.0; len], w: 0, taps, damp: if mode == BinMode::Far { 0.25 } else { 0.4 }, lp: [0.0; 2] }
    }

    fn run(&mut self, x: f32) -> (f32, f32) {
        if self.taps.is_empty() {
            return (0.0, 0.0);
        }
        let n = self.buf.len();
        self.buf[self.w] = x;
        let (mut l, mut r) = (0.0, 0.0);
        for (d, gl, gr) in &self.taps {
            let v = self.buf[(self.w + n - d) % n];
            l += v * gl;
            r += v * gr;
        }
        self.w = (self.w + 1) % n;
        self.lp[0] += self.damp * (l - self.lp[0]);
        self.lp[1] += self.damp * (r - self.lp[1]);
        (self.lp[0], self.lp[1])
    }
}

/// Headphone rendering of a set of speaker positions at one distance mode.
#[derive(Clone)]
struct Binaural {
    ears: Vec<[Ear; 2]>,
    room: Room,
    mode: BinMode,
}

impl Binaural {
    fn new(pos: Vec<(f32, f32)>, mode: BinMode, rate: f32, roomy: f32) -> Binaural {
        let ears = pos
            .iter()
            .map(|&(az, el)| {
                let mut l = Ear::new();
                let mut r = Ear::new();
                l.aim(az, el, false, rate);
                r.aim(az, el, true, rate);
                [l, r]
            })
            .collect();
        Binaural { ears, room: Room::new(mode, rate, roomy), mode }
    }

    /// Adds the binaural rendering of `chans` (one per position) into `l`, `r`.
    fn run(&mut self, chans: &[&[f32]], l: &mut [f32], r: &mut [f32]) {
        let direct = match self.mode {
            BinMode::Near => 1.12,
            BinMode::Far => 0.75,
            _ => 1.0,
        };
        for f in 0..l.len() {
            let mut sum = 0.0;
            for (k, ch) in chans.iter().enumerate() {
                let x = ch.get(f).copied().unwrap_or(0.0);
                sum += x;
                let e = &mut self.ears[k];
                l[f] += e[0].run(x) * direct;
                r[f] += e[1].run(x) * direct;
            }
            let (a, b) = self.room.run(sum * 0.5);
            l[f] += a;
            r[f] += b;
        }
    }
}

/// The 7.1.4 layout the renderer works in.
pub const FULL: Format = Format::S714;

/// The Dolby Atmos plug-in's renderer (and Spatial Audio Monitoring's, with no objects).
pub struct Renderer {
    rate: f32,
    full: Vec<Vec<f32>>,
    bin: Vec<(BinMode, bool, Binaural)>,
    quad: [Quadrature; 2],
    /// 5.1 scratch for 2.0 from 5.1.
    s51: Vec<Vec<f32>>,
}

impl Renderer {
    pub fn new(rate: f32) -> Renderer {
        Renderer { rate, full: Vec::new(), bin: Vec::new(), quad: Default::default(), s51: Vec::new() }
    }

    pub fn reset(&mut self) {
        self.bin.clear();
        self.quad = Default::default();
    }

    /// Bed channels (in `bed`'s order) placed in the 7.1.4 layout: (channel, 7.1.4 index, gain).
    fn bed_map(bed: Format) -> Vec<(usize, usize, f32)> {
        let mut v = Vec::new();
        for (k, c) in bed.channels().iter().enumerate() {
            let targets: Vec<(Ch, f32)> = match c {
                Ch::Ltm => vec![(Ch::Lt, FRAC_1_SQRT_2), (Ch::Lts, FRAC_1_SQRT_2)],
                Ch::Rtm => vec![(Ch::Rt, FRAC_1_SQRT_2), (Ch::Rts, FRAC_1_SQRT_2)],
                Ch::Lc => vec![(Ch::L, 0.5f32.sqrt()), (Ch::C, 0.5f32.sqrt())],
                Ch::Rc => vec![(Ch::R, 0.5f32.sqrt()), (Ch::C, 0.5f32.sqrt())],
                Ch::S => vec![(Ch::Ls, FRAC_1_SQRT_2), (Ch::Rs, FRAC_1_SQRT_2)],
                c => vec![(*c, 1.0)],
            };
            for (t, g) in targets {
                if let Some(i) = FULL.index(t) {
                    v.push((k, i, g));
                }
            }
        }
        v
    }

    /// Renders the bed (`bed_fmt` channels) and the objects to the monitoring format; returns the
    /// output channels (`cfg.monitoring.layout()`), each `frames` long. `bed_modes` are the bed
    /// channels' binaural modes (LFE is always Off).
    pub fn render(&mut self, cfg: &RenderCfg, bed_fmt: Format, bed: &[Vec<f32>], bed_modes: &[BinMode], objects: &[ObjectIn], frames: usize) -> Vec<Vec<f32>> {
        let mon = cfg.monitoring;
        let layout = mon.layout();
        let mut out = vec![vec![0f32; frames]; layout.len()];
        if mon.binaural() || matches!(mon, Monitoring::BuiltInSpeakers | Monitoring::DisplaySpeakers) {
            self.render_binaural(cfg, bed_fmt, bed, bed_modes, objects, frames, &mut out);
            return out;
        }
        // speakers: everything in 7.1.4 first, objects either there or straight on the target
        let target = mon.format();
        let direct = matches!(cfg.down51, Down51::Direct | Down51::DirectRoomBalance) && matches!(mon, Monitoring::M51 | Monitoring::M512 | Monitoring::M514 | Monitoring::Downmix20) || mon == Monitoring::Direct20;
        self.full.resize_with(FULL.n(), Vec::new);
        for c in self.full.iter_mut() {
            c.clear();
            c.resize(frames, 0.0);
        }
        for (k, i, g) in Self::bed_map(bed_fmt) {
            if let Some(src) = bed.get(k) {
                for f in 0..frames.min(src.len()) {
                    self.full[i][f] += src[f] * g;
                }
            }
        }
        if !direct {
            for o in objects {
                let g = object_gains(FULL, &o.pos);
                for (i, gi) in g.iter().enumerate() {
                    if *gi > 1e-5 {
                        for f in 0..frames.min(o.signal.len()) {
                            self.full[i][f] += o.signal[f] * gi;
                        }
                    }
                }
            }
        }
        let trim_layout = mon.trim_layout();
        if let Some(t) = trim_layout {
            Self::apply_trim(&mut self.full, &cfg.trims[t].effective(t));
        }
        match mon {
            Monitoring::M714 => {
                for (o, s) in out.iter_mut().zip(self.full.iter()) {
                    o.copy_from_slice(s);
                }
            }
            Monitoring::M51 | Monitoring::M71 | Monitoring::M512 | Monitoring::M514 => {
                let to = target.unwrap_or(Format::S51);
                Self::fold(&self.full, to, cfg, &mut out);
                if direct {
                    Self::objects_on(to, objects, cfg.down51 == Down51::DirectRoomBalance, &mut out);
                }
            }
            Monitoring::Downmix20 => {
                self.s51.resize_with(6, Vec::new);
                for c in self.s51.iter_mut() {
                    c.clear();
                    c.resize(frames, 0.0);
                }
                let mut s51 = std::mem::take(&mut self.s51);
                Self::fold(&self.full, Format::S51, cfg, &mut s51);
                if direct {
                    Self::objects_on(Format::S51, objects, cfg.down51 == Down51::DirectRoomBalance, &mut s51);
                }
                self.to_stereo(&s51, cfg.down20, &mut out);
                self.s51 = s51;
            }
            Monitoring::Direct20 => {
                // the bed folded Lo/Ro, the objects panned straight onto the pair
                let mut s51 = vec![vec![0f32; frames]; 6];
                Self::fold(&self.full, Format::S51, cfg, &mut s51);
                self.to_stereo(&s51, Down20::LoRo, &mut out);
                for o in objects {
                    let (gl, gr) = object_stereo(&o.pos);
                    for f in 0..frames.min(o.signal.len()) {
                        out[0][f] += o.signal[f] * gl;
                        out[1][f] += o.signal[f] * gr;
                    }
                }
            }
            _ => {}
        }
        out
    }

    fn objects_on(to: Format, objects: &[ObjectIn], room_balance: bool, out: &mut [Vec<f32>]) {
        for o in objects {
            let mut pos = o.pos;
            if room_balance && pos.y < 0.0 {
                // (between the room's midpoint and the back: kept nearer the rear speakers)
                pos.y = -(-pos.y).sqrt();
            }
            let g = object_gains(to, &pos);
            for (i, gi) in g.iter().enumerate() {
                if *gi > 1e-5 {
                    for f in 0..out[i].len().min(o.signal.len()) {
                        out[i][f] += o.signal[f] * gi;
                    }
                }
            }
        }
    }

    fn apply_trim(full: &mut [Vec<f32>], t: &Trim) {
        let db = |d: f32| if d <= -40.0 { 0.0 } else { 10f32.powf(d / 20.0) };
        let (sur, hgt) = (db(t.surround_db), db(t.height_db));
        // balance: right (positive) favours the front speakers, left the back ones
        let bal = |b: f32, front: bool| if front { (1.0 + b.min(0.0)).max(0.0) } else { (1.0 - b.max(0.0)).max(0.0) };
        for (k, c) in FULL.channels().iter().enumerate() {
            let mut g = 1.0;
            if matches!(c, Ch::Lm | Ch::Rm | Ch::Ls | Ch::Rs) {
                g *= sur;
            }
            if c.height() {
                g *= hgt * bal(t.overhead, matches!(c, Ch::Lt | Ch::Rt));
            } else if *c != Ch::Lfe {
                g *= bal(t.listener, matches!(c, Ch::L | Ch::C | Ch::R));
            }
            if g != 1.0 {
                for x in full[k].iter_mut() {
                    *x *= g;
                }
            }
        }
    }

    fn fold(full: &[Vec<f32>], to: Format, cfg: &RenderCfg, out: &mut [Vec<f32>]) {
        for (a, b, g) in fold_714(to, cfg) {
            if let (Some(i), Some(j)) = (FULL.index(a), to.index(b)) {
                for (o, x) in out[j].iter_mut().zip(full[i].iter()) {
                    *o += x * g;
                }
            }
        }
    }

    /// 5.1 (L C R Ls Rs LFE) to two channels.
    fn to_stereo(&mut self, s: &[Vec<f32>], how: Down20, out: &mut [Vec<f32>]) {
        let c = FRAC_1_SQRT_2;
        let n = out[0].len();
        for f in 0..n {
            let (l, cc, r, ls, rs) = (s[0][f], s[1][f], s[2][f], s[3][f], s[4][f]);
            let (lo, ro) = match how {
                Down20::LoRo => (l + c * cc + c * ls, r + c * cc + c * rs),
                Down20::LtRt => {
                    let sur = c * (ls + rs);
                    (l + c * cc - sur, r + c * cc + sur)
                }
                Down20::LtRtPhase90 => {
                    // front through one network, surrounds through the other (90° apart)
                    let fl = self.quad[0].run_a(l + c * cc);
                    let fr = self.quad[1].run_a(r + c * cc);
                    let sl = self.quad[0].run_b(-0.8718 * ls - 0.4899 * rs);
                    let sr = self.quad[1].run_b(0.4899 * ls + 0.8718 * rs);
                    (fl + sl, fr + sr)
                }
            };
            out[0][f] += lo;
            out[1][f] += ro;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_binaural(&mut self, cfg: &RenderCfg, bed_fmt: Format, bed: &[Vec<f32>], bed_modes: &[BinMode], objects: &[ObjectIn], frames: usize, out: &mut [Vec<f32>]) {
        let dolby = cfg.monitoring == Monitoring::DolbyRenderer;
        // Apple-style and speaker virtualizers: one room for everything (no render modes)
        let roomy = match cfg.monitoring {
            Monitoring::AppleRenderer => if cfg.movie_mode { 1.6 } else { 1.0 },
            Monitoring::BuiltInSpeakers | Monitoring::DisplaySpeakers => 0.6,
            _ => 1.0,
        };
        let pos: Vec<(f32, f32)> = FULL.channels().iter().filter(|c| **c != Ch::Lfe).map(|c| position(FULL, *c)).collect();
        let idx: Vec<usize> = FULL.channels().iter().enumerate().filter(|(_, c)| **c != Ch::Lfe).map(|(k, _)| k).collect();
        let lfe_full = FULL.index(Ch::Lfe).unwrap_or(11);
        // one 7.1.4 buffer per mode in use
        let mut per: Vec<(BinMode, Vec<Vec<f32>>)> = Vec::new();
        let mut lfe = vec![0f32; frames];
        let buf_for = |m: BinMode, per: &mut Vec<(BinMode, Vec<Vec<f32>>)>| -> usize {
            let m = if dolby { m } else { BinMode::Mid };
            if let Some(k) = per.iter().position(|x| x.0 == m) {
                return k;
            }
            per.push((m, vec![vec![0f32; frames]; FULL.n()]));
            per.len() - 1
        };
        for (k, i, g) in Self::bed_map(bed_fmt) {
            let Some(src) = bed.get(k) else { continue };
            if i == lfe_full {
                for f in 0..frames.min(src.len()) {
                    lfe[f] += src[f] * g;
                }
                continue;
            }
            let m = bed_modes.get(k).copied().unwrap_or_default();
            let b = buf_for(m, &mut per);
            for f in 0..frames.min(src.len()) {
                per[b].1[i][f] += src[f] * g;
            }
        }
        for o in objects {
            let b = buf_for(o.mode, &mut per);
            let g = object_gains(FULL, &o.pos);
            for (i, gi) in g.iter().enumerate() {
                if *gi > 1e-5 {
                    for f in 0..frames.min(o.signal.len()) {
                        per[b].1[i][f] += o.signal[f] * gi;
                    }
                }
            }
        }
        for (m, bufs) in &per {
            let k = match self.bin.iter().position(|x| x.0 == *m && x.1 == dolby) {
                Some(k) => k,
                None => {
                    self.bin.push((*m, dolby, Binaural::new(pos.clone(), *m, self.rate, roomy)));
                    self.bin.len() - 1
                }
            };
            let chans: Vec<&[f32]> = idx.iter().map(|i| bufs[*i].as_slice()).collect();
            let (l, r) = out.split_at_mut(1);
            self.bin[k].2.run(&chans, &mut l[0], &mut r[0]);
        }
        // (the LFE: to both ears, as Off — no distance)
        for f in 0..frames {
            out[0][f] += lfe[f] * FRAC_1_SQRT_2;
            out[1][f] += lfe[f] * FRAC_1_SQRT_2;
        }
    }
}

/// A surround mix to headphones (Spatial Audio Monitoring, the monitor fold): `chans` in
/// `f`'s order rendered binaurally (one shared distance), added into `l`, `r`.
pub struct Headphones {
    rate: f32,
    fmt: Option<Format>,
    bin: Option<Binaural>,
    roomy: f32,
}

impl Headphones {
    pub fn new(rate: f32, roomy: f32) -> Headphones {
        Headphones { rate, fmt: None, bin: None, roomy }
    }

    pub fn run(&mut self, f: Format, chans: &[Vec<f32>], l: &mut [f32], r: &mut [f32]) {
        if self.fmt != Some(f) || self.bin.is_none() {
            let pos = f.channels().iter().filter(|c| **c != Ch::Lfe).map(|c| position(f, *c)).collect();
            self.bin = Some(Binaural::new(pos, BinMode::Mid, self.rate, self.roomy));
            self.fmt = Some(f);
        }
        let srcs: Vec<&[f32]> = f.channels().iter().enumerate().filter(|(_, c)| **c != Ch::Lfe).map(|(k, _)| chans[k].as_slice()).collect();
        if let Some(b) = self.bin.as_mut() {
            b.run(&srcs, l, r);
        }
        if let Some(k) = f.index(Ch::Lfe) {
            for (i, x) in chans[k].iter().enumerate().take(l.len()) {
                l[i] += x * FRAC_1_SQRT_2;
                r[i] += x * FRAC_1_SQRT_2;
            }
        }
    }
}

/// A plain fold of any layout to stereo (Lo/Ro): left channels left, right channels right, the
/// centre ones at −3 dB to both, LFE dropped. For monitoring a surround mix on two speakers.
pub fn fold_gains(c: Ch) -> (f32, f32) {
    let h = FRAC_1_SQRT_2;
    match c {
        Ch::L | Ch::Lm | Ch::Lt | Ch::Ltm => (1.0, 0.0),
        Ch::R | Ch::Rm | Ch::Rt | Ch::Rtm => (0.0, 1.0),
        Ch::Ls | Ch::Lts => (h, 0.0),
        Ch::Rs | Ch::Rts => (0.0, h),
        Ch::Lc => (0.92, 0.38),
        Ch::Rc => (0.38, 0.92),
        Ch::C | Ch::S => (h, h),
        Ch::Lfe => (0.0, 0.0),
    }
}

/// A matrix (dest × src) folding layout `from` onto layout `to` by position (Down Mixer): each
/// source channel present in the destination goes straight there; others are panned between
/// the destination's nearest speakers at their angle (heights to the ear level). Levels:
/// `center`, `surround`, `lfe` gains for those source channels missing in `to`.
pub fn downmix_matrix(from: &[Ch], to: &[Ch], from_fmt: Format, center: f32, surround: f32, lfe: f32) -> Vec<Vec<f32>> {
    let mut m = vec![vec![0f32; from.len()]; to.len()];
    let to_fmt = Format::of_channels(to);
    for (s, c) in from.iter().enumerate() {
        if let Some(d) = to.iter().position(|x| x == c) {
            m[d][s] = 1.0;
            continue;
        }
        if *c == Ch::Lfe {
            // no LFE in the destination: dropped unless its level is raised
            for (d, t) in to.iter().enumerate() {
                if matches!(t, Ch::L | Ch::R) {
                    m[d][s] = lfe * FRAC_1_SQRT_2;
                }
            }
            continue;
        }
        let level = if *c == Ch::C { center } else if matches!(c, Ch::Ls | Ch::Rs | Ch::Lm | Ch::Rm | Ch::S) { surround } else { 1.0 };
        let az = position(from_fmt, *c).0;
        let speakers: Vec<(usize, f32)> = to.iter().enumerate().filter(|(_, t)| **t != Ch::Lfe && !t.height()).map(|(k, t)| (k, to_fmt.map_or_else(|| stereo_az(*t), |f| position(f, *t).0))).collect();
        let mut pow = vec![0f32; to.len()];
        if speakers.len() == 2 && to.len() == 2 {
            // to stereo: Lo/Ro style (centre at −3 dB to both, sides to their own)
            let (gl, gr) = fold_gains(*c);
            m[0][s] = gl * level;
            m[1][s] = gr * level;
            continue;
        }
        pair_power(&speakers, az, &mut pow);
        for (d, p) in pow.iter().enumerate() {
            m[d][s] = p.sqrt() * level;
        }
    }
    m
}

fn stereo_az(c: Ch) -> f32 {
    match c {
        Ch::L => -30.0,
        Ch::R => 30.0,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn power(g: &[f32]) -> f32 {
        g.iter().map(|x| x * x).sum()
    }

    #[test]
    fn channel_orders_are_logics() {
        let names = |f: Format| f.channels().iter().map(|c| c.label()).collect::<Vec<_>>().join(" ");
        assert_eq!(names(Format::Quad), "L R Ls Rs");
        assert_eq!(names(Format::Lcrs), "L C R S");
        assert_eq!(names(Format::S51), "L C R Ls Rs LFE");
        assert_eq!(names(Format::S512), "L C R Ls Rs Ltm Rtm LFE");
        assert_eq!(names(Format::S514), "L C R Ls Rs Lt Rt Lts Rts LFE");
        assert_eq!(names(Format::S61), "L C R Ls S Rs LFE");
        assert_eq!(names(Format::S71), "L C R Lm Rm Ls Rs LFE");
        assert_eq!(names(Format::Sdds), "L Lc C Rc R Ls Rs LFE");
        assert_eq!(names(Format::S712), "L C R Lm Rm Ls Rs Ltm Rtm LFE");
        assert_eq!(names(Format::S714), "L C R Lm Rm Ls Rs Lt Rt Lts Rts LFE");
        assert_eq!(Format::ALL.len(), 10);
        assert_eq!(Format::from_name("7.1.4"), Some(Format::S714));
    }

    #[test]
    fn meter_orders_follow_the_settings() {
        let s = |f, o| meter_order(f, o).iter().map(|c| c.label()).collect::<Vec<_>>().join(" ");
        assert_eq!(s(Format::S51, MeterOrder::Dts), "L R Ls Rs C LFE");
        assert_eq!(s(Format::S51, MeterOrder::Smpte), "L R C LFE Ls Rs");
        assert_eq!(s(Format::S51, MeterOrder::Clockwise), "Ls L C R Rs LFE");
        assert_eq!(s(Format::S51, MeterOrder::Film), "L C R Ls Rs LFE");
        assert_eq!(s(Format::S714, MeterOrder::Dts), "L R Ls Rs Lm Rm C LFE Lt Rt Lts Rts");
        assert_eq!(s(Format::S714, MeterOrder::Smpte), "L R C LFE Ls Rs Lm Rm Lt Rt Lts Rts");
        assert_eq!(s(Format::S714, MeterOrder::Clockwise), "Ls Lm L C R Rm Rs LFE Lts Lt Rt Rts");
        assert_eq!(s(Format::S714, MeterOrder::Film), "L C R Lm Rm Ls Rs LFE Lt Rt Lts Rts");
        assert_eq!(MeterOrder::from_setting("Film (L C R Ls Rs LFE)"), MeterOrder::Film);
    }

    #[test]
    fn panner_puts_a_source_on_its_speaker_and_spreads_with_diversity() {
        let f = Format::S51;
        let mut p = Pan::default();
        p.angle = -30.0;
        let g = mono_gains(f, &p);
        assert!((g[0] - 1.0).abs() < 1e-4, "at L only L: {g:?}");
        assert!(g[1..5].iter().all(|x| x.abs() < 1e-3));
        assert_eq!(g[5], 0.0, "LFE off by default");
        // between L and C: both, constant power
        p.angle = -15.0;
        let g = mono_gains(f, &p);
        assert!(g[0] > 0.6 && g[1] > 0.6 && (power(&g) - 1.0).abs() < 1e-3);
        // diversity 1: every speaker alike
        p.diversity = 1.0;
        let g = mono_gains(f, &p);
        assert!(g[..5].iter().all(|x| (x - (0.2f32).sqrt()).abs() < 1e-3), "{g:?}");
        // behind: between the surrounds
        let mut p = Pan::default();
        p.angle = 180.0;
        let g = mono_gains(f, &p);
        assert!((g[3] - g[4]).abs() < 1e-3 && g[3] > 0.6);
        // LFE and Center levels
        p.lfe_db = 0.0;
        p.angle = 0.0;
        p.center_db = -6.0;
        let g = mono_gains(f, &p);
        assert!((g[5] - 1.0).abs() < 1e-4 && (g[1] - 0.501).abs() < 0.01);
    }

    #[test]
    fn speakers_turned_off_pass_their_share_on() {
        let f = Format::S51;
        let mut p = Pan::default();
        p.toggle(Ch::C);
        let g = mono_gains(f, &p);
        assert_eq!(g[1], 0.0);
        assert!((g[0] - g[2]).abs() < 1e-4 && g[0] > 0.6, "front centre falls between L and R: {g:?}");
    }

    #[test]
    fn elevation_moves_a_source_to_the_height_speakers() {
        let f = Format::S714;
        let mut p = Pan::default();
        p.elevation = 90.0;
        let g = mono_gains(f, &p);
        let h: f32 = f.channels().iter().zip(&g).filter(|(c, _)| c.height()).map(|(_, x)| x * x).sum();
        assert!((h - 1.0).abs() < 1e-3, "only heights at 90°");
        p.elevation = 0.0;
        p.diversity = 1.0;
        let g = mono_gains(f, &p);
        assert!(g[f.index(Ch::Lt).unwrap()] > 0.1, "Planar: a centred puck feeds the heights too");
    }

    #[test]
    fn stereo_spread_places_left_and_right() {
        let f = Format::S71;
        let mut p = Pan::default();
        p.spread = 180.0;
        let (l, r) = stereo_gains(f, &p);
        assert!((l[f.index(Ch::Lm).unwrap()] - 1.0).abs() < 1e-3, "180°: left at left mid");
        assert!((r[f.index(Ch::Rm).unwrap()] - 1.0).abs() < 1e-3);
        p.spread = -60.0;
        let (l, _) = stereo_gains(f, &p);
        assert!((l[f.index(Ch::R).unwrap()] - 1.0).abs() < 1e-3, "negative spread swaps");
        p.spread = 0.0;
        let (l, r) = stereo_gains(f, &p);
        assert_eq!(l, r, "0°: mono");
    }

    #[test]
    fn separation_zero_makes_both_sides_alike() {
        let f = Format::S51;
        let mut p = Pan::default();
        p.angle = -30.0;
        p.separation = true;
        p.sep_xf = 0.0;
        let g = mono_gains(f, &p);
        assert!((g[0] - g[2]).abs() < 1e-5);
    }

    #[test]
    fn balancer_only_changes_levels() {
        let f = Format::S51;
        let mut p = Pan::default();
        assert!(balancer_gains(f, &p).iter().all(|g| (*g - 1.0).abs() < 1e-6), "Amount 0: unchanged");
        p.amount = 1.0;
        p.angle = 0.0;
        let g = balancer_gains(f, &p);
        assert!(g[1] > 0.99 && g[3] < g[0] && g[3] < 0.5, "toward the front: rear lower {g:?}");
        p.toggle(Ch::Rs);
        assert_eq!(balancer_gains(f, &p)[4], 0.0);
    }

    #[test]
    fn objects_render_near_their_position() {
        let o = Object { x: -1.0, y: 1.0, z: 0.0, size: 0.0, spread: 0.0 };
        let g = object_gains(Format::S714, &o);
        assert!(g[0] > 0.95, "at L: {g:?}");
        assert_eq!(g[Format::S714.index(Ch::Lfe).unwrap()], 0.0, "never the LFE");
        let up = Object { x: -1.0, y: 0.5, z: 1.0, ..o };
        let g = object_gains(Format::S714, &up);
        assert!(g[Format::S714.index(Ch::Lt).unwrap()] > 0.9);
        let big = Object { size: 1.0, ..o };
        let g = object_gains(Format::S714, &big);
        assert!(g.iter().filter(|x| **x > 0.2).count() >= 10, "Size 1 spreads it");
        assert!((power(&g) - 1.0).abs() < 1e-3);
        let (l, r) = Object { x: 0.0, y: 1.0, z: 0.0, size: 0.0, spread: 1.0 }.stereo();
        assert_eq!((l.x, r.x), (-1.0, 1.0));
    }

    fn sine(n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect()
    }

    #[test]
    fn renderer_folds_and_renders_to_each_monitoring_format() {
        let n = 4800;
        let sig = sine(n);
        let mut bed = vec![vec![0f32; n]; Format::S712.n()];
        bed[Format::S712.index(Ch::Ltm).unwrap()] = sig.clone();
        let mut r = Renderer::new(48000.0);
        let mut cfg = RenderCfg { monitoring: Monitoring::M714, ..Default::default() };
        let out = r.render(&cfg, Format::S712, &bed, &[], &[], n);
        assert_eq!(out.len(), 12);
        // 7.1.2's Ltm spreads over Lt and Lts
        let e = |v: &Vec<f32>| v.iter().map(|x| x * x).sum::<f32>();
        assert!(e(&out[Format::S714.index(Ch::Lt).unwrap()]) > 1.0 && e(&out[Format::S714.index(Ch::Lts).unwrap()]) > 1.0);
        // 5.1: heights fold into the ear level
        cfg.monitoring = Monitoring::M51;
        let out = r.render(&cfg, Format::S712, &bed, &[], &[], n);
        assert_eq!(out.len(), 6);
        assert!(e(&out[0]) > 1.0 && e(&out[3]) > 1.0);
        // an object panned left in 2.0 Direct is louder on the left
        let obj = ObjectIn { signal: &sig, pos: Object { x: -1.0, y: 1.0, z: 0.0, size: 0.0, spread: 0.0 }, mode: BinMode::Mid };
        cfg.monitoring = Monitoring::Direct20;
        let quiet = vec![vec![0f32; n]; Format::S712.n()];
        let out = r.render(&cfg, Format::S712, &quiet, &[], &[obj], n);
        assert!(e(&out[0]) > 10.0 * e(&out[1]).max(1e-9));
        // binaural: two channels, the left ear louder for a source on the left
        cfg.monitoring = Monitoring::DolbyRenderer;
        let obj = ObjectIn { signal: &sig, pos: Object { x: -1.0, y: 0.0, z: 0.0, size: 0.0, spread: 0.0 }, mode: BinMode::Off };
        let out = r.render(&cfg, Format::S712, &quiet, &[], &[obj], n);
        assert_eq!(out.len(), 2);
        assert!(e(&out[0]) > 1.5 * e(&out[1]), "{} {}", e(&out[0]), e(&out[1]));
    }

    #[test]
    fn lo_ro_and_pro_logic_folds() {
        let n = 1000;
        let sig = vec![0.5f32; n];
        let mut bed = vec![vec![0f32; n]; Format::S71.n()];
        bed[Format::S71.index(Ch::Lm).unwrap()] = sig.clone();
        let mut r = Renderer::new(48000.0);
        let mut cfg = RenderCfg { monitoring: Monitoring::M51, ..Default::default() };
        let out = r.render(&cfg, Format::S71, &bed, &[], &[], n);
        assert!((out[3][10] - 0.5).abs() < 1e-4, "Lo/Ro: the side at 0 dB into Ls");
        cfg.down51 = Down51::ProLogicIIx;
        let out = r.render(&cfg, Format::S71, &bed, &[], &[], n);
        assert!((out[3][10] - 0.5 * 10f32.powf(-1.2 / 20.0)).abs() < 1e-4);
        // 5.1 to 2.0 Lo/Ro: centre at −3 dB on both, LFE dropped
        let mut bed = vec![vec![0f32; n]; 6];
        bed[1] = sig.clone();
        bed[5] = sig.clone();
        cfg.monitoring = Monitoring::Downmix20;
        cfg.down51 = Down51::LoRo;
        let out = r.render(&cfg, Format::S51, &bed, &[], &[], n);
        assert!((out[0][10] - 0.5 * FRAC_1_SQRT_2).abs() < 1e-3 && (out[1][10] - out[0][10]).abs() < 1e-6);
    }

    #[test]
    fn trims_lower_surrounds_and_heights() {
        let n = 100;
        let mut bed = vec![vec![0f32; n]; 10];
        bed[Format::S712.index(Ch::Ls).unwrap()] = vec![1.0; n];
        let mut r = Renderer::new(48000.0);
        let mut cfg = RenderCfg { monitoring: Monitoring::M51, ..Default::default() };
        cfg.trims[1] = Trim { manual: true, surround_db: -6.0, height_db: 0.0, overhead: 0.0, listener: 0.0 };
        let out = r.render(&cfg, Format::S712, &bed, &[], &[], n);
        assert!((out[3][5] - 0.501).abs() < 0.01);
    }

    #[test]
    fn quadrature_networks_are_ninety_degrees_apart() {
        let mut q = Quadrature::default();
        let n = 48000;
        let f = 1000.0 / 48000.0 * std::f32::consts::TAU;
        let (mut aa, mut bb, mut ab) = (0.0, 0.0, 0.0);
        for i in 0..n {
            let (a, b) = q.step((i as f32 * f).sin());
            if i > 1000 {
                aa += a * a;
                bb += b * b;
                ab += a * b;
            }
        }
        let corr = ab / (aa * bb).sqrt();
        assert!(corr.abs() < 0.1, "uncorrelated (90°): {corr}");
        assert!((aa / bb - 1.0).abs() < 0.05);
    }

    #[test]
    fn downmix_matrix_folds_5_1_to_stereo() {
        let m = downmix_matrix(Format::S51.channels(), &[Ch::L, Ch::R], Format::S51, 1.0, 1.0, 0.0);
        assert_eq!(m[0][0], 1.0);
        assert!((m[0][1] - FRAC_1_SQRT_2).abs() < 1e-6 && (m[1][1] - FRAC_1_SQRT_2).abs() < 1e-6);
        assert_eq!(m[0][5], 0.0);
        let q = downmix_matrix(Format::S71.channels(), Format::S51.channels(), Format::S71, 1.0, 1.0, 1.0);
        assert!(q[3][3] > 0.5, "Lm folds into Ls");
    }

    #[test]
    fn puck_sets_angle_and_diversity_or_elevation() {
        let mut p = Pan::default();
        p.set_puck(-1.0, 0.0, 0, false);
        assert!((p.angle + 90.0).abs() < 1e-3 && p.diversity.abs() < 1e-4);
        p.set_puck(0.0, 0.5, 1, false);
        assert!(p.diversity.abs() < 1e-4, "diversity locked");
        p.set_puck(0.0, 0.0, 2, false);
        assert!(p.angle.abs() < 1e-3 && (p.diversity - 1.0).abs() < 1e-4, "angle locked");
        p.spherical = true;
        p.set_puck(0.0, 0.0, 0, true);
        assert!((p.elevation - 90.0).abs() < 1e-3, "Spherical: centre is overhead");
    }
}
