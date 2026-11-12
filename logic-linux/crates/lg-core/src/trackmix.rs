//! Track and channel strip settings beside Logic's volume / pan / mute / solo: the track header's
//! On/Off, Protect, Freeze and groove buttons, colours and icons, the default region type,
//! external MIDI destinations, tracks sharing a channel strip; the strip's input format, pan
//! mode (Balance / Pan, Stereo Pan, Binaural), solo-safe; and the pure rules around them
//! (renaming several tracks, Sort Tracks By, the groove track, drum replacement, export file
//! names, normalizing). Studio keeps these in its sidecar (the .logicx chunks for them aren't
//! decoded).

use crate::events::Note;

/// What the Pencil makes on a software instrument track, and which editor opens for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RegionType {
    #[default]
    Midi,
    Pattern,
    SessionPlayer,
}

impl RegionType {
    pub const ALL: [RegionType; 3] = [RegionType::Midi, RegionType::Pattern, RegionType::SessionPlayer];
    pub fn name(self) -> &'static str {
        match self {
            RegionType::Midi => "MIDI",
            RegionType::Pattern => "Pattern",
            RegionType::SessionPlayer => "Session Player",
        }
    }
}

/// Track inspector ▸ Freeze Mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FreezeMode {
    /// Without the effect plug-ins (they keep running on the frozen sound); a green button.
    SourceOnly,
    /// With all effect plug-ins; a blue button.
    #[default]
    PreFader,
}

impl FreezeMode {
    pub fn name(self) -> &'static str {
        match self {
            FreezeMode::SourceOnly => "Source Only",
            FreezeMode::PreFader => "Pre Fader",
        }
    }
}

/// Reassign Track: its own channel strip, none (it sends no data), or a folder's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Assign {
    #[default]
    Strip,
    NotAssigned,
    Folder,
}

/// The channel strip's input format (the Channel Mode button).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InputFormat {
    Mono,
    #[default]
    Stereo,
    Left,
    Right,
    Surround,
}

impl InputFormat {
    pub const ALL: [InputFormat; 5] = [InputFormat::Mono, InputFormat::Stereo, InputFormat::Left, InputFormat::Right, InputFormat::Surround];
    pub fn name(self) -> &'static str {
        match self {
            InputFormat::Mono => "Mono",
            InputFormat::Stereo => "Stereo",
            InputFormat::Left => "Left",
            InputFormat::Right => "Right",
            InputFormat::Surround => "Surround",
        }
    }
    /// One meter column (mono, left, right) or two (stereo).
    pub fn meter_columns(self) -> usize {
        match self {
            InputFormat::Stereo => 2,
            InputFormat::Surround => 6,
            _ => 1,
        }
    }
    /// A mono strip pans (Pan); a stereo one balances (Balance).
    pub fn is_stereo(self) -> bool {
        matches!(self, InputFormat::Stereo | InputFormat::Surround)
    }
    /// The format applied to a block: Mono sums, Left / Right play that channel alone.
    pub fn apply(self, l: &mut [f32], r: &mut [f32]) {
        match self {
            InputFormat::Stereo | InputFormat::Surround => {}
            InputFormat::Mono => {
                for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                    let m = (*a + *b) * 0.5;
                    *a = m;
                    *b = m;
                }
            }
            InputFormat::Left => r.copy_from_slice(l),
            InputFormat::Right => l.copy_from_slice(r),
        }
    }
}

/// The panner on a strip (Control-click the Pan knob).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PanMode {
    /// Pan on mono strips, Balance on stereo ones.
    #[default]
    Balance,
    StereoPan,
    Binaural,
}

impl PanMode {
    pub fn name(self, stereo: bool) -> &'static str {
        match self {
            PanMode::Balance if stereo => "Stereo Balance",
            PanMode::Balance => "Pan",
            PanMode::StereoPan => "Stereo Pan",
            PanMode::Binaural => "Binaural Panner",
        }
    }
}

/// Stereo Pan: where the left and the right signal sit (-1 left … 1 right). Swapped when the
/// left sits right of the right (Command-click; the ring turns orange).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StereoPan {
    pub left: f32,
    pub right: f32,
}

impl Default for StereoPan {
    fn default() -> Self {
        StereoPan { left: -1.0, right: 1.0 }
    }
}

impl StereoPan {
    pub fn swapped(self) -> bool {
        self.left > self.right
    }
    pub fn spread(self) -> f32 {
        (self.right - self.left).abs()
    }
    pub fn center(self) -> f32 {
        (self.left + self.right) / 2.0
    }
    /// Sets spread and centre keeping the order (swapped or not), within -1…1.
    pub fn set(&mut self, center: f32, spread: f32) {
        let half = (spread.clamp(0.0, 2.0)) / 2.0;
        let c = center.clamp(-1.0 + half, 1.0 - half);
        let sw = self.swapped();
        let (a, b) = (c - half, c + half);
        (self.left, self.right) = if sw { (b, a) } else { (a, b) };
    }
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.left, &mut self.right);
    }
    /// [[L→L, R→L], [L→R, R→R]]: each side panned at equal power to its place.
    pub fn matrix(self) -> [[f32; 2]; 2] {
        let g = |pos: f32| {
            let a = (pos.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
            (a.cos(), a.sin())
        };
        let (ll, lr) = g(self.left);
        let (rl, rr) = g(self.right);
        [[ll, rl], [lr, rr]]
    }
}

/// The Binaural Panner's settings. Angles in degrees: angle 0 front, positive to the right,
/// ±180 behind; elevation -90…90; distance and size in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Binaural {
    pub angle: f32,
    pub elevation: f32,
    pub distance: f32,
    /// Stereo strips: the width between the left and right pucks, degrees (0: one puck).
    pub spread: f32,
    /// Planar (a flat circular plane) or Spherical.
    pub planar: bool,
    pub vertical_offset: f32,
    pub tilt_amount: f32,
    pub tilt_direction: f32,
    pub doppler: bool,
    pub size: f32,
    pub diffuse_field: bool,
}

pub const BINAURAL_SIZE_DEFAULT: f32 = 1.5;

impl Default for Binaural {
    fn default() -> Self {
        Binaural { angle: 0.0, elevation: 0.0, distance: 1.0, spread: 60.0, planar: true, vertical_offset: 0.0, tilt_amount: 0.0, tilt_direction: 0.0, doppler: false, size: BINAURAL_SIZE_DEFAULT, diffuse_field: true }
    }
}

impl Binaural {
    /// The direction puck on the panning plane (x right, y front; the plane's radius is 1 =
    /// `size` metres) for the current angle and distance.
    pub fn puck(&self) -> (f32, f32) {
        let a = self.angle.to_radians();
        let d = (self.distance / self.size.max(0.01)).clamp(0.0, 1.0);
        if self.planar {
            (d * a.sin(), d * a.cos())
        } else {
            // spherical: the plane shows the sphere from above and unrolled: the upper half is
            // the front (centre = overhead), the lower half behind
            let el = self.elevation.clamp(0.0, 90.0);
            let rr = 1.0 - el / 90.0;
            (rr * a.sin(), rr * a.cos())
        }
    }
    /// The puck dragged to (x, y) on the plane: angle, distance (planar) or angle and elevation
    /// (spherical), then the planar tilt applied.
    pub fn set_puck(&mut self, x: f32, y: f32) {
        let rr = (x * x + y * y).sqrt().min(1.0);
        self.angle = if rr < 1e-4 { 0.0 } else { x.atan2(y).to_degrees() };
        if self.planar {
            self.distance = rr * self.size;
            self.apply_tilt();
        } else {
            self.elevation = (1.0 - rr) * 90.0;
            self.distance = self.size;
        }
    }
    /// Planar mode: the plane's height (Vertical Offset) and tilt (Tilt Amount towards Tilt
    /// Direction) give the elevation of the puck's place.
    pub fn apply_tilt(&mut self) {
        if !self.planar {
            return;
        }
        let (x, y) = self.puck();
        let dir = self.tilt_direction.to_radians();
        // height of the plane at the puck: offset + slope along the tilt direction
        let along = x * dir.sin() + y * dir.cos();
        let h = self.vertical_offset + along * self.tilt_amount.to_radians().tan();
        let flat = (x * x + y * y).sqrt().max(1e-4);
        self.elevation = h.atan2(flat).to_degrees().clamp(-90.0, 90.0);
        if (x, y) == (0.0, 0.0) {
            self.elevation = if h > 0.0 { 90.0 } else if h < 0.0 { -90.0 } else { 0.0 };
        }
    }
    /// Option-click on the plane: centre front.
    pub fn reset_position(&mut self) {
        self.angle = 0.0;
        self.distance = self.size.min(1.0);
        self.elevation = 0.0;
        self.apply_tilt();
    }
    /// The left and right pucks of a stereo strip: the direction ± half the spread.
    pub fn sides(&self) -> (f32, f32) {
        (wrap_deg(self.angle - self.spread / 2.0), wrap_deg(self.angle + self.spread / 2.0))
    }
    /// How a source at `angle` reaches the ears: (left gain, right gain, left delay s, right
    /// delay s, far-ear low-pass Hz). A spherical-head model (Woodworth's delay, a head shadow
    /// that grows with frequency), 1/distance level, elevation narrowing the difference.
    pub fn ears(&self, angle: f32) -> Ears {
        let az = angle.to_radians();
        let el = self.elevation.to_radians();
        // lateral angle: how far to the side, front/back folded
        let lat = (az.sin() * el.cos()).clamp(-1.0, 1.0).asin();
        let head = 0.0875f32;
        let c = 343.0f32;
        let itd = head / c * (lat.abs() + lat.abs().sin());
        let (dl, dr) = if lat > 0.0 { (itd, 0.0) } else { (0.0, itd) };
        // level: the far ear down to about -9 dB broadband at the side; behind slightly duller
        let shadow = 1.0 - 0.65 * lat.abs() / std::f32::consts::FRAC_PI_2;
        let near = 1.0 + 0.2 * lat.abs() / std::f32::consts::FRAC_PI_2;
        let (mut gl, mut gr) = if lat > 0.0 { (shadow, near) } else { (near, shadow) };
        let dist = self.distance.max(0.1);
        let lvl = (1.0 / dist).clamp(0.1, 4.0);
        gl *= lvl;
        gr *= lvl;
        let behind = az.cos() < 0.0;
        let lp = 16000.0 - 11000.0 * lat.abs() / std::f32::consts::FRAC_PI_2;
        let lp = if behind { lp * 0.7 } else { lp };
        Ears { gain_l: gl, gain_r: gr, delay_l: dl, delay_r: dr, far_lowpass: lp, behind }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ears {
    pub gain_l: f32,
    pub gain_r: f32,
    pub delay_l: f32,
    pub delay_r: f32,
    pub far_lowpass: f32,
    pub behind: bool,
}

pub fn wrap_deg(a: f32) -> f32 {
    let mut a = a % 360.0;
    if a > 180.0 {
        a -= 360.0;
    }
    if a < -180.0 {
        a += 360.0;
    }
    a
}

/// A send's place in the strip: before the fader, after it (before pan), or after pan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SendMode {
    #[default]
    PostFader,
    PreFader,
    PostPan,
}

impl SendMode {
    pub fn name(self) -> &'static str {
        match self {
            SendMode::PostFader => "Post Fader",
            SendMode::PreFader => "Pre Fader",
            SendMode::PostPan => "Post Pan",
        }
    }
}

/// What a send has beside its bus and level (kept per strip, by send index).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SendSet {
    pub post_pan: bool,
    /// Independent Pan: the send's own pan (-1…1).
    pub independent_pan: Option<f32>,
    /// Low Latency Safe: stays on in Low Latency Monitoring mode.
    pub low_latency_safe: bool,
    pub bypass: bool,
}

/// A channel strip's settings beside the Mixer's own.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StripSet {
    pub solo_safe: bool,
    pub pan_mode: PanMode,
    pub stereo_pan: StereoPan,
    pub binaural: Binaural,
    pub format: InputFormat,
    /// The colour from the palette (None: the track type's).
    pub color: Option<u8>,
    pub sends: Vec<SendSet>,
    /// Toggle Level: the level the fader left when it was toggled down (dB).
    pub toggled_level: Option<f32>,
    /// Phase reverse on the strip's input.
    pub phase_invert: bool,
    /// Input gain in dB (-12…12).
    pub input_gain: f32,
    /// A name given in the Mixer (auxes, outputs: tracks keep their own).
    pub notes: String,
}

impl StripSet {
    pub fn send(&self, k: usize) -> SendSet {
        self.sends.get(k).copied().unwrap_or_default()
    }
    pub fn send_mut(&mut self, k: usize) -> &mut SendSet {
        if self.sends.len() <= k {
            self.sends.resize(k + 1, SendSet::default());
        }
        &mut self.sends[k]
    }
}

/// An External MIDI track's destination.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct External {
    /// The port's name ("" = Bramble Studio Virtual Out).
    pub port: String,
    /// MIDI channel 1…16.
    pub channel: u8,
    /// Played through the External Instrument plug-in (monitored as audio).
    pub use_plugin: bool,
    pub program: Option<u8>,
    pub bank: (u8, u8),
    /// The MIDI channel strip's Assign 1–5 knobs: controller number and value.
    pub assigns: [(u8, u8); 5],
}

/// Track inspector: the MIDI parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct MidiSet {
    /// "Off", "All", "Musical Typing", a port's name, "Virtual In".
    pub input: String,
    /// Another track's id (Internal MIDI In), or none.
    pub internal_in: Option<u64>,
    /// 0 = All.
    pub in_channel: u8,
    /// 0 = All, 1…16, 17 = Round Robin.
    pub out_channel: u8,
    pub no_reset: bool,
    /// Record: both MIDI input and Internal MIDI In (false: Internal MIDI In only).
    pub record_both: bool,
    pub staff_style: String,
    /// Hermode Tuning: 0 Off, 1 Analyze, 2 Apply, 3 Analyze & Apply.
    pub hermode: u8,
    /// Delay shown in milliseconds (kept beside the ticks value).
    pub delay_ms: f32,
    pub delay_in_ms: bool,
}

impl Default for MidiSet {
    fn default() -> Self {
        MidiSet { input: "All".into(), internal_in: None, in_channel: 0, out_channel: 0, no_reset: false, record_both: true, staff_style: "Auto".into(), hermode: 3, delay_ms: 0.0, delay_in_ms: false }
    }
}

/// A track's settings.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TrackSet {
    pub strip: StripSet,
    /// The icon chosen in the Icon palette (a built-in icon's name, or "custom:<file>").
    pub icon: Option<String>,
    /// Turned off with the On/Off button (its plug-ins deactivated); `offline`: Option-click,
    /// offline with the plug-ins kept loaded.
    pub off: bool,
    pub offline: bool,
    pub protected: bool,
    pub frozen: bool,
    pub freeze_mode: FreezeMode,
    /// The project's groove track (one only), or matching it.
    pub groove: bool,
    pub match_groove: bool,
    pub default_region: RegionType,
    pub external: Option<External>,
    /// Shares the channel strip of the track with this id (New Track With Same Channel).
    pub same_strip: Option<u64>,
    pub assign: Assign,
    /// The header's Pan knob drives this send (index) instead.
    pub knob_send: Option<u8>,
    pub midi: MidiSet,
    pub articulation_set: Option<String>,
    /// The name was typed by the user (a patch no longer renames it).
    pub renamed: bool,
    /// Audio tracks: Q-Reference (transients are quantize reference points).
    pub q_reference: bool,
    /// Automation: record region automation with regions (Read mode included).
    pub record_region_automation: bool,
    /// Automation Trim and Relative on top of Touch / Latch / Write.
    pub auto_trim: bool,
    pub auto_relative: bool,
    /// The automation lanes shown under the track (parameter keys), and whether they show.
    pub auto_lanes: Vec<(u8, Option<(u32, u32)>)>,
    pub auto_lanes_open: bool,
    /// Automation parameters muted (their curves grey, not played).
    pub auto_muted: Vec<(u8, Option<(u32, u32)>)>,
    /// The track header shows region automation (false: track automation).
    pub show_region_automation: bool,
}

// ---------------------------------------------------------------------------------------------
// colours

/// The Color palette: 24 hues in 4 rows (bright to dark), as index → RGB.
pub const PALETTE_COLS: usize = 24;
pub const PALETTE_ROWS: usize = 4;

/// The user's own palette (Bramble's custom colour palette), when one is set: replaces the
/// built-in colours index for index (a shorter list keeps the built-in ones after it).
static CUSTOM: std::sync::RwLock<Option<Vec<[u8; 3]>>> = std::sync::RwLock::new(None);

/// Sets (or with None, clears) the custom palette.
pub fn set_custom_palette(colors: Option<Vec<[u8; 3]>>) {
    *CUSTOM.write().unwrap_or_else(|e| e.into_inner()) = colors;
}

/// The custom palette, when one is set.
pub fn custom_palette() -> Option<Vec<[u8; 3]>> {
    CUSTOM.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// A palette colour: the custom palette's, else the built-in one.
pub fn palette(i: u8) -> [u8; 3] {
    if let Some(c) = CUSTOM.read().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|v| v.get(i as usize).copied()) {
        return c;
    }
    default_palette(i)
}

/// Bramble Studio's palette file: a first line `bramble-studio-palette 1`, an optional
/// `name\t<name>` line, then one `rrggbb` colour per line in palette order (row by row); `#`
/// lines are comments.
pub fn palette_text(name: &str, colors: &[[u8; 3]]) -> String {
    let mut s = String::from("bramble-studio-palette 1\n");
    if !name.is_empty() {
        s += &format!("name\t{}\n", name.replace(['\n', '\t'], " "));
    }
    for (k, c) in colors.iter().enumerate() {
        if k % PALETTE_COLS == 0 {
            s += &format!("# row {}\n", k / PALETTE_COLS + 1);
        }
        s += &format!("{:02x}{:02x}{:02x}\n", c[0], c[1], c[2]);
    }
    s
}

/// Reads a palette file: (name, colours), None when it isn't one.
pub fn parse_palette(text: &str) -> Option<(String, Vec<[u8; 3]>)> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'));
    let head = lines.next()?;
    if !head.starts_with("bramble-studio-palette") {
        return None;
    }
    let mut name = String::new();
    let mut v = Vec::new();
    for l in lines {
        if let Some(n) = l.strip_prefix("name\t") {
            name = n.to_string();
            continue;
        }
        let h = l.trim_start_matches('#');
        if h.len() != 6 {
            return None;
        }
        let byte = |k: usize| u8::from_str_radix(&h[k..k + 2], 16).ok();
        v.push([byte(0)?, byte(2)?, byte(4)?]);
    }
    if v.is_empty() || v.len() > PALETTE_COLS * PALETTE_ROWS {
        return None;
    }
    Some((name, v))
}

/// The built-in palette (24 hues, 4 rows).
pub fn default_palette(i: u8) -> [u8; 3] {
    let col = i as usize % PALETTE_COLS;
    let row = (i as usize / PALETTE_COLS).min(PALETTE_ROWS - 1);
    let h = col as f32 / PALETTE_COLS as f32;
    let (s, v) = [(0.45, 1.0), (0.70, 0.92), (0.80, 0.72), (0.75, 0.50)][row];
    hsv(h, s, v)
}

fn hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

// ---------------------------------------------------------------------------------------------
// names

/// The names several tracks (or channel strips) get from one typed name: when it ends in a
/// number, the number counts up across them; otherwise all get the same name.
pub fn numbered_names(typed: &str, n: usize) -> Vec<String> {
    let t = typed.trim_end();
    let digits = t.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || n <= 1 {
        return vec![typed.to_string(); n];
    }
    let (base, num) = t.split_at(t.len() - digits);
    let start: u64 = num.parse().unwrap_or(1);
    (0..n).map(|k| format!("{base}{:0w$}", start + k as u64, w = digits)).collect()
}

/// A name cut in the middle to `max` characters with an ellipsis (channel strip names).
pub fn middle_ellipsis(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max || max < 3 {
        return s.to_string();
    }
    let keep = max - 1;
    let head = keep.div_ceil(2);
    let tail = keep - head;
    let h: String = s.chars().take(head).collect();
    let t: String = s.chars().skip(n - tail).collect();
    format!("{h}…{t}")
}

/// A strip name in two lines of `per_line` characters (11 on narrow strips, 14 on wide ones),
/// cut in the middle when longer.
pub fn two_lines(s: &str, per_line: usize) -> (String, String) {
    let n = s.chars().count();
    if n <= per_line {
        return (s.to_string(), String::new());
    }
    let s = middle_ellipsis(s, per_line * 2);
    // break at a space near the line's end when there is one
    let chars: Vec<char> = s.chars().collect();
    let cut = (1..=per_line.min(chars.len())).rev().find(|&k| chars.get(k) == Some(&' ') && chars.len() - k - 1 <= per_line).unwrap_or(per_line.min(chars.len()));
    let a: String = chars[..cut].iter().collect();
    let b: String = chars[cut..].iter().collect();
    (a.trim().to_string(), b.trim().to_string())
}

// ---------------------------------------------------------------------------------------------
// Sort Tracks By

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    MidiChannel,
    AudioChannel,
    OutputChannel,
    InstrumentName,
    TrackName,
    Used,
}

impl SortBy {
    pub const ALL: [(SortBy, &'static str); 6] = [(SortBy::MidiChannel, "MIDI Channel"), (SortBy::AudioChannel, "Audio Channel"), (SortBy::OutputChannel, "Output Channel"), (SortBy::InstrumentName, "Instrument Name"), (SortBy::TrackName, "Track Name"), (SortBy::Used, "Used ↔ Unused")];
}

/// What Sort Tracks By compares for one top-level track.
#[derive(Clone, Debug, Default)]
pub struct SortKey {
    pub midi_channel: u32,
    pub audio_channel: u32,
    pub output: u32,
    pub instrument: String,
    pub name: String,
    pub used: bool,
}

/// The order (indexes into `keys`) Sort Tracks By gives; stable, so equal tracks keep their
/// order.
pub fn sort_order(keys: &[SortKey], by: SortBy) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..keys.len()).collect();
    match by {
        SortBy::MidiChannel => idx.sort_by_key(|&i| keys[i].midi_channel),
        SortBy::AudioChannel => idx.sort_by_key(|&i| keys[i].audio_channel),
        SortBy::OutputChannel => idx.sort_by_key(|&i| keys[i].output),
        SortBy::InstrumentName => idx.sort_by_key(|&i| keys[i].instrument.to_lowercase()),
        SortBy::TrackName => idx.sort_by_key(|&i| natural_key(&keys[i].name)),
        SortBy::Used => idx.sort_by_key(|&i| !keys[i].used),
    }
    idx
}

/// Names compared with their numbers as numbers ("Audio 2" before "Audio 10").
fn natural_key(s: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut num = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            if !num.is_empty() {
                out.push((std::mem::take(&mut text), num.parse().unwrap_or(0)));
                num.clear();
            }
            text.push(c);
        }
    }
    out.push((text, num.parse().unwrap_or(0)));
    out
}

// ---------------------------------------------------------------------------------------------
// the groove track

/// Moves each note start to the nearest groove onset within `window` ticks (the note keeps its
/// length); notes with no onset near stay. `onsets` sorted.
pub fn match_groove(notes: &mut [(u32, u32, u8, u8)], onsets: &[u32], window: u32) {
    if onsets.is_empty() {
        return;
    }
    for n in notes.iter_mut() {
        let k = onsets.partition_point(|&o| o < n.0);
        let near = [k.checked_sub(1), (k < onsets.len()).then_some(k)].into_iter().flatten().map(|j| onsets[j]).min_by_key(|&o| o.abs_diff(n.0));
        if let Some(o) = near {
            if o.abs_diff(n.0) <= window {
                let len = n.1.saturating_sub(n.0);
                n.0 = o;
                n.1 = o + len;
            }
        }
    }
    notes.sort_by_key(|n| n.0);
}

// ---------------------------------------------------------------------------------------------
// drum replacement

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DrumKind {
    #[default]
    Kick,
    Snare,
    Tom,
    Other,
}

impl DrumKind {
    pub const ALL: [DrumKind; 4] = [DrumKind::Kick, DrumKind::Snare, DrumKind::Tom, DrumKind::Other];
    pub fn name(self) -> &'static str {
        match self {
            DrumKind::Kick => "Kick",
            DrumKind::Snare => "Snare",
            DrumKind::Tom => "Tom",
            DrumKind::Other => "Other",
        }
    }
    /// AUTO's trigger note: Kick C1 (36), Snare D1 (38), Tom A1 (45); Other has no AUTO (C3, 60).
    pub fn auto_note(self) -> Option<u8> {
        match self {
            DrumKind::Kick => Some(36),
            DrumKind::Snare => Some(38),
            DrumKind::Tom => Some(45),
            DrumKind::Other => None,
        }
    }
    pub fn default_note(self) -> u8 {
        self.auto_note().unwrap_or(60)
    }
}

/// The trigger notes for transients given as (tick, peak level 0…1): those at least
/// `threshold` (0…1, relative to the loudest) get a note at their tick + `offset_ticks`, with
/// the level as velocity.
pub fn trigger_notes(transients: &[(u32, f32)], threshold: f32, offset_ticks: i32, pitch: u8, len: u32) -> Vec<Note> {
    let top = transients.iter().map(|t| t.1).fold(0.0f32, f32::max);
    if top <= 0.0 {
        return Vec::new();
    }
    transients
        .iter()
        .filter(|t| t.1 / top >= threshold)
        .map(|&(tick, lvl)| Note { channel: 0, pitch, velocity: ((lvl / top).sqrt() * 127.0).round().clamp(1.0, 127.0) as u8, pos: (tick as i64 + offset_ticks as i64).max(0) as u32, len, muted: false })
        .collect()
}

/// Set average attack time: the mean distance in samples from each transient to the peak after
/// it (within `look` samples), for the Timing Offset.
pub fn average_attack(mono: &[f32], transients: &[usize], look: usize) -> f32 {
    let mut sum = 0usize;
    let mut n = 0usize;
    for &t in transients {
        let end = (t + look).min(mono.len());
        if t >= end {
            continue;
        }
        let (k, _) = mono[t..end].iter().enumerate().fold((0, 0.0f32), |b, (i, x)| if x.abs() > b.1 { (i, x.abs()) } else { b });
        sum += k;
        n += 1;
    }
    if n == 0 { 0.0 } else { sum as f32 / n as f32 }
}

// ---------------------------------------------------------------------------------------------
// bounce and export

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Normalize {
    Off,
    /// Only downwards, when the peak is over 0 dB.
    #[default]
    OverloadProtection,
    /// Up or down to 0 dB.
    On,
}

impl Normalize {
    pub const ALL: [Normalize; 3] = [Normalize::Off, Normalize::OverloadProtection, Normalize::On];
    pub fn name(self) -> &'static str {
        match self {
            Normalize::Off => "Off",
            Normalize::OverloadProtection => "Overload Protection Only",
            Normalize::On => "On",
        }
    }
    /// The gain applied to samples with this peak.
    pub fn gain(self, peak: f32) -> f32 {
        match self {
            Normalize::Off => 1.0,
            Normalize::OverloadProtection if peak > 1.0 => 1.0 / peak,
            Normalize::OverloadProtection => 1.0,
            Normalize::On if peak > 1e-6 => 1.0 / peak,
            Normalize::On => 1.0,
        }
    }
    pub fn apply(self, samples: &mut [f32]) -> f32 {
        let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let g = self.gain(peak);
        if g != 1.0 {
            for s in samples.iter_mut() {
                *s *= g;
            }
        }
        g
    }
}

/// The end of the sound in interleaved stereo `samples` (frames): after it everything stays
/// under `floor` (for effect tails and trimming silence at the end).
pub fn sound_end(samples: &[f32], floor: f32) -> usize {
    samples.iter().rposition(|x| x.abs() > floor).map_or(0, |i| i / 2 + 1)
}

/// The parts an exported file's name is built from (the Pattern field).
#[derive(Clone, Debug, PartialEq)]
pub enum NameElem {
    Custom(String),
    ProjectName,
    AlternativeName,
    TrackName,
    TrackNumber,
    GroupName,
    RegionName,
    BarRange,
    TimeRange,
    Increment,
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
}

impl NameElem {
    pub fn label(&self) -> &'static str {
        match self {
            NameElem::Custom(_) => "Custom",
            NameElem::ProjectName => "Project Name",
            NameElem::AlternativeName => "Alternative Name",
            NameElem::TrackName => "Track Name",
            NameElem::TrackNumber => "Track Number",
            NameElem::GroupName => "Group Name",
            NameElem::RegionName => "Region Name",
            NameElem::BarRange => "Bar Range",
            NameElem::TimeRange => "Time Range",
            NameElem::Increment => "Increment",
            NameElem::Year => "Year",
            NameElem::Month => "Month",
            NameElem::Day => "Day",
            NameElem::Hour => "Hour",
            NameElem::Minute => "Minute",
            NameElem::Second => "Second",
        }
    }
    /// The elements offered for tracks, or for regions.
    pub fn choices(regions: bool) -> Vec<NameElem> {
        let mut v = vec![NameElem::Custom(String::new()), NameElem::ProjectName, NameElem::AlternativeName];
        if regions {
            v.push(NameElem::RegionName);
        }
        v.extend([NameElem::TrackName, NameElem::TrackNumber]);
        if !regions {
            v.extend([NameElem::GroupName, NameElem::BarRange, NameElem::TimeRange]);
        }
        v.extend([NameElem::Increment, NameElem::Year, NameElem::Month, NameElem::Day, NameElem::Hour, NameElem::Minute, NameElem::Second]);
        v
    }
}

/// What a file name is made from.
#[derive(Clone, Debug, Default)]
pub struct NameCtx {
    pub project: String,
    pub alternative: String,
    pub track: String,
    pub track_number: usize,
    pub group: String,
    pub region: String,
    pub bars: (u32, u32),
    pub seconds: (f64, f64),
    pub increment: usize,
    /// Year, month, day, hour, minute, second.
    pub now: [u32; 6],
}

pub fn build_name(elems: &[NameElem], c: &NameCtx) -> String {
    let mut parts: Vec<String> = Vec::new();
    for e in elems {
        let s = match e {
            NameElem::Custom(s) => s.clone(),
            NameElem::ProjectName => c.project.clone(),
            NameElem::AlternativeName => c.alternative.clone(),
            NameElem::TrackName => c.track.clone(),
            NameElem::TrackNumber => format!("{:02}", c.track_number),
            NameElem::GroupName => c.group.clone(),
            NameElem::RegionName => c.region.clone(),
            NameElem::BarRange => format!("{}-{}", c.bars.0, c.bars.1),
            NameElem::TimeRange => format!("{}-{}", clock(c.seconds.0), clock(c.seconds.1)),
            NameElem::Increment => format!("{}", c.increment),
            NameElem::Year => format!("{:04}", c.now[0]),
            NameElem::Month => format!("{:02}", c.now[1]),
            NameElem::Day => format!("{:02}", c.now[2]),
            NameElem::Hour => format!("{:02}", c.now[3]),
            NameElem::Minute => format!("{:02}", c.now[4]),
            NameElem::Second => format!("{:02}", c.now[5]),
        };
        if !s.is_empty() {
            parts.push(s);
        }
    }
    let name = parts.join("_");
    name.chars().map(|ch| if matches!(ch, '/' | '\\' | ':' | '\0') { '-' } else { ch }).collect()
}

fn clock(s: f64) -> String {
    let t = s.max(0.0) as u64;
    format!("{:02}.{:02}", t / 60, t % 60)
}

/// The civil date and time of a Unix time (UTC): year, month, day, hour, minute, second.
pub fn civil(unix: u64) -> [u32; 6] {
    let days = (unix / 86400) as i64;
    let rem = unix % 86400;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    [y as u32, m as u32, d as u32, (rem / 3600) as u32, (rem % 3600 / 60) as u32, (rem % 60) as u32]
}

// ---------------------------------------------------------------------------------------------
// the sidecar text (one line per track: key=value pairs separated by tabs)

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl StripSet {
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let d = StripSet::default();
        let mut v: Vec<(String, String)> = Vec::new();
        let mut put = |k: &str, s: String| v.push((k.to_string(), s));
        if self.solo_safe {
            put("solo_safe", "1".into());
        }
        if self.pan_mode != d.pan_mode {
            put("pan_mode", match self.pan_mode { PanMode::Balance => "balance", PanMode::StereoPan => "stereo", PanMode::Binaural => "binaural" }.into());
        }
        if self.stereo_pan != d.stereo_pan {
            put("stereo_pan", format!("{},{}", self.stereo_pan.left, self.stereo_pan.right));
        }
        if self.binaural != d.binaural {
            let b = &self.binaural;
            put("binaural", format!("{},{},{},{},{},{},{},{},{},{},{}", b.angle, b.elevation, b.distance, b.spread, b.planar as u8, b.vertical_offset, b.tilt_amount, b.tilt_direction, b.doppler as u8, b.size, b.diffuse_field as u8));
        }
        if self.format != d.format {
            put("format", self.format.name().into());
        }
        if let Some(c) = self.color {
            put("color", c.to_string());
        }
        if self.sends.iter().any(|s| *s != SendSet::default()) {
            put("sends", self.sends.iter().map(|s| format!("{}:{}:{}:{}", s.post_pan as u8, s.independent_pan.map_or("-".into(), |p| p.to_string()), s.low_latency_safe as u8, s.bypass as u8)).collect::<Vec<_>>().join(","));
        }
        if self.phase_invert {
            put("phase", "1".into());
        }
        if self.input_gain != 0.0 {
            put("input_gain", self.input_gain.to_string());
        }
        if !self.notes.is_empty() {
            put("notes", self.notes.clone());
        }
        v
    }

    pub fn set_pair(&mut self, k: &str, val: &str) -> bool {
        let f = |s: &str| s.parse::<f32>().unwrap_or(0.0);
        match k {
            "solo_safe" => self.solo_safe = val == "1",
            "pan_mode" => self.pan_mode = match val { "stereo" => PanMode::StereoPan, "binaural" => PanMode::Binaural, _ => PanMode::Balance },
            "stereo_pan" => {
                let p: Vec<f32> = val.split(',').map(f).collect();
                if p.len() == 2 {
                    self.stereo_pan = StereoPan { left: p[0], right: p[1] };
                }
            }
            "binaural" => {
                let p: Vec<f32> = val.split(',').map(f).collect();
                if p.len() == 11 {
                    self.binaural = Binaural { angle: p[0], elevation: p[1], distance: p[2], spread: p[3], planar: p[4] != 0.0, vertical_offset: p[5], tilt_amount: p[6], tilt_direction: p[7], doppler: p[8] != 0.0, size: p[9], diffuse_field: p[10] != 0.0 };
                }
            }
            "format" => self.format = InputFormat::ALL.into_iter().find(|x| x.name() == val).unwrap_or_default(),
            "color" => self.color = val.parse().ok(),
            "sends" => {
                self.sends = val
                    .split(',')
                    .map(|s| {
                        let p: Vec<&str> = s.split(':').collect();
                        SendSet { post_pan: p.first() == Some(&"1"), independent_pan: p.get(1).and_then(|x| x.parse().ok()), low_latency_safe: p.get(2) == Some(&"1"), bypass: p.get(3) == Some(&"1") }
                    })
                    .collect()
            }
            "phase" => self.phase_invert = val == "1",
            "input_gain" => self.input_gain = f(val),
            "notes" => self.notes = val.to_string(),
            _ => return false,
        }
        true
    }
}

impl TrackSet {
    /// The settings as one line (empty when all are the defaults).
    pub fn to_line(&self) -> String {
        let d = TrackSet::default();
        let mut v = self.strip.to_pairs();
        let mut put = |k: &str, s: String| v.push((k.to_string(), s));
        if let Some(i) = &self.icon {
            put("icon", i.clone());
        }
        if self.off {
            put("off", if self.offline { "offline" } else { "1" }.into());
        }
        if self.protected {
            put("protected", "1".into());
        }
        if self.frozen {
            put("frozen", "1".into());
        }
        if self.freeze_mode != d.freeze_mode {
            put("freeze_mode", "source".into());
        }
        if self.groove {
            put("groove", "1".into());
        }
        if self.match_groove {
            put("match_groove", "1".into());
        }
        if self.default_region != d.default_region {
            put("default_region", self.default_region.name().into());
        }
        if let Some(e) = &self.external {
            put("external", format!("{}|{}|{}|{}|{}|{}|{}", e.channel, e.use_plugin as u8, e.program.map_or(-1, |p| p as i32), e.bank.0, e.bank.1, e.assigns.iter().map(|a| format!("{}.{}", a.0, a.1)).collect::<Vec<_>>().join("/"), e.port));
        }
        if let Some(s) = self.same_strip {
            put("same_strip", s.to_string());
        }
        if self.assign != d.assign {
            put("assign", if self.assign == Assign::Folder { "folder" } else { "none" }.into());
        }
        if let Some(k) = self.knob_send {
            put("knob_send", k.to_string());
        }
        if self.midi != d.midi {
            let m = &self.midi;
            put("midi", format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", m.internal_in.map_or(0, |x| x), m.in_channel, m.out_channel, m.no_reset as u8, m.record_both as u8, m.hermode, m.delay_ms, m.delay_in_ms as u8, m.staff_style, m.input));
        }
        if let Some(a) = &self.articulation_set {
            put("articulation_set", a.clone());
        }
        if self.renamed {
            put("renamed", "1".into());
        }
        if self.q_reference {
            put("q_reference", "1".into());
        }
        if self.record_region_automation {
            put("record_region_automation", "1".into());
        }
        if self.auto_trim {
            put("auto_trim", "1".into());
        }
        if self.auto_relative {
            put("auto_relative", "1".into());
        }
        let keys = |l: &[(u8, Option<(u32, u32)>)]| l.iter().map(|(p, pl)| match pl { Some((s, i)) => format!("{p}.{s}.{i}"), None => p.to_string() }).collect::<Vec<_>>().join(",");
        if !self.auto_lanes.is_empty() {
            put("auto_lanes", keys(&self.auto_lanes));
        }
        if self.auto_lanes_open {
            put("auto_lanes_open", "1".into());
        }
        if !self.auto_muted.is_empty() {
            put("auto_muted", keys(&self.auto_muted));
        }
        if self.show_region_automation {
            put("show_region_automation", "1".into());
        }
        v.iter().map(|(k, s)| format!("{k}={}", esc(s))).collect::<Vec<_>>().join("\t")
    }

    pub fn from_line(line: &str) -> TrackSet {
        let mut t = TrackSet::default();
        let keys = |val: &str| -> Vec<(u8, Option<(u32, u32)>)> {
            val.split(',')
                .filter(|s| !s.is_empty())
                .filter_map(|s| {
                    let p: Vec<&str> = s.split('.').collect();
                    let param = p.first()?.parse().ok()?;
                    Some((param, if p.len() == 3 { Some((p[1].parse().ok()?, p[2].parse().ok()?)) } else { None }))
                })
                .collect()
        };
        for pair in line.split('\t') {
            let Some((k, raw)) = pair.split_once('=') else { continue };
            let val = unesc(raw);
            if t.strip.set_pair(k, &val) {
                continue;
            }
            match k {
                "icon" => t.icon = Some(val),
                "off" => {
                    t.off = true;
                    t.offline = val == "offline";
                }
                "protected" => t.protected = true,
                "frozen" => t.frozen = true,
                "freeze_mode" => t.freeze_mode = FreezeMode::SourceOnly,
                "groove" => t.groove = true,
                "match_groove" => t.match_groove = true,
                "default_region" => t.default_region = RegionType::ALL.into_iter().find(|x| x.name() == val).unwrap_or_default(),
                "external" => {
                    let p: Vec<&str> = val.splitn(7, '|').collect();
                    if p.len() == 7 {
                        let mut assigns = [(0u8, 0u8); 5];
                        for (k, a) in p[5].split('/').take(5).enumerate() {
                            if let Some((c, v)) = a.split_once('.') {
                                assigns[k] = (c.parse().unwrap_or(0), v.parse().unwrap_or(0));
                            }
                        }
                        t.external = Some(External { channel: p[0].parse().unwrap_or(1), use_plugin: p[1] == "1", program: p[2].parse::<i32>().ok().filter(|x| *x >= 0).map(|x| x as u8), bank: (p[3].parse().unwrap_or(0), p[4].parse().unwrap_or(0)), assigns, port: p[6].to_string() });
                    }
                }
                "same_strip" => t.same_strip = val.parse().ok(),
                "assign" => t.assign = if val == "folder" { Assign::Folder } else { Assign::NotAssigned },
                "knob_send" => t.knob_send = val.parse().ok(),
                "midi" => {
                    let p: Vec<&str> = val.splitn(10, '|').collect();
                    if p.len() == 10 {
                        t.midi = MidiSet { internal_in: p[0].parse::<u64>().ok().filter(|x| *x != 0), in_channel: p[1].parse().unwrap_or(0), out_channel: p[2].parse().unwrap_or(0), no_reset: p[3] == "1", record_both: p[4] == "1", hermode: p[5].parse().unwrap_or(3), delay_ms: p[6].parse().unwrap_or(0.0), delay_in_ms: p[7] == "1", staff_style: p[8].to_string(), input: p[9].to_string() };
                    }
                }
                "articulation_set" => t.articulation_set = Some(val),
                "renamed" => t.renamed = true,
                "q_reference" => t.q_reference = true,
                "record_region_automation" => t.record_region_automation = true,
                "auto_trim" => t.auto_trim = true,
                "auto_relative" => t.auto_relative = true,
                "auto_lanes" => t.auto_lanes = keys(&val),
                "auto_lanes_open" => t.auto_lanes_open = true,
                "auto_muted" => t.auto_muted = keys(&val),
                "show_region_automation" => t.show_region_automation = true,
                _ => {}
            }
        }
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_count_up_from_a_trailing_number() {
        assert_eq!(numbered_names("Vox 1", 3), ["Vox 1", "Vox 2", "Vox 3"]);
        assert_eq!(numbered_names("Gtr 09", 2), ["Gtr 09", "Gtr 10"]);
        assert_eq!(numbered_names("Pad", 2), ["Pad", "Pad"]);
        assert_eq!(numbered_names("Solo 7", 1), ["Solo 7"]);
    }

    #[test]
    fn long_strip_names_are_cut_in_the_middle() {
        assert_eq!(middle_ellipsis("Short", 11), "Short");
        let s = middle_ellipsis("Background Vocals Double Left", 14);
        assert_eq!(s.chars().count(), 14);
        assert!(s.contains('…') && s.starts_with("Backgro") && s.ends_with("Left"));
        let (a, b) = two_lines("Lead Vocal Double", 11);
        assert!(a.chars().count() <= 11 && b.chars().count() <= 11, "{a} / {b}");
        assert_eq!(two_lines("Kick", 11), ("Kick".to_string(), String::new()));
    }

    #[test]
    fn stereo_pan_places_each_side_and_swaps() {
        let mut p = StereoPan::default();
        let m = p.matrix();
        assert!((m[0][0] - 1.0).abs() < 1e-6 && m[0][1].abs() < 1e-6 && m[1][0].abs() < 1e-6 && (m[1][1] - 1.0).abs() < 1e-6);
        p.swap();
        assert!(p.swapped());
        let m = p.matrix();
        assert!(m[0][0].abs() < 1e-6 && (m[0][1] - 1.0).abs() < 1e-6, "left now comes out right");
        p.swap();
        p.set(0.5, 0.5);
        assert!((p.left - 0.25).abs() < 1e-6 && (p.right - 0.75).abs() < 1e-6);
        p.set(1.0, 1.0);
        assert!((p.right - 1.0).abs() < 1e-6 && (p.left - 0.0).abs() < 1e-6, "the spread stays inside");
        // both sides at the centre: a mono sum at equal power
        p.set(0.0, 0.0);
        let m = p.matrix();
        assert!((m[0][0] - m[1][0]).abs() < 1e-6 && (m[0][0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
    }

    #[test]
    fn input_formats() {
        let (mut l, mut r) = (vec![1.0, 0.0], vec![0.0, 1.0]);
        InputFormat::Mono.apply(&mut l, &mut r);
        assert_eq!((l.clone(), r.clone()), (vec![0.5, 0.5], vec![0.5, 0.5]));
        let (mut l, mut r) = (vec![1.0, 2.0], vec![3.0, 4.0]);
        InputFormat::Left.apply(&mut l, &mut r);
        assert_eq!(r, vec![1.0, 2.0]);
        let (mut l, mut r) = (vec![1.0, 2.0], vec![3.0, 4.0]);
        InputFormat::Right.apply(&mut l, &mut r);
        assert_eq!(l, vec![3.0, 4.0]);
        assert_eq!(InputFormat::Stereo.meter_columns(), 2);
        assert_eq!(InputFormat::Left.meter_columns(), 1);
    }

    #[test]
    fn binaural_puck_and_ears() {
        let mut b = Binaural::default();
        b.set_puck(1.0, 0.0);
        assert!((b.angle - 90.0).abs() < 1e-3 && (b.distance - b.size).abs() < 1e-3);
        let e = b.ears(b.angle);
        assert!(e.gain_r > e.gain_l && e.delay_l > 0.0 && e.delay_r == 0.0, "a source at the right reaches the left ear later and softer");
        assert!(e.delay_l < 0.001);
        b.reset_position();
        assert_eq!(b.angle, 0.0);
        let e = b.ears(0.0);
        assert!((e.gain_l - e.gain_r).abs() < 1e-6 && e.delay_l == 0.0);
        // farther is softer
        let near = Binaural { distance: 0.5, ..Binaural::default() }.ears(0.0).gain_l;
        let far = Binaural { distance: 3.0, ..Binaural::default() }.ears(0.0).gain_l;
        assert!(near > far);
        // spherical: the centre of the plane is overhead, the lower half behind
        let mut s = Binaural { planar: false, ..Binaural::default() };
        s.set_puck(0.0, 0.0);
        assert!((s.elevation - 90.0).abs() < 1e-3);
        s.set_puck(0.0, -0.5);
        assert!((s.angle.abs() - 180.0).abs() < 1e-3 && s.ears(s.angle).behind);
        // planar tilt raises the front
        let mut t = Binaural { tilt_amount: 30.0, tilt_direction: 0.0, ..Binaural::default() };
        t.set_puck(0.0, 0.5);
        assert!(t.elevation > 20.0, "{}", t.elevation);
        // sides of a stereo strip
        let (l, r) = Binaural { angle: 170.0, spread: 60.0, ..Binaural::default() }.sides();
        assert!((l - 140.0).abs() < 1e-3 && (r + 160.0).abs() < 1e-3);
    }

    #[test]
    fn sorts_tracks() {
        let k = |name: &str, ch: u32, used: bool| SortKey { name: name.into(), midi_channel: ch, used, ..Default::default() };
        let keys = [k("Audio 10", 3, false), k("audio 2", 1, true), k("Bass", 2, true)];
        assert_eq!(sort_order(&keys, SortBy::TrackName), [1, 0, 2]);
        assert_eq!(sort_order(&keys, SortBy::MidiChannel), [1, 2, 0]);
        assert_eq!(sort_order(&keys, SortBy::Used), [1, 2, 0]);
    }

    #[test]
    fn groove_pulls_note_starts_to_the_groove_tracks() {
        let mut notes = vec![(0, 100, 60, 90), (470, 600, 62, 90), (1000, 1100, 64, 90)];
        match_groove(&mut notes, &[10, 480, 2000], 60);
        assert_eq!(notes[0], (10, 110, 60, 90));
        assert_eq!(notes[1], (480, 610, 62, 90));
        assert_eq!(notes[2], (1000, 1100, 64, 90), "nothing near: stays");
    }

    #[test]
    fn drum_triggers() {
        let tr = [(0, 1.0), (480, 0.2), (960, 0.7)];
        let n = trigger_notes(&tr, 0.5, -10, 36, 60);
        assert_eq!(n.len(), 2);
        assert_eq!((n[0].pos, n[0].pitch, n[0].velocity), (0, 36, 127));
        assert_eq!(n[1].pos, 950);
        assert_eq!(DrumKind::Kick.auto_note(), Some(36));
        assert_eq!(DrumKind::Snare.auto_note(), Some(38));
        assert_eq!(DrumKind::Tom.auto_note(), Some(45));
        assert_eq!((DrumKind::Other.auto_note(), DrumKind::Other.default_note()), (None, 60));
        let mono = [0.0, 0.1, 0.9, 0.2, 0.0, 0.0, 0.3, 0.8];
        assert_eq!(average_attack(&mono, &[0, 5], 3), 2.0);
    }

    #[test]
    fn normalizing() {
        assert_eq!(Normalize::Off.gain(2.0), 1.0);
        assert_eq!(Normalize::OverloadProtection.gain(2.0), 0.5);
        assert_eq!(Normalize::OverloadProtection.gain(0.5), 1.0);
        assert_eq!(Normalize::On.gain(0.5), 2.0);
        let mut s = vec![0.25, -0.5];
        Normalize::On.apply(&mut s);
        assert_eq!(s, vec![0.5, -1.0]);
        assert_eq!(sound_end(&[0.0, 0.0, 0.5, 0.0, 0.0, 0.0], 0.001), 2);
    }

    #[test]
    fn export_names() {
        let c = NameCtx { project: "Song".into(), track: "Lead/Vox".into(), track_number: 3, bars: (1, 9), increment: 2, now: civil(1_794_000_000), ..Default::default() };
        assert_eq!(build_name(&[NameElem::ProjectName, NameElem::TrackNumber, NameElem::TrackName], &c), "Song_03_Lead-Vox");
        assert_eq!(build_name(&[NameElem::Custom("Mix".into()), NameElem::BarRange, NameElem::Increment], &c), "Mix_1-9_2");
        assert_eq!(civil(0), [1970, 1, 1, 0, 0, 0]);
        assert_eq!(civil(951_782_400), [2000, 2, 29, 0, 0, 0]);
        assert!(NameElem::choices(true).contains(&NameElem::RegionName) && !NameElem::choices(false).contains(&NameElem::RegionName));
    }

    #[test]
    fn settings_round_trip_through_the_sidecar_line() {
        let mut t = TrackSet::default();
        assert_eq!(t.to_line(), "");
        t.strip.solo_safe = true;
        t.strip.pan_mode = PanMode::Binaural;
        t.strip.binaural.angle = -45.0;
        t.strip.stereo_pan = StereoPan { left: 0.5, right: -0.5 };
        t.strip.format = InputFormat::Left;
        t.strip.color = Some(17);
        t.strip.send_mut(1).independent_pan = Some(0.25);
        t.strip.send_mut(0).post_pan = true;
        t.strip.notes = "line one\nline\ttwo".into();
        t.icon = Some("custom:kick.png".into());
        t.off = true;
        t.offline = true;
        t.protected = true;
        t.frozen = true;
        t.freeze_mode = FreezeMode::SourceOnly;
        t.groove = true;
        t.default_region = RegionType::Pattern;
        t.external = Some(External { port: "USB | Synth".into(), channel: 5, use_plugin: true, program: Some(12), bank: (1, 2), assigns: [(1, 10), (7, 100), (0, 0), (0, 0), (74, 64)] });
        t.same_strip = Some(42);
        t.assign = Assign::Folder;
        t.knob_send = Some(2);
        t.midi.out_channel = 17;
        t.midi.input = "Musical Typing".into();
        t.articulation_set = Some("Strings".into());
        t.renamed = true;
        t.auto_trim = true;
        t.auto_lanes = vec![(7, None), (0, Some((3, 12)))];
        t.auto_muted = vec![(10, None)];
        t.auto_lanes_open = true;
        let back = TrackSet::from_line(&t.to_line());
        assert_eq!(back, t);
    }

    #[test]
    fn palette_files_round_trip_and_override() {
        let mut cols: Vec<[u8; 3]> = (0..(PALETTE_COLS * PALETTE_ROWS) as u8).map(default_palette).collect();
        cols[3] = [1, 2, 250];
        let t = palette_text("Mine", &cols);
        let (n, back) = parse_palette(&t).unwrap();
        assert_eq!((n.as_str(), back.clone()), ("Mine", cols.clone()));
        assert!(parse_palette("not a palette\n00ff00").is_none());
        assert!(parse_palette("bramble-studio-palette 1\nzz00ff").is_none());
        set_custom_palette(Some(back[..4].to_vec()));
        assert_eq!(palette(3), [1, 2, 250]);
        // past the custom list: the built-in colours
        assert_eq!(palette(10), default_palette(10));
        set_custom_palette(None);
        assert_eq!(palette(3), default_palette(3));
    }

    #[test]
    fn palette_has_distinct_colours() {
        let all: std::collections::HashSet<[u8; 3]> = (0..(PALETTE_COLS * PALETTE_ROWS) as u8).map(default_palette).collect();
        assert_eq!(all.len(), PALETTE_COLS * PALETTE_ROWS);
    }
}
