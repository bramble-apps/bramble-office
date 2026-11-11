//! "Tonewheel B3": our tonewheel organ, a stand-in for Logic's Vintage B3 Organ.
//!
//! The organ is modelled the way the real one works rather than as voices: 91 tonewheels spin all
//! the time (frequencies from the real gear ratios, so the slightly-off equal temperament is
//! there), and each key closes nine contacts that connect wheels to the drawbar busses. There are
//! two manuals and a pedalboard, each with its own registration (9 + 9 + 2 drawbars), twelve
//! preset keys per manual (MIDI notes 24..35) and a Morph between upper presets. The upper manual
//! has the single-trigger percussion; each register goes through a vibrato/chorus scanner (a
//! modulated delay line), then the effects (EQ, wah, tube distortion in any order, reverb before
//! or after the cabinet) and a rotary speaker cabinet (horn and drum rotors with Doppler,
//! amplitude modulation, cabinet types, deflector, motor modes, brake, microphones). Key click
//! comes from the contacts closing. Polyphony is unlimited (all keys share the wheels).
//!
//! Parameter ids 0..29 are the first version's (library patches use them); the rest follow.

use std::sync::LazyLock;

use crate::dsp::{self, Biquad, CHUNK, Delay, OnePole, Rng, Room};
use crate::poly::{Channel, Params};
use lg_dsp::{Instrument, Param, Plugin, Unit};

const VIB: &[&str] = &["Off", "V1", "V2", "V3", "C1", "C2", "C3"];
const ROTOR: &[&str] = &["Off", "Brake", "Slow", "Fast"];
const PERC_H: &[&str] = &["2nd", "3rd"];
pub const SPEED_CTL: &[&str] = &["Off", "ModWheel", "ModWhl Toggle", "ModWhl Temp", "Touch", "Touch Temp", "SusPdl Toggle", "SusPdl Temp", "CC 18/19 Toggle", "CC 18/19 Temp"];
pub const CABINETS: &[&str] = &["Real Cabinet", "Wood", "Proline", "Single", "Split", "Wood & Horn IR", "Proline & Horn IR", "Split & Horn IR"];
pub const MOTOR: &[&str] = &["Normal", "Inv", "910", "Sync"];
pub const MICS: &[&str] = &["Dynamic", "Condenser", "Mid-Side Mic"];
pub const WAH: &[&str] = &["Classic", "Retro", "Modern", "Opto Wah 1", "Opto Wah 2", "Resonant LP", "Resonant HP", "Peak"];
pub const DIST: &[&str] = &["Growl", "Bity", "Nasty", "6550 AB V3", "6550 AB V2", "6550 GL"];
pub const REVERB: &[&str] = &["Air", "Box", "Small", "Medium", "Large", "Big", "Spring"];
pub const CHAIN: &[&str] = &["EQ-Wah-Dist", "EQ-Dist-Wah", "Wah-Dist-EQ", "Dist-EQ-Wah"];
pub const KBD_MODE: &[&str] = &["Single", "Split", "Multi"];
pub const HARDWARE: &[&str] = &["Off", "Roland VK or Korg CX", "Hammond Suzuki", "Native Instruments B4D", "Nord Electro", "Crumar"];
pub const MORPH_RANGE: &[&str] = &["A#", "A", "G#", "G", "F#", "F", "E", "D#", "D", "C#"];

/// First ids of the drawbar groups and the preset keys' stored registrations.
pub const UPPER: u32 = 0;
pub const LOWER: u32 = 30;
pub const PEDAL: u32 = 39;
pub const PRESETS_U: u32 = 200;
pub const PRESETS_L: u32 = 308;
/// A MIDI controller choice: 0 off, 1..=119 the controller, 120 channel aftertouch, 121 Learn.
pub const CTL_TOUCH: f32 = 120.0;
pub const CTL_LEARN: f32 = 121.0;

const BASE: &[Param] = &[
    Param::new(0, "Drawbar 16'", 0.0, 8.0, 8.0, Unit::None),
    Param::new(1, "Drawbar 5 1/3'", 0.0, 8.0, 8.0, Unit::None),
    Param::new(2, "Drawbar 8'", 0.0, 8.0, 8.0, Unit::None),
    Param::new(3, "Drawbar 4'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(4, "Drawbar 2 2/3'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(5, "Drawbar 2'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(6, "Drawbar 1 3/5'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(7, "Drawbar 1 1/3'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(8, "Drawbar 1'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(9, "Percussion", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(10, "Percussion Harmonic", PERC_H, 1.0),
    Param::new(11, "Percussion Time Fast", 0.1, 3.0, 0.5, Unit::Seconds),
    Param::new(12, "Percussion Volume Normal", -24.0, 6.0, 0.0, Unit::Db),
    Param::new(13, "Click Minimum", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::new(14, "Click Maximum", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::menu(15, "Vibrato Type", VIB, 6.0),
    Param::new(16, "Leakage", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(17, "Distortion Drive", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::menu(18, "Rotation", ROTOR, 2.0),
    Param::new(19, "Horn Slow", 0.1, 3.0, 0.83, Unit::Hz),
    Param::new(20, "Horn Fast", 2.0, 10.0, 6.75, Unit::Hz),
    Param::new(21, "Drum Slow", 0.1, 3.0, 0.7, Unit::Hz),
    Param::new(22, "Drum Fast", 2.0, 10.0, 5.9, Unit::Hz),
    Param::new(23, "Acceleration", 0.0, 2.0, 1.0, Unit::Ratio),
    Param::new(24, "Mic Horn Width", 0.0, 1.0, 0.8, Unit::Ratio),
    Param::new(25, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(26, "Tonal Balance", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::menu(27, "Speed MIDI Control", SPEED_CTL, 1.0),
    Param::new(28, "Pitchbend Up", 0.0, 12.0, 0.0, Unit::Semitones),
    Param::new(29, "Rotor Mix", 0.0, 1.0, 1.0, Unit::Ratio),
    // lower manual and pedals
    Param::new(30, "Lower Drawbar 16'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(31, "Lower Drawbar 5 1/3'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(32, "Lower Drawbar 8'", 0.0, 8.0, 8.0, Unit::None),
    Param::new(33, "Lower Drawbar 4'", 0.0, 8.0, 4.0, Unit::None),
    Param::new(34, "Lower Drawbar 2 2/3'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(35, "Lower Drawbar 2'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(36, "Lower Drawbar 1 3/5'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(37, "Lower Drawbar 1 1/3'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(38, "Lower Drawbar 1'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(39, "Pedal Drawbar 16'", 0.0, 8.0, 6.0, Unit::None),
    Param::new(40, "Pedal Drawbar 8'", 0.0, 8.0, 3.0, Unit::None),
    // scanner vibrato
    Param::new(41, "Vibrato Upper", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(42, "Vibrato Lower", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(43, "Vibrato Mode", &["Model", "Real"], 0.0),
    Param::new(44, "Vibrato Rate", 3.0, 10.0, 6.9, Unit::Hz),
    Param::new(45, "Chorus Depth", 0.0, 1.0, 0.5, Unit::Ratio),
    // percussion
    Param::menu(46, "Percussion Time", &["Slow", "Fast"], 1.0),
    Param::menu(47, "Percussion Volume", &["Soft", "Normal"], 1.0),
    Param::new(48, "Percussion Time Slow", 0.3, 6.0, 1.6, Unit::Seconds),
    Param::new(49, "Percussion Volume Soft", -30.0, 0.0, -6.0, Unit::Db),
    Param::menu(50, "Percussion Mode", &["Poly", "Mono"], 0.0),
    Param::menu(51, "Perc on Preset", &["B0", "All"], 1.0),
    Param::new(52, "Percussion Upper Level", -12.0, 6.0, 0.0, Unit::Db),
    Param::new(53, "Percussion Velocity", 0.0, 1.0, 0.0, Unit::Ratio),
    // master and click
    Param::new(54, "Tune", -100.0, 100.0, 0.0, Unit::Cents),
    Param::new(55, "Expression", 0.0, 1.0, 1.0, Unit::Ratio),
    Param::new(56, "Click On", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::new(57, "Click Off", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::new(58, "Click Pedal", 0.0, 1.0, 0.2, Unit::Ratio),
    Param::new(59, "Click Velocity", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(60, "Click Color", 0.0, 1.0, 0.5, Unit::Ratio),
    // keyboard
    Param::menu(61, "Keyboard Mode", KBD_MODE, 0.0),
    Param::new(62, "Split Pedal/Lower", 24.0, 108.0, 48.0, Unit::None),
    Param::new(63, "Split Lower/Upper", 24.0, 108.0, 60.0, Unit::None),
    Param::new(64, "Upper Transpose", -2.0, 2.0, 0.0, Unit::Octaves),
    Param::new(65, "Lower Transpose", -2.0, 2.0, 0.0, Unit::Octaves),
    Param::new(66, "Pedal Transpose", -2.0, 2.0, 0.0, Unit::Octaves),
    Param::new(67, "Upper Channel", 1.0, 16.0, 1.0, Unit::None),
    Param::new(68, "Lower Channel", 1.0, 16.0, 2.0, Unit::None),
    Param::new(69, "Pedal Channel", 1.0, 16.0, 3.0, Unit::None),
    // presets and morph
    Param::new(70, "Upper Preset Key", 0.0, 11.0, 11.0, Unit::None),
    Param::new(71, "Lower Preset Key", 0.0, 11.0, 11.0, Unit::None),
    Param::new(72, "Select via Keyboard", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(73, "Morph Mode", &["Step", "Linear"], 0.0),
    Param::menu(74, "Morph Range", MORPH_RANGE, 0.0),
    Param::new(75, "Upper Morph", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(76, "Morph MIDI Controller", 0.0, 121.0, 0.0, Unit::None),
    Param::menu(77, "Edit Preset Key", &["All", "A# and B only"], 0.0),
    // rotor cabinet
    Param::menu(80, "Cabinet Type", CABINETS, 1.0),
    Param::new(81, "Deflector", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(82, "Max Rate", 0.5, 2.0, 1.0, Unit::Ratio),
    Param::menu(83, "Motor Control", MOTOR, 0.0),
    Param::new(84, "Brake Dry Level", 0.0, 1.0, 0.7, Unit::Ratio),
    Param::menu(85, "Brake Output", &["Dry", "Rotor"], 1.0),
    Param::new(86, "Horn Position", 0.0, 360.0, 0.0, Unit::None),
    Param::new(87, "Drum Position", 0.0, 360.0, 0.0, Unit::None),
    Param::menu(88, "Horn Mic Type", MICS, 1.0),
    Param::menu(89, "Drum Mic Type", MICS, 0.0),
    Param::menu(90, "Mic Position", &["Front", "Rear"], 0.0),
    Param::new(91, "Mic Drum Width", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(92, "Mic Distance", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(93, "Mic Angle", 0.0, 180.0, 90.0, Unit::None),
    Param::new(94, "Mic Balance", -1.0, 1.0, 0.0, Unit::Ratio),
    // effects
    Param::new(100, "Effects", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(101, "EQ", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(102, "EQ Low", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(103, "EQ Mid", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(104, "EQ High", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(105, "EQ Level", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(106, "Wah", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(107, "Wah Type", WAH, 0.0),
    Param::new(108, "Wah Range", 0.0, 1.0, 1.0, Unit::Ratio),
    Param::new(109, "Wah Bite", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(110, "Wah Sweep MIDI Ctrl", 0.0, 121.0, 0.0, Unit::None),
    Param::new(111, "Distortion", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(112, "Distortion Type", DIST, 0.0),
    Param::new(113, "Distortion Tone", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::new(114, "Reverb", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(115, "Reverb Type", REVERB, 3.0),
    Param::new(116, "Reverb Level", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::menu(117, "Reverb Position", &["Pre", "Post"], 1.0),
    Param::menu(118, "Effect Chain", CHAIN, 0.0),
    Param::menu(119, "Pedal Effects", &["Bypass", "FX"], 0.0),
    // expert
    Param::new(120, "Upper Stretch", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(121, "Lower Stretch", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(122, "Warmth", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(123, "Pitchbend Down", 0.0, 13.0, 0.0, Unit::Semitones),
    Param::new(124, "Sustain Upper", 0.0, 3.0, 0.0, Unit::Seconds),
    Param::new(125, "Sustain Lower", 0.0, 3.0, 0.0, Unit::Seconds),
    Param::new(126, "Sustain Pedals", 0.0, 3.0, 0.0, Unit::Seconds),
    Param::menu(127, "Sustain Mode", &["Smart", "Normal"], 0.0),
    Param::new(128, "Filter Age", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(129, "Drawbar Leak", 0.0, 1.0, 0.1, Unit::Ratio),
    Param::new(130, "Crosstalk", 0.0, 1.0, 0.1, Unit::Ratio),
    Param::new(131, "Random FM", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(132, "Maximum Wheels", 12.0, 91.0, 91.0, Unit::None),
    Param::new(133, "Lower Volume", -24.0, 6.0, 0.0, Unit::Db),
    Param::new(134, "Pedals Volume", -24.0, 6.0, 0.0, Unit::Db),
    Param::new(135, "Shape", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(136, "Bass Filter", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(137, "Extended Bass", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(138, "Ultrabass", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(139, "Hardware Controller", HARDWARE, 0.0),
];

/// The preset keys' factory registrations (C..B), upper and lower manual.
pub const FACTORY_U: [&str; 12] = ["888000000", "005320000", "004432000", "008740000", "004544222", "005403000", "004675300", "005644320", "006876540", "327645222", "888800000", "888000000"];
pub const FACTORY_L: [&str; 12] = ["008400000", "004545440", "004423220", "007373430", "004544220", "006644322", "005642200", "006845433", "008030000", "427866244", "838000000", "008400000"];

fn factory(manual: usize, key: usize, bar: usize) -> f32 {
    let s = if manual == 0 { FACTORY_U[key] } else { FACTORY_L[key] };
    (s.as_bytes()[bar] - b'0') as f32
}

static ALL: LazyLock<Vec<Param>> = LazyLock::new(|| {
    let mut v = BASE.to_vec();
    for (m, base) in [PRESETS_U, PRESETS_L].into_iter().enumerate() {
        for k in 0..12 {
            for d in 0..9 {
                v.push(Param::new(base + (k * 9 + d) as u32, if m == 0 { "Upper Preset Drawbar" } else { "Lower Preset Drawbar" }, 0.0, 8.0, factory(m, k, d), Unit::None).hide());
            }
        }
    }
    v
});

/// Every parameter (the visible ones, then the preset keys' registrations).
pub fn params() -> &'static [Param] {
    &ALL
}

const WHEELS: usize = 92; // index 1..=91
/// Footage offsets in semitones from the 8' pitch.
const FOOT: [i32; 9] = [-12, 7, 0, 12, 19, 24, 28, 31, 36];

/// Real tonewheel gear ratios for C..B (driving/driven teeth); frequency = 20 Hz × ratio × teeth.
const GEARS: [(f32, f32); 12] = [(85.0, 104.0), (71.0, 82.0), (67.0, 73.0), (105.0, 108.0), (103.0, 100.0), (84.0, 77.0), (74.0, 64.0), (98.0, 80.0), (96.0, 74.0), (88.0, 64.0), (67.0, 46.0), (108.0, 70.0)];

fn wheel_hz(w: usize) -> f32 {
    // wheel 1 = C at 32.7 Hz; 12 wheels per octave with 2,4,8..192 teeth
    let i = w - 1;
    let note = i % 12;
    let oct = i / 12;
    let teeth = [2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 192.0][oct.min(7)];
    let (a, b) = GEARS[note];
    let mut f = 20.0 * a / b * teeth;
    if oct >= 7 {
        // 192 teeth = 1.5 × 128: the real organ uses different gears there; keep the octave relation
        f = 20.0 * a / b * 256.0;
    }
    f
}

struct Rotor {
    angle: f32,
    speed: f32,
}

/// One vibrato / chorus scanner (each register has its own).
struct Scanner {
    dl: Delay,
    phase: f32,
    tone: OnePole,
    air: OnePole,
}

impl Scanner {
    fn new(rate: f32) -> Scanner {
        let mut air = OnePole::default();
        air.lowpass_hz(2500.0, rate);
        Scanner { dl: Delay::new((rate * 0.004) as usize), phase: 0.0, tone: OnePole::default(), air }
    }
    /// `kind` 1..=6 (V1..C3); `real`: the delay-line circuit; `depth`: the chorus's dry/wet mix.
    #[inline]
    fn run(&mut self, x: f32, kind: usize, real: bool, hz: f32, depth: f32, rate: f32) -> f32 {
        self.dl.push(x);
        if kind == 0 {
            return x;
        }
        let amount = [0.25, 0.5, 0.85][(kind - 1) % 3] * 0.001 * rate;
        let t = self.phase;
        let tri = if t < 0.5 { 4.0 * t - 1.0 } else { 3.0 - 4.0 * t };
        self.phase += hz / rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let pos = 0.5 + 0.5 * tri;
        let mut wet = self.dl.read(amount * pos + 1.0);
        if real {
            // the line's later taps are duller and a touch louder (the original's LC ladder)
            self.tone.a = 0.2 + 0.4 * pos;
            wet = self.tone.lp(wet) * (1.0 + 0.1 * pos);
        }
        let y = if kind >= 4 {
            // the chorus mixes the scanned signal with the dry one
            let m = depth.clamp(0.0, 1.0);
            (wet * m + x * (1.0 - m)) * (1.0 + 0.4 * (1.0 - (2.0 * m - 1.0).abs()))
        } else {
            wet
        };
        // any vibrato setting lifts the treble a little
        y + 0.18 * (y - self.air.lp(y))
    }
    fn clear(&mut self) {
        self.dl.clear();
        self.tone.z = 0.0;
        self.air.z = 0.0;
    }
}

/// What MIDI Learn waits for: the parameter, its value before, samples left.
struct Learn {
    id: u32,
    before: f32,
    left: i64,
}

pub struct TonewheelB3 {
    params: Params,
    ch: Channel,
    rate: f32,
    phase: [f32; WHEELS],
    base_hz: [f32; WHEELS],
    inc: [f32; WHEELS],
    detune: [f32; WHEELS],
    level: [f32; WHEELS],
    /// Per register (upper, lower, pedals): the wheels' levels now and where they are going.
    amp: [[f32; WHEELS]; 3],
    target: [[f32; WHEELS]; 3],
    perc_amp: [f32; WHEELS],
    perc_target: [f32; WHEELS],
    /// Keys down per register (by the register's own note), and where each incoming note went.
    keys: [[u8; 128]; 3],
    placed: [Option<(u8, u8)>; 128],
    sustained: [bool; 128],
    down: [usize; 3],
    perc_env: f32,
    perc_coef: f32,
    perc_gain: f32,
    perc_vel: f32,
    contact_coef: f32,
    release_coef: [f32; 3],
    click: f32,
    click_decay: f32,
    click_f: Biquad,
    rng: Rng,
    leak_level: f32,
    fm: [f32; 12],
    scan: [Scanner; 3],
    // tone
    tone_lo: Biquad,
    tone_hi: Biquad,
    scoop: Biquad,
    dull: OnePole,
    // effects
    eq: [Biquad; 3],
    wah: Biquad,
    wah_pos: f32,
    wah_ctl: f32,
    dist_lp: OnePole,
    dist_dc: OnePole,
    room: Room,
    room_kind: usize,
    spring: [Delay; 2],
    // rotary
    xover: OnePole,
    horn: Rotor,
    drum: Rotor,
    horn_dl: [Delay; 2],
    drum_dl: [Delay; 2],
    horn_tone: [OnePole; 2],
    far: [OnePole; 2],
    body_lo: Biquad,
    body_hi: [Biquad; 2],
    throat: [Biquad; 4],
    mic_shelf: [Biquad; 4],
    stopped: f32,
    brake_dry: f32,
    // speed switching
    before_brake: f32,
    wheel_side: i8,
    touch_on: bool,
    pedal_held: Option<i64>,
    learn: Option<Learn>,
    /// Effective drawbar gains per register.
    db: [[f32; 9]; 3],
    expression: f32,
    gain: f32,
}

impl TonewheelB3 {
    pub fn new(rate: f32) -> TonewheelB3 {
        dsp::warm_tables();
        let mut base_hz = [0.0; WHEELS];
        for (w, x) in base_hz.iter_mut().enumerate().skip(1) {
            *x = wheel_hz(w);
        }
        let mut phase = [0.0; WHEELS];
        let mut detune = [0.0; WHEELS];
        let mut rng = Rng::new(77);
        for p in phase.iter_mut() {
            *p = rng.unit();
        }
        for d in detune.iter_mut() {
            *d = rng.bipolar();
        }
        let ms10 = (rate * 0.01) as usize;
        let mut s = TonewheelB3 {
            params: Params::new(params()),
            ch: Channel::new(rate),
            rate,
            phase,
            base_hz,
            inc: [0.0; WHEELS],
            detune,
            level: [1.0; WHEELS],
            amp: [[0.0; WHEELS]; 3],
            target: [[0.0; WHEELS]; 3],
            perc_amp: [0.0; WHEELS],
            perc_target: [0.0; WHEELS],
            keys: [[0; 128]; 3],
            placed: [None; 128],
            sustained: [false; 128],
            down: [0; 3],
            perc_env: 0.0,
            perc_coef: 0.0,
            perc_gain: 1.0,
            perc_vel: 1.0,
            contact_coef: dsp::tau_coef(0.0006, rate),
            release_coef: [0.0; 3],
            click: 0.0,
            click_decay: 0.992,
            click_f: Biquad::default(),
            rng,
            leak_level: 0.0,
            fm: [1.0; 12],
            scan: [Scanner::new(rate), Scanner::new(rate), Scanner::new(rate)],
            tone_lo: Biquad::default(),
            tone_hi: Biquad::default(),
            scoop: Biquad::default(),
            dull: OnePole::default(),
            eq: [Biquad::default(); 3],
            wah: Biquad::default(),
            wah_pos: 0.5,
            wah_ctl: 0.5,
            dist_lp: OnePole::default(),
            dist_dc: OnePole::default(),
            room: Room::new(rate, 1.0),
            room_kind: usize::MAX,
            spring: [Delay::new((rate * 0.05) as usize), Delay::new((rate * 0.05) as usize)],
            xover: OnePole::default(),
            horn: Rotor { angle: 0.0, speed: 0.83 },
            drum: Rotor { angle: 0.3, speed: 0.7 },
            horn_dl: [Delay::new(ms10), Delay::new(ms10)],
            drum_dl: [Delay::new(ms10), Delay::new(ms10)],
            horn_tone: [OnePole::default(); 2],
            far: [OnePole::default(); 2],
            body_lo: Biquad::default(),
            body_hi: [Biquad::default(); 2],
            throat: [Biquad::default(); 4],
            mic_shelf: [Biquad::default(); 4],
            stopped: 0.0,
            brake_dry: 0.0,
            before_brake: 2.0,
            wheel_side: 0,
            touch_on: false,
            pedal_held: None,
            learn: None,
            db: [[0.0; 9]; 3],
            expression: 1.0,
            gain: 1.0,
        };
        s.click_f.bandpass(2500.0, 0.7, rate);
        s.xover.lowpass_hz(800.0, rate);
        s.dist_dc.lowpass_hz(25.0, rate);
        s.update();
        s
    }

    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn on(&self, id: u32) -> bool {
        self.params.get(id) >= 0.5
    }

    /// Whether the percussion may sound (its switch, and the preset key it is tied to).
    fn perc_on(&self) -> bool {
        self.on(9) && (self.on(51) || self.p(70).round() as i32 == 11)
    }

    /// The Morph: whether it is blending registrations right now.
    fn morphing(&self) -> bool {
        self.on(73) && self.p(75) > 0.0
    }

    fn update(&mut self) {
        let rate = self.rate;
        let perc_on = self.perc_on();
        let floor = self.p(129) * 0.012;
        for m in 0..2 {
            let base = if m == 0 { UPPER } else { LOWER };
            for i in 0..9 {
                let v = self.p(base + i as u32);
                // 3 dB per drawbar step; percussion takes the 1' bus as on the original
                self.db[m][i] = if m == 0 && perc_on && i == 8 {
                    0.0
                } else if v <= 0.01 {
                    floor
                } else {
                    10f32.powf(-3.0 * (8.0 - v) / 20.0)
                };
            }
        }
        for i in 0..2 {
            let v = self.p(PEDAL + i as u32);
            self.db[2][i] = if v <= 0.01 { floor } else { 10f32.powf(-3.0 * (8.0 - v) / 20.0) };
        }
        // percussion: decay and level by its two switches
        let time = if self.on(46) { self.p(11) } else { self.p(48) };
        self.perc_coef = if !self.on(46) && self.p(48) >= 5.99 { 1.0 } else { dsp::t60_coef(time, rate) };
        self.perc_gain = lg_dsp::db_to_gain(if self.on(47) { self.p(12) } else { self.p(49) }) * 0.9;
        // sustain (release) per register
        for m in 0..3 {
            let t = self.p(124 + m as u32);
            self.release_coef[m] = if t <= 0.001 { self.contact_coef } else { dsp::t60_coef(t, rate) };
        }
        // tuning: Tune, the stretches, Warmth
        let tune = self.p(54);
        let (us, ls, warm) = (self.p(120), self.p(121), self.p(122));
        let age = self.p(128);
        for w in 1..WHEELS {
            let x = (w as f32 - 46.0) / 45.0;
            let mut cents = tune + warm * 7.0 * self.detune[w];
            if x > 0.0 {
                cents += us * 30.0 * x * x;
            } else {
                cents -= ls * 30.0 * x * x;
            }
            self.inc[w] = self.base_hz[w] * 2f32.powf(cents / 1200.0) / rate;
            // the generator's taper; Filter Age moves the filters' centres (older: duller top, thinner bass)
            let taper = if w > 60 { 1.0 - (w - 60) as f32 * 0.008 } else { 1.0 };
            let aged = -age * 5.0 * x * x.abs() - age.abs() * 1.5 * x * x;
            self.level[w] = taper * 10f32.powf(aged / 20.0);
        }
        let tb = self.p(26);
        self.tone_lo.low_shelf(250.0 * (1.0 + 0.3 * age), -tb * 6.0, rate);
        self.tone_hi.high_shelf(3000.0 * (1.0 - 0.3 * age), tb * 6.0, rate);
        self.dull.lowpass_hz(dsp::lerp(16000.0, 1800.0, (-self.p(135)).max(0.0)), rate);
        self.ch.bend_up = self.p(28);
        self.ch.bend_down = self.p(123).min(12.0);
        self.ch.update_bend();
        self.gain = lg_dsp::db_to_gain(self.p(25)) * 0.035;
        // effects
        self.eq[0].low_shelf(200.0, self.p(102), rate);
        self.eq[1].peaking(1000.0, 0.8, self.p(103), rate);
        self.eq[2].high_shelf(4000.0, self.p(104), rate);
        self.dist_lp.lowpass_hz(dsp::lerp(900.0, 9000.0, self.p(113)), rate);
        let kind = self.p(115).round() as usize;
        if kind != self.room_kind {
            self.room_kind = kind;
            let (size, fb, damp) = [(0.45, 0.5, 0.15), (0.6, 0.62, 0.45), (0.8, 0.68, 0.35), (1.0, 0.76, 0.3), (1.3, 0.82, 0.28), (1.6, 0.88, 0.25), (0.7, 0.8, 0.1)][kind.min(6)];
            self.room = Room::new(rate, size);
            self.room.feedback = fb;
            self.room.damp = damp;
        }
        // cabinet
        let cab = self.p(80).round() as usize;
        let wood = matches!(cab, 0 | 1 | 5);
        let open = matches!(cab, 2 | 6);
        self.body_lo.peaking(if wood { 190.0 } else { 240.0 }, 1.2, if wood { 4.0 } else if open { 1.0 } else { 2.5 }, rate);
        for c in 0..2 {
            self.body_hi[c].high_shelf(5000.0, if wood { -3.0 } else if open { 3.0 } else { 0.0 }, rate);
            self.throat[c * 2].peaking(1250.0, 2.5, 4.0, rate);
            self.throat[c * 2 + 1].peaking(2900.0, 3.0, 3.0, rate);
            self.far[c].lowpass_hz(dsp::lerp(16000.0, 3500.0, if cab == 0 { 0.15 } else { self.p(92) }), rate);
            // microphones (Real Cabinet): the dynamic one is the brighter
            for (k, id) in [88u32, 89].into_iter().enumerate() {
                let t = if cab == 0 { self.p(id).round() as i32 } else { 1 };
                self.mic_shelf[k * 2 + c].high_shelf(4500.0, if t == 0 { 4.0 } else { 0.0 }, rate);
            }
        }
        self.recompute_targets();
    }

    /// Sounding MIDI note → wheel (MIDI 24 = wheel 1), folding at both ends.
    fn wheel_for(&self, sem: i32) -> usize {
        let top = self.p(132).round() as i32;
        let mut w = sem - 23;
        while w < 1 {
            w += 12;
        }
        while w > top {
            w -= 12;
        }
        w.max(1) as usize
    }

    fn recompute_targets(&mut self) {
        self.target = [[0.0; WHEELS]; 3];
        self.perc_target = [0.0; WHEELS];
        let perc_h = if self.on(10) { 19 } else { 12 };
        let ultra = self.on(138);
        let ct = self.p(130) * 0.05;
        let bass = self.p(136);
        for m in 0..2 {
            for k in 0..128usize {
                if self.keys[m][k] == 0 {
                    continue;
                }
                let n = k as i32;
                for (i, f) in FOOT.iter().enumerate() {
                    let g = self.db[m][i];
                    if g > 0.0 {
                        let mut w = self.wheel_for(n + f);
                        // the 16' drawbar repeats in the lowest octave unless Ultrabass is on
                        if i == 0 && !ultra && (36..48).contains(&n) {
                            w = self.wheel_for(n + f + 12);
                        }
                        self.target[m][w] += g * self.level[w];
                    }
                }
                if m == 0 {
                    self.perc_target[self.wheel_for(n + perc_h)] = 1.0;
                }
            }
        }
        // pedals: a mixed waveform (the fundamental and softer upper partials; Bass Filter removes them)
        let mix: [(i32, f32); 4] = [(0, 1.0), (12, 0.45), (19, 0.3), (24, 0.2)];
        for k in 0..128usize {
            if self.keys[2][k] == 0 {
                continue;
            }
            let n = k as i32;
            for (bar, foot) in [(0usize, -12i32), (1, 0)] {
                let g = self.db[2][bar];
                if g <= 0.0 {
                    continue;
                }
                for (j, (st, a)) in mix.iter().enumerate() {
                    let a = if j == 0 { *a } else { a * (1.0 - bass) };
                    if a > 0.0 {
                        let w = self.wheel_for(n + foot + st);
                        self.target[2][w] += g * a * self.level[w];
                    }
                }
            }
        }
        // crosstalk between wheel pairs four octaves apart
        if ct > 0.0 {
            for m in 0..3 {
                let src = self.target[m];
                for w in 1..WHEELS {
                    if src[w] > 0.0 {
                        let o = if w > 48 { w - 48 } else { w + 48 };
                        if o < WHEELS {
                            self.target[m][o] += src[w] * ct;
                        }
                    }
                }
            }
        }
    }

    /// Which register an incoming note plays (0 upper, 1 lower, 2 pedals), and its note there.
    fn route(&self, chan: u8, note: u8) -> (usize, u8) {
        let (uc, lc, pc) = ((self.p(67) as u8).saturating_sub(1), (self.p(68) as u8).saturating_sub(1), (self.p(69) as u8).saturating_sub(1));
        let mode = self.p(61).round() as i32;
        let (s1, s2) = (self.p(62).round() as u8, self.p(63).round() as u8);
        let m = if chan == lc && lc != uc {
            1
        } else if chan == pc && pc != uc {
            2
        } else if mode == 2 && note < s1 {
            2
        } else if mode >= 1 && note < s2 {
            1
        } else {
            0
        };
        let mut n = note as i32 + 12 * self.p(64 + m as u32).round() as i32;
        let (lo, hi) = if m == 2 { (36, 60) } else { (if self.on(137) { 24 } else { 36 }, 96) };
        while n < lo {
            n += 12;
        }
        while n > hi {
            n -= 12;
        }
        (m, n as u8)
    }

    fn add_click(&mut self, level: f32) {
        if level <= 0.0 {
            return;
        }
        // each click: a random length between Minimum and Maximum, random colour and volume
        let (a, b) = (self.p(13), self.p(14));
        let len = dsp::lerp(a.min(b), a.max(b), self.rng.unit());
        self.click_decay = dsp::t60_coef(0.002 + 0.03 * len, self.rate);
        let hz = dsp::lerp(900.0, 6000.0, self.p(60)) * (1.0 + 0.25 * self.rng.bipolar());
        self.click_f.bandpass(hz, 0.7, self.rate);
        self.click += level * (0.75 + 0.25 * self.rng.unit()) * 0.6;
    }

    fn key_down(&mut self, m: usize, n: u8, vel: u8) {
        let k = n as usize;
        self.keys[m][k] = self.keys[m][k].saturating_add(1);
        if self.keys[m][k] > 1 {
            return;
        }
        let v = vel as f32 / 127.0;
        if m == 0 && self.perc_on() && (self.down[0] == 0 || self.on(50)) {
            // Poly: only when playing detached; Mono: on every key
            self.perc_env = 1.0;
            self.perc_vel = dsp::lerp(1.0, v, self.p(53));
        }
        self.down[m] += 1;
        // Smart sustain: what is still ringing from released keys stops when new notes come
        if !self.on(127) && self.p(124 + m as u32) > 0.001 {
            for w in 1..WHEELS {
                if self.amp[m][w] > self.target[m][w] {
                    self.amp[m][w] = self.target[m][w] + (self.amp[m][w] - self.target[m][w]) * 0.15;
                }
            }
        }
        let lvl = if m == 2 { self.p(58) } else { self.p(56) };
        self.add_click(lvl * dsp::lerp(1.0, v, self.p(59)));
        self.recompute_targets();
    }

    fn key_up(&mut self, m: usize, n: u8) {
        let k = n as usize;
        if self.keys[m][k] == 0 {
            return;
        }
        self.keys[m][k] -= 1;
        if self.keys[m][k] > 0 {
            return;
        }
        self.down[m] = self.down[m].saturating_sub(1);
        let lvl = if m == 2 { self.p(58) * 0.5 } else { self.p(57) };
        self.add_click(lvl);
        self.recompute_targets();
    }

    fn release_note(&mut self, pitch: u8) {
        if let Some((m, n)) = self.placed[pitch as usize].take() {
            self.key_up(m as usize, n);
        }
    }

    fn all_off(&mut self) {
        self.keys = [[0; 128]; 3];
        self.placed = [None; 128];
        self.sustained = [false; 128];
        self.down = [0; 3];
        self.recompute_targets();
    }

    /// A drawbar moved: the active preset key remembers it.
    fn store_drawbar(&mut self, manual: usize, bar: usize, v: f32) {
        if manual == 0 && self.morphing() {
            return;
        }
        let key = if self.on(77) { if manual == 0 { 10 } else { 11 } } else { self.p(70 + manual as u32).round() as usize };
        let base = if manual == 0 { PRESETS_U } else { PRESETS_L };
        self.params.set(base + (key * 9 + bar) as u32, v);
    }

    /// A preset key's registration goes to the drawbars.
    fn recall(&mut self, manual: usize) {
        let key = self.p(70 + manual as u32).round() as u32;
        let (base, bars) = if manual == 0 { (PRESETS_U, UPPER) } else { (PRESETS_L, LOWER) };
        for d in 0..9u32 {
            let v = self.p(base + key * 9 + d);
            self.params.set(bars + d, v);
        }
    }

    /// Chooses a preset key (a click on it, or MIDI notes 24..35). Key C initializes the active
    /// key's registration instead (`before`: the key that was active).
    fn press_preset(&mut self, manual: usize, key: usize, before: usize) {
        let id = 70 + manual as u32;
        if key == 0 {
            let k = before.clamp(1, 11);
            self.params.set(id, k as f32);
            let base = if manual == 0 { PRESETS_U } else { PRESETS_L };
            for d in 0..9 {
                self.params.set(base + (k * 9 + d) as u32, factory(manual, k, d));
            }
        } else {
            self.params.set(id, key as f32);
        }
        self.recall(manual);
    }

    /// The Upper Morph position: steps through, or blends, the presets from key B downward.
    fn apply_morph(&mut self) {
        let n = self.p(74).round() as usize + 2;
        let x = self.p(75) * (n - 1) as f32;
        if !self.on(73) {
            let key = 11 - (x.round() as usize).min(n - 1);
            if key != self.p(70).round() as usize {
                self.params.set(70, key as f32);
                self.recall(0);
            }
            return;
        }
        if x <= 0.0 {
            return self.recall(0);
        }
        let i = (x.floor() as usize).min(n - 1);
        let j = (i + 1).min(n - 1);
        let f = x - i as f32;
        for d in 0..9u32 {
            let a = self.p(PRESETS_U + (11 - i) as u32 * 9 + d);
            let b = self.p(PRESETS_U + (11 - j) as u32 * 9 + d);
            self.params.set(UPPER + d, dsp::lerp(a, b, f));
        }
    }

    /// Saves the morphed registration to a preset key (Logic's Save Morph To).
    pub fn save_morph_to(&mut self, key: usize) {
        for d in 0..9u32 {
            let v = self.p(UPPER + d).round();
            self.params.set(PRESETS_U + key.min(11) as u32 * 9 + d, v);
        }
    }

    /// Which preset key is active on a manual (0 upper, 1 lower).
    pub fn preset_key(&self, manual: usize) -> usize {
        self.p(70 + manual as u32).round() as usize
    }

    // --- rotor speed -------------------------------------------------------------------------

    /// 1 brake, 2 slow, 3 fast (the Rotation switch's values).
    fn set_speed(&mut self, v: f32) {
        let now = self.p(18);
        if now < 0.5 {
            return; // cabinet off
        }
        if v == 1.0 && now != 1.0 {
            self.before_brake = now;
        }
        self.params.set(18, v);
    }
    fn toggle_speed(&mut self) {
        let now = self.p(18).round();
        let next = if now == 1.0 { self.before_brake } else if now == 3.0 { 2.0 } else { 3.0 };
        self.set_speed(next);
    }
    fn toggle_brake(&mut self) {
        if self.p(18).round() == 1.0 {
            let b = self.before_brake;
            self.set_speed(b);
        } else {
            self.set_speed(1.0);
        }
    }
    /// A speed pedal went down or up: Toggle modes switch on the press, Temp ones on both; held
    /// about a second and let go, it brakes (and again brings the speed back).
    fn speed_pedal(&mut self, down: bool, temp: bool) {
        if down {
            self.pedal_held = Some(0);
            self.toggle_speed();
        } else {
            let long = self.pedal_held.take().is_some_and(|n| n as f32 >= self.rate * 0.9);
            if long {
                self.toggle_speed(); // (undo the press's switch)
                self.toggle_brake();
            } else if temp {
                self.toggle_speed();
            }
        }
    }

    fn rotor_targets(&self) -> (f32, f32) {
        let mode = self.p(18).round() as i32;
        let max = self.p(82);
        let (hs, hf, ds, df) = (self.p(19), self.p(20) * max, self.p(21), self.p(22) * max);
        if mode <= 1 {
            return (0.0, 0.0);
        }
        let fast = mode == 3;
        let single = self.p(80).round() as i32 == 3;
        match (if single { 0 } else { self.p(83).round() as i32 }, fast) {
            (1, true) => (hs, df),
            (1, false) => (hf, ds),
            (2, false) => (hs, 0.0),
            (_, true) => (hf, df),
            (_, false) => (hs, ds),
        }
    }

    // --- MIDI ---------------------------------------------------------------------------------

    fn set01(&mut self, id: u32, v: u8) {
        let Some(p) = params().iter().find(|p| p.id == id) else { return };
        let x = p.min + (p.max - p.min) * v as f32 / 127.0;
        self.set(id, if p.unit == Unit::Menu || p.unit == Unit::OnOff { x.round() } else { x });
    }
    /// A drawbar from a fader: reversed, like the drawbars (down is louder).
    fn bar(&mut self, id: u32, v: u8) {
        self.set(id, (8.0 - v as f32 / 127.0 * 8.0).round());
    }
    fn flag(&mut self, id: u32, on: bool) {
        self.set(id, on as u8 as f32);
    }
    fn vib_type(&mut self, v: u8) {
        self.set(15, 1.0 + (v as f32 / 127.0 * 5.0).round());
    }
    fn brake_to(&mut self, on: bool) {
        if on != (self.p(18).round() == 1.0) {
            self.toggle_brake();
        }
    }

    /// The Hardware Controller modes' controller maps. True when the controller was used.
    fn hardware(&mut self, cc: u8, v: u8) -> bool {
        let on = v >= 64;
        match self.p(139).round() as i32 {
            1 => match cc {
                70..=78 => self.bar(UPPER + (cc - 70) as u32, v),
                80 | 92 => self.set_speed(if v < 43 { 2.0 } else if v < 85 { 1.0 } else { 3.0 }),
                81 => self.set_speed(if on { 1.0 } else { 2.0 }),
                82 => self.set01(116, v),
                85 => self.flag(41, on),
                86 => self.flag(42, on),
                87 => self.vib_type(v),
                94 => self.flag(9, on),
                95 => self.flag(10, on),
                102 => self.flag(47, on),
                103 => self.flag(46, on),
                104 => self.set01(102, v),
                105 => self.set01(103, v),
                106 => self.set01(104, v),
                107 => self.set01(105, v),
                108 => self.set01(107, v),
                109 => self.set01(109, v),
                110 => self.set01(112, v),
                111 => self.set01(17, v),
                112 => self.set01(113, v),
                113 => self.set01(56, v),
                114 => self.set01(57, v),
                115 => self.set01(25, v),
                116 => self.set01(133, v),
                117 => self.set01(134, v),
                118 => self.set01(82, v),
                _ => return false,
            },
            // (Hammond XB: a manual's whole registration in one controller isn't decoded; the
            // switches are)
            2 => match cc {
                87 => self.vib_type(v),
                84 => self.set(18, if on { 2.0 } else { 0.0 }),
                85 => self.set_speed(if on { 3.0 } else { 2.0 }),
                86 => self.brake_to(on),
                76 => self.set01(17, v),
                91 => self.set01(116, v),
                72 => self.flag(46, on),
                71 => self.flag(47, !on),
                73 => {
                    self.flag(9, on);
                    if on {
                        self.set(10, 0.0);
                    }
                }
                74 => {
                    // the 3rd harmonic wins when both are on
                    if on {
                        self.flag(9, true);
                        self.set(10, 1.0);
                    }
                }
                _ => return false,
            },
            3 => match cc {
                12..=20 => self.bar(UPPER + (cc - 12) as u32, v),
                21..=29 => self.bar(LOWER + (cc - 21) as u32, v),
                31 => self.flag(41, on),
                30 => self.flag(42, on),
                90 => self.set01(102, v),
                70 => self.set01(103, v),
                5 => self.set01(104, v),
                76 => self.set01(17, v),
                78 => self.set01(113, v),
                75 => self.set01(56, v),
                3 => self.set01(92, v),
                10 => self.set01(93, v),
                83 => self.set01(23, v),
                82 => self.set01(82, v),
                1 => self.set_speed(if on { 3.0 } else { 2.0 }),
                68 => self.brake_to(v == 0),
                _ => return false,
            },
            4 => match cc {
                16..=24 => self.bar(UPPER + (cc - 16) as u32, v),
                70..=78 => self.bar(LOWER + (cc - 70) as u32, v),
                85 => self.flag(41, on),
                86 => self.flag(42, on),
                84 => self.vib_type(v),
                87 => self.flag(9, on),
                88 => {
                    // four steps: volume and time together
                    let k = (v as usize * 4 / 128).min(3);
                    self.flag(47, k & 1 == 0);
                    self.flag(46, k < 2);
                }
                95 => self.flag(10, on),
                113 => self.set01(104, v),
                114 => self.set01(102, v),
                111 => self.set01(17, v),
                81 => self.set(18, if on { 2.0 } else { 0.0 }),
                82 => self.set_speed(if on { 3.0 } else { 2.0 }),
                83 => self.brake_to(on),
                _ => return false,
            },
            5 => match cc {
                12..=20 => self.bar(UPPER + (cc - 12) as u32, v),
                21..=29 => self.bar(LOWER + (cc - 21) as u32, v),
                33 => self.bar(PEDAL, v),
                35 => self.bar(PEDAL + 1, v),
                56 => self.set01(126, v),
                31 => self.flag(41, on),
                30 => self.flag(42, on),
                73 => self.vib_type(v),
                66 => self.flag(9, on),
                70 => self.flag(47, !on),
                71 => self.flag(46, on),
                89 => self.set01(52, v),
                72 => self.flag(10, on),
                10 => self.set01(104, v),
                9 => self.set01(103, v),
                8 => self.set01(102, v),
                91 => self.set01(116, v),
                76 => self.set01(17, v),
                75 => self.set01(56, v),
                86 => self.set01(16, v),
                87 => self.set01(130, v),
                88 => self.set01(135, v),
                85 => self.set(18, if on { 2.0 } else { 0.0 }),
                1 => self.set_speed(if on { 3.0 } else { 2.0 }),
                68 => self.brake_to(on),
                90 => self.set01(94, v),
                93 => self.set01(92, v),
                _ => return false,
            },
            _ => return false,
        }
        true
    }

    /// A controller value for the Morph or the wah (their MIDI controller choices).
    fn assigned(&mut self, source: f32, v: f32) {
        if source < 1.0 {
            return;
        }
        if self.p(76).round() == source {
            self.params.set(75, v);
            self.apply_morph();
            self.update();
        }
        if self.p(110).round() == source {
            self.wah_ctl = v;
        }
    }

    fn reverb(&mut self, l: &mut [f32], r: &mut [f32], level: f32, spring: bool) {
        if spring {
            // a spring: short echoes chasing each other
            let (da, db) = ((self.rate * 0.031) as usize, (self.rate * 0.043) as usize);
            for i in 0..l.len() {
                let x = 0.5 * (l[i] + r[i]);
                let a = self.spring[0].tap(da);
                let b = self.spring[1].tap(db);
                self.spring[0].push(dsp::flush(x + b * 0.62));
                self.spring[1].push(dsp::flush(x - a * 0.58));
                l[i] += a * level * 0.8;
                r[i] += b * level * 0.8;
            }
        }
        self.room.process(l, r, level * if spring { 1.2 } else { 2.5 });
    }
}

impl Plugin for TonewheelB3 {
    fn params(&self) -> &[Param] {
        params()
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, value: f32) {
        let before = self.params.get(id);
        if !self.params.set(id, value) {
            return;
        }
        let v = self.params.get(id);
        match id {
            0..=8 => self.store_drawbar(0, id as usize, v),
            30..=38 => self.store_drawbar(1, (id - 30) as usize, v),
            70 | 71 if v != before => self.press_preset((id - 70) as usize, v.round() as usize, before.round() as usize),
            75 if v != before => self.apply_morph(),
            73 | 74 if self.morphing() => self.apply_morph(),
            76 | 110 if v == CTL_LEARN => self.learn = Some(Learn { id, before: if before == CTL_LEARN { 0.0 } else { before }, left: (self.rate * 20.0) as i64 }),
            18 if v == 1.0 && before > 1.0 => self.before_brake = before,
            PRESETS_U..=415 => {
                // the active key's stored registration and the drawbars stay the same thing
                let (m, k) = if id < PRESETS_L { (0u32, id - PRESETS_U) } else { (1, id - PRESETS_L) };
                if (k / 9) as f32 == self.p(70 + m).round() && !(m == 0 && self.morphing()) {
                    self.params.set(if m == 0 { UPPER } else { LOWER } + k % 9, v);
                }
            }
            _ => {}
        }
        self.update();
    }
    fn reset(&mut self) {
        self.keys = [[0; 128]; 3];
        self.placed = [None; 128];
        self.sustained = [false; 128];
        self.down = [0; 3];
        self.amp = [[0.0; WHEELS]; 3];
        self.perc_amp = [0.0; WHEELS];
        self.perc_env = 0.0;
        self.click = 0.0;
        for s in &mut self.scan {
            s.clear();
        }
        for d in self.horn_dl.iter_mut().chain(self.drum_dl.iter_mut()).chain(self.spring.iter_mut()) {
            d.clear();
        }
        self.room.reset();
        self.ch.reset();
        self.expression = 1.0;
        self.update();
    }
    fn tail(&self) -> f32 {
        0.3 + self.p(124).max(self.p(125)).max(self.p(126)) + if self.on(114) { 2.0 } else { 0.0 }
    }
    fn value_text(&self, id: u32, value: f32) -> Option<String> {
        match id {
            76 | 110 => Some(match value.round() as i32 {
                0 => "Off".into(),
                120 => "Aftertouch".into(),
                121 => "Learn".into(),
                n => format!("CC {n}"),
            }),
            62 | 63 => Some(crate::sampler_doc::note_name(value.round() as u8)),
            123 if value > 12.5 => Some("Brake".into()),
            _ => None,
        }
    }
}

impl Instrument for TonewheelB3 {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let c = (self.p(67) as u8).saturating_sub(1);
        self.note_on_ch(c, pitch, velocity);
    }
    fn note_off(&mut self, pitch: u8) {
        let pitch = pitch.min(127);
        let pedal_sustains = !matches!(self.p(27).round() as i32, 6 | 7);
        if self.ch.ctl.sustain && pedal_sustains {
            if self.placed[pitch as usize].is_some() {
                self.sustained[pitch as usize] = true;
            }
            return;
        }
        self.release_note(pitch);
    }
    fn note_on_ch(&mut self, chan: u8, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        let pitch = pitch.min(127);
        // the preset keys: the octave below the lowest playable note
        if (24..36).contains(&pitch) && self.on(72) {
            let lc = (self.p(68) as u8).saturating_sub(1);
            let uc = (self.p(67) as u8).saturating_sub(1);
            let manual = (chan == lc && lc != uc) as usize;
            let before = self.p(70 + manual as u32).round() as usize;
            self.press_preset(manual, (pitch - 24) as usize, before);
            self.update();
            return;
        }
        self.sustained[pitch as usize] = false;
        if self.placed[pitch as usize].is_some() {
            self.release_note(pitch);
        }
        let (m, n) = self.route(chan, pitch);
        self.placed[pitch as usize] = Some((m as u8, n));
        self.key_down(m, n, velocity);
    }
    fn note_off_ch(&mut self, _chan: u8, pitch: u8) {
        self.note_off(pitch);
    }
    /// Channel aftertouch (the Touch speed modes; the Morph and the wah when assigned to it).
    fn pressure(&mut self, value: u8) {
        self.ch.ctl.aftertouch = value as f32 / 127.0;
        let mode = self.p(27).round() as i32;
        let on = value >= 64;
        if (mode == 4 || mode == 5) && on != self.touch_on && (on || mode == 5) {
            self.toggle_speed();
        }
        self.touch_on = on;
        self.assigned(CTL_TOUCH, value as f32 / 127.0);
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            self.all_off();
            return;
        }
        let v = value as f32 / 127.0;
        // MIDI Learn takes the first suitable controller
        if let Some(l) = self.learn.take() {
            if (1..120).contains(&cc) && cc != 7 && cc != 10 && cc != 64 {
                self.params.set(l.id, cc as f32);
                self.assigned(cc as f32, v);
                return;
            }
            self.learn = Some(l);
        }
        if self.hardware(cc, value) {
            return;
        }
        self.assigned(cc as f32, v);
        let mode = self.p(27).round() as i32;
        match cc {
            1 => {
                let side: i8 = if value > 64 { 1 } else { -1 };
                match mode {
                    1 => self.set_speed(if value < 43 { 2.0 } else if value < 85 { 1.0 } else { 3.0 }),
                    2 if side > 0 && self.wheel_side <= 0 => self.toggle_speed(),
                    3 if side != self.wheel_side && self.wheel_side != 0 => self.toggle_speed(),
                    _ => {}
                }
                self.wheel_side = side;
                self.ch.ctl.mod_wheel = v;
            }
            11 => self.expression = v,
            18 | 19 if mode == 8 || mode == 9 => {
                let down = value >= 64;
                if down != self.pedal_held.is_some() {
                    self.speed_pedal(down, mode == 9);
                }
            }
            64 if mode == 6 || mode == 7 => {
                let down = value >= 64;
                if down != self.pedal_held.is_some() {
                    self.speed_pedal(down, mode == 7);
                }
            }
            _ => {
                if let Some(false) = self.ch.cc(cc, value) {
                    for k in 0..128u8 {
                        if self.sustained[k as usize] {
                            self.sustained[k as usize] = false;
                            self.release_note(k);
                        }
                    }
                }
            }
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let rate = self.rate;
        let (gl, gr) = self.ch.out_gains();
        // timers: MIDI Learn gives up after 20 seconds; how long a speed pedal has been down
        if let Some(l) = self.learn.as_mut() {
            l.left -= n as i64;
            if l.left <= 0 {
                let (id, before) = (l.id, l.before);
                self.learn = None;
                self.params.set(id, before);
            }
        }
        if let Some(h) = self.pedal_held.as_mut() {
            *h += n as i64;
        }
        let vib = self.p(15).round() as usize;
        let vkind = [if self.on(41) { vib } else { 0 }, if self.on(42) { vib } else { 0 }];
        let real = self.on(43);
        let (vhz, vdepth) = (self.p(44), self.p(45));
        let fx = self.on(100);
        let drive = if fx && self.on(111) { self.p(17) } else { 0.0 };
        let dist_kind = self.p(112).round() as usize;
        let eq_on = fx && self.on(101);
        let eq_gain = lg_dsp::db_to_gain(self.p(105));
        let wah_on = fx && self.on(106);
        let wah_kind = self.p(107).round() as usize;
        let order: [u8; 3] = [[0, 1, 2], [0, 2, 1], [1, 2, 0], [2, 0, 1]][(self.p(118).round() as usize).min(3)];
        let pedal_fx = self.on(119);
        let rev = if fx && self.on(114) { self.p(116) } else { 0.0 };
        let rev_post = self.on(117);
        let spring = self.room_kind == 6;
        let mode = self.p(18).round() as i32;
        let rotary_on = mode != 0;
        let rotor_mix = self.p(29);
        let accel = self.p(23);
        let (ht, dt) = self.rotor_targets();
        let cab = self.p(80).round() as usize;
        let single = cab == 3;
        let split = matches!(cab, 4 | 7);
        let horn_ir = cab >= 5;
        let real_cab = cab == 0;
        let rear = self.on(90);
        let sync = self.p(83).round() as i32 == 3;
        // microphones: two per rotor, their angle apart; farther away, less movement is heard
        let far = if real_cab { 0.15 } else { self.p(92) };
        let (hwid, dwid) = if real_cab { (self.p(24), self.p(91)) } else { (self.p(93) / 180.0, self.p(93) / 180.0) };
        let bal = if real_cab { 0.0 } else { self.p(94) };
        let (hbal, dbal) = (1.0 + bal.min(0.0), 1.0 - bal.max(0.0));
        let depth = 1.0 / (1.0 + 1.5 * far);
        // without its deflectors the horn beams: more amplitude, less pitch movement
        let (ham, hfm) = if self.on(81) { (0.35 * depth, 0.0007 * depth) } else { (0.6 * depth, 0.00035 * depth) };
        let ms = real_cab && (self.p(88).round() as i32 == 2 || self.p(89).round() as i32 == 2);
        let brake_dry_on = mode == 1 && !self.on(85);
        let dry_level = self.p(84);
        let stop_at = (self.p(86) / 360.0, self.p(87) / 360.0);
        let leak = self.p(16);
        let upper_gain = if self.perc_on() { lg_dsp::db_to_gain(self.p(52)) } else { 1.0 };
        let (lower_gain, ped_gain) = (lg_dsp::db_to_gain(self.p(133)), lg_dsp::db_to_gain(self.p(134)));
        let shape = self.p(135);
        let sh = shape.max(0.0) * 0.35;
        let rfm = self.p(131);
        let expr = dsp::lerp(1.0, self.expression * self.expression, self.p(55));
        // the swell pedal's preamp: mids go down more than bass and treble
        self.scoop.peaking(900.0, 0.6, -9.0 * (1.0 - expr), rate);
        // Pitchbend Down all the way: the wheels slow to a stop (and a stopped wheel is silent)
        let bend = if self.p(123) > 12.5 && self.ch.ctl.bend < 0.0 { (1.0 + self.ch.ctl.bend / 12.0).max(0.0) } else { self.ch.ctl.bend_ratio };
        let spin = (bend * 4.0).min(1.0);
        let any_key = self.down.iter().any(|d| *d > 0);
        let (range, bite) = (self.p(108), self.p(109));
        let mut pos = 0;
        let mut man = [0.0f32; CHUNK];
        let mut ped = [0.0f32; CHUNK];
        let mut ol = [0.0f32; CHUNK];
        let mut or = [0.0f32; CHUNK];
        while pos < n {
            let m = (n - pos).min(CHUNK);
            // rotor speeds follow their targets (the horn is lighter and gets there faster)
            let blocks = rate / m as f32;
            let (hc, dc) = if accel < 0.02 {
                (0.0, 0.0)
            } else if sync {
                (dsp::tau_coef(1.2 * accel, blocks), dsp::tau_coef(1.2 * accel, blocks))
            } else {
                (dsp::tau_coef(0.6 * accel, blocks), dsp::tau_coef(2.5 * accel, blocks))
            };
            self.horn.speed = ht + (self.horn.speed - ht) * hc;
            self.drum.speed = dt + (self.drum.speed - dt) * dc;
            if mode == 1 && self.horn.speed.abs() < 0.08 && self.drum.speed.abs() < 0.08 {
                // stopped: the rotors come to rest at their stop positions
                self.stopped += m as f32 / rate;
                self.horn.angle += (stop_at.0 - self.horn.angle) * 0.02;
                self.drum.angle += (stop_at.1 - self.drum.angle) * 0.02;
            } else {
                self.stopped = 0.0;
            }
            let bd_target = if brake_dry_on && self.stopped > 1.0 { 1.0 } else { 0.0 };
            self.brake_dry += (bd_target - self.brake_dry) * 0.05;
            let lt = if any_key { leak * 0.0025 } else { 0.0 };
            self.leak_level += (lt - self.leak_level) * 0.05;
            if rfm > 0.0 {
                for f in self.fm.iter_mut() {
                    *f += (1.0 + self.rng.bipolar() * 0.004 * rfm - *f) * 0.2;
                }
            } else {
                self.fm = [1.0; 12];
            }
            // wah: the pedal position moves the filter
            if wah_on {
                let slow = matches!(wah_kind, 3 | 4);
                self.wah_pos += (self.wah_ctl - self.wah_pos) * if slow { 0.03 } else { 0.25 };
                let x = (0.5 + (self.wah_pos - 0.5) * range).clamp(0.0, 1.0);
                let (lo, hi, q): (f32, f32, f32) = [(450.0, 2200.0, 4.0), (350.0, 1800.0, 3.0), (500.0, 3000.0, 6.0), (400.0, 2000.0, 2.5), (450.0, 2600.0, 5.0), (300.0, 4000.0, 2.0), (200.0, 3000.0, 2.0), (400.0, 2500.0, 2.0)][wah_kind.min(7)];
                let hz = lo * (hi / lo).powf(x);
                let q = q * (0.5 + 1.5 * bite);
                match wah_kind {
                    5 => self.wah.lowpass(hz, q, rate),
                    6 => self.wah.highpass(hz, q, rate),
                    7 => self.wah.peaking(hz, q, 6.0 + 12.0 * bite, rate),
                    _ => self.wah.bandpass(hz, q, rate),
                }
            }
            let all = self.leak_level > 1e-6;
            let cc = self.contact_coef;
            let rc = self.release_coef;
            for i in 0..m {
                let mut bus = [0.0f32; 3];
                let mut px = 0.0;
                let mut lk = 0.0;
                for w in 1..WHEELS {
                    let mut live = all;
                    let mut a = [0.0f32; 3];
                    for (k, a) in a.iter_mut().enumerate() {
                        let (t, cur) = (self.target[k][w], self.amp[k][w]);
                        if t != 0.0 || cur != 0.0 {
                            let c = if t < cur { rc[k] } else { cc };
                            *a = dsp::flush(t + (cur - t) * c);
                            self.amp[k][w] = *a;
                            live = true;
                        }
                    }
                    let (pt, pc) = (self.perc_target[w], self.perc_amp[w]);
                    let pa = if pt != 0.0 || pc != 0.0 { dsp::flush(pt + (pc - pt) * cc) } else { 0.0 };
                    self.perc_amp[w] = pa;
                    let ph = self.phase[w] + self.inc[w] * bend * self.fm[(w - 1) % 12];
                    self.phase[w] = ph - ph.floor();
                    if live || pa > 0.0 {
                        let mut s = dsp::sin_cycles(self.phase[w]);
                        if sh > 0.0 {
                            // a brighter wheel: some third harmonic
                            s *= 1.0 + sh * (3.0 - 4.0 * s * s);
                        }
                        bus[0] += s * a[0];
                        bus[1] += s * a[1];
                        bus[2] += s * a[2];
                        px += s * pa;
                        lk += s;
                    }
                }
                let mut xu = (bus[0] * upper_gain + px * self.perc_env * self.perc_gain * self.perc_vel + lk * self.leak_level) * spin;
                self.perc_env *= self.perc_coef;
                if self.click > 1e-5 {
                    xu += self.click_f.run(self.rng.bipolar()) * self.click;
                    self.click *= self.click_decay;
                }
                // the scanners: the pedals follow the lower manual's setting
                let u = self.scan[0].run(xu, vkind[0], real, vhz, vdepth, rate);
                let l = self.scan[1].run(bus[1] * lower_gain * spin, vkind[1], real, vhz, vdepth, rate);
                let pd = self.scan[2].run(bus[2] * ped_gain * spin, vkind[1], real, vhz, vdepth, rate);
                let mut x = u + l;
                if shape < 0.0 {
                    x = self.dull.lp(x) * (1.0 + 0.5 * shape);
                }
                man[i] = x;
                ped[i] = pd;
            }
            // tone, level, expression; then the effects
            for i in 0..m {
                let mut pd = ped[i] * self.gain * expr;
                let mut x = self.scoop.run(self.tone_hi.run(self.tone_lo.run(man[i]))) * self.gain * expr;
                if pedal_fx {
                    x += pd;
                    pd = 0.0;
                }
                for stage in order {
                    match stage {
                        0 if eq_on => {
                            for f in self.eq.iter_mut() {
                                x = f.run(x);
                            }
                            x *= eq_gain;
                        }
                        1 if wah_on => x = self.wah.run(x) * if wah_kind < 5 { 2.0 } else { 1.0 },
                        2 if drive > 0.0 => {
                            // a two-stage tube amplifier; Tone only changes the distorted part
                            let (pre, bias, hard): (f32, f32, f32) = [(6.0, 0.15, 0.0), (10.0, 0.05, 0.5), (16.0, 0.3, 1.0), (5.0, 0.1, 0.0), (7.0, 0.2, 0.2), (9.0, 0.0, 0.4)][dist_kind.min(5)];
                            let g = 1.0 + drive * pre;
                            let a = dsp::soft_clip(x * g + bias * drive);
                            let b = if hard > 0.0 { dsp::lerp(a, (a * 2.5).clamp(-0.8, 0.8), hard) } else { a };
                            let y = dsp::fast_tanh(b * (1.0 + drive));
                            let y = self.dist_lp.lp(y - self.dist_dc.lp(y)) / g.sqrt() * (1.0 + drive);
                            x = dsp::lerp(x, y, (drive * 4.0).min(1.0));
                        }
                        _ => {}
                    }
                }
                man[i] = x + pd;
            }
            if rev > 0.0 && !rev_post {
                let (mut a, mut b) = (man, man);
                self.reverb(&mut a[..m], &mut b[..m], rev, spring);
                for i in 0..m {
                    man[i] = 0.5 * (a[i] + b[i]);
                }
            }
            // the cabinet
            for i in 0..m {
                let x = man[i];
                let (mut l, mut r) = (x, x);
                if rotary_on {
                    let low = if single { 0.0 } else { self.xover.lp(x) };
                    let high = x - low;
                    let low = self.body_lo.run(low);
                    let turn = if rear { 0.5 } else { 0.0 };
                    let hw = (self.horn.angle + turn) * std::f32::consts::TAU;
                    let dw = (self.drum.angle + turn) * std::f32::consts::TAU;
                    let mut hs = [0.0f32; 2];
                    let mut ds = [0.0f32; 2];
                    for c in 0..2 {
                        let side = if c == 0 { -1.0 } else { 1.0 };
                        let hcos = (hw + side * hwid * 0.5 * std::f32::consts::PI).cos();
                        let dcos = (dw + side * dwid * 0.5 * std::f32::consts::PI).cos();
                        self.horn_dl[c].push(high);
                        self.drum_dl[c].push(low);
                        let hd = self.horn_dl[c].read((0.0018 + hfm * hcos) * rate);
                        let dd = self.drum_dl[c].read((0.0022 + 0.0004 * depth * dcos) * rate);
                        // the horn is directional: louder and brighter facing the microphone
                        self.horn_tone[c].lowpass_hz((2500.0 + 3500.0 * (0.5 + 0.5 * hcos)) * if rear { 0.7 } else { 1.0 }, rate);
                        let mut h = self.horn_tone[c].lp(hd) * (1.0 - ham + ham * hcos);
                        if horn_ir {
                            h = self.throat[c * 2].run(h);
                            h = self.throat[c * 2 + 1].run(h);
                        }
                        hs[c] = self.far[c].lp(h) * 1.1 * hbal;
                        ds[c] = dd * (0.85 + 0.15 * depth * dcos) * dbal * if rear { 1.15 } else { 1.0 };
                        if real_cab {
                            hs[c] = self.mic_shelf[c].run(hs[c]);
                            ds[c] = self.mic_shelf[2 + c].run(ds[c]);
                        }
                    }
                    let (mut wl, mut wr) = if split {
                        // the bass rotor a little to the left, the treble rotor more to the right
                        (hs[0] * 0.55 + ds[0], hs[1] + ds[1] * 0.8)
                    } else {
                        (hs[0] + ds[0], hs[1] + ds[1])
                    };
                    wl = self.body_hi[0].run(wl);
                    wr = self.body_hi[1].run(wr);
                    if ms {
                        let (mid, sd) = (0.5 * (wl + wr), 0.5 * (wl - wr));
                        wl = mid + 1.7 * sd;
                        wr = mid - 1.7 * sd;
                    }
                    let bd = self.brake_dry;
                    l = (x * (1.0 - rotor_mix) + wl * rotor_mix) * (1.0 - bd) + x * dry_level * bd;
                    r = (x * (1.0 - rotor_mix) + wr * rotor_mix) * (1.0 - bd) + x * dry_level * bd;
                    self.horn.angle = (self.horn.angle + self.horn.speed / rate).rem_euclid(1.0);
                    self.drum.angle = (self.drum.angle + self.drum.speed / rate).rem_euclid(1.0);
                }
                ol[i] = l;
                or[i] = r;
            }
            if rev > 0.0 && rev_post {
                self.reverb(&mut ol[..m], &mut or[..m], rev, spring);
            }
            for i in 0..m {
                left[pos + i] += ol[i] * gl;
                right[pos + i] += or[i] * gr;
            }
            pos += m;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }
    fn play(o: &mut TonewheelB3, secs: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (48000.0 * secs) as usize;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            o.render(a, b);
        }
        (l, r)
    }
    fn organ() -> TonewheelB3 {
        let mut o = TonewheelB3::new(48000.0);
        o.set(18, 0.0); // cabinet off: steady levels
        o.set(15, 0.0);
        o
    }
    fn level(o: &mut TonewheelB3) -> f32 {
        play(o, 0.2);
        rms(&play(o, 0.3).0)
    }

    #[test]
    fn three_registers_have_their_own_drawbars() {
        let mut o = organ();
        o.set(61, 2.0); // Multi: pedals below C2 (48), lower below C3 (60)
        o.note_on(72, 100);
        let up = level(&mut o);
        o.note_off(72);
        o.note_on(55, 100);
        let low = level(&mut o);
        for d in 0..9 {
            o.set(LOWER + d, 0.0);
        }
        o.set(129, 0.0);
        let low_closed = level(&mut o);
        o.note_off(55);
        o.note_on(40, 100);
        let ped = level(&mut o);
        o.set(PEDAL, 0.0);
        o.set(PEDAL + 1, 0.0);
        let ped_closed = level(&mut o);
        assert!(up > 0.01 && low > 0.005 && ped > 0.005, "{up} {low} {ped}");
        assert!(low_closed < low * 0.05, "lower drawbars closed: {low_closed} vs {low}");
        assert!(ped_closed < ped * 0.05, "pedal drawbars closed: {ped_closed} vs {ped}");
    }

    #[test]
    fn channels_pick_the_register() {
        let mut o = organ();
        o.note_on_ch(1, 72, 100); // channel 2: the lower manual
        assert_eq!(o.down, [0, 1, 0]);
        o.note_on_ch(2, 50, 100); // channel 3: pedals
        assert_eq!(o.down, [0, 1, 1]);
        o.note_on_ch(0, 72, 100);
        assert_eq!(o.down, [1, 0, 1], "the same incoming note moves");
        o.note_off(72);
        o.note_off(50);
        assert_eq!(o.down, [0, 0, 0]);
    }

    #[test]
    fn split_points_and_transpose() {
        let mut o = organ();
        o.set(61, 1.0);
        assert_eq!(o.route(0, 59).0, 1);
        assert_eq!(o.route(0, 60).0, 0);
        o.set(61, 2.0);
        assert_eq!(o.route(0, 47).0, 2);
        // both split points the same: no lower manual
        o.set(62, 60.0);
        assert_eq!(o.route(0, 59).0, 2);
        o.set(64, 1.0);
        assert_eq!(o.route(0, 60), (0, 72));
        o.set(61, 0.0);
        assert_eq!(o.route(0, 40).0, 0, "Single: the whole keyboard plays the upper manual");
    }

    #[test]
    fn preset_keys_recall_and_remember() {
        let mut o = organ();
        assert_eq!(o.preset_key(0), 11);
        o.note_on(24 + 5, 100); // key F
        assert_eq!(o.preset_key(0), 5);
        let want: Vec<f32> = FACTORY_U[5].bytes().map(|b| (b - b'0') as f32).collect();
        let got: Vec<f32> = (0..9).map(|d| o.get(d)).collect();
        assert_eq!(got, want);
        assert_eq!(o.down, [0, 0, 0], "preset keys make no sound");
        // an edit is remembered by the key
        o.set(3, 7.0);
        o.note_on(24 + 11, 100);
        assert_eq!(o.get(3), 0.0);
        o.note_on(24 + 5, 100);
        assert_eq!(o.get(3), 7.0);
        // key C puts the factory registration back
        o.note_on(24, 100);
        assert_eq!(o.preset_key(0), 5);
        assert_eq!(o.get(3), want[3]);
        // Select via Keyboard off: the notes do nothing to the presets
        o.set(72, 0.0);
        o.note_on(24 + 2, 100);
        assert_eq!(o.preset_key(0), 5);
        // A# and B only: the upper drawbars edit key A#
        o.set(77, 1.0);
        o.set(5, 6.0);
        assert_eq!(o.get(PRESETS_U + 10 * 9 + 5), 6.0);
    }

    #[test]
    fn morph_steps_or_blends() {
        let mut o = organ();
        o.set(74, 2.0); // four presets: B, A#, A, G#
        o.set(75, 1.0);
        assert_eq!(o.preset_key(0), 8);
        o.set(75, 0.0);
        assert_eq!(o.preset_key(0), 11);
        o.set(73, 1.0);
        o.set(74, 0.0); // B and A#
        o.set(75, 0.5);
        // B = 888000000, A# = 888800000: the 4' bar is halfway
        assert!((o.get(3) - 4.0).abs() < 0.01, "{}", o.get(3));
        assert_eq!(o.get(PRESETS_U + 11 * 9 + 3), 0.0, "morphing doesn't overwrite the key");
        // a controller drives it
        o.set(76, 21.0);
        o.controller(21, 127);
        assert!((o.get(3) - 8.0).abs() < 0.01);
        o.save_morph_to(4);
        assert_eq!(o.get(PRESETS_U + 4 * 9 + 3), 8.0);
    }

    #[test]
    fn percussion_modes() {
        let mut o = organ();
        o.set(9, 1.0);
        o.note_on(60, 100);
        assert_eq!(o.perc_env, 1.0);
        play(&mut o, 0.3);
        let faded = o.perc_env;
        assert!(faded < 0.2);
        o.note_on(64, 100); // legato: Poly percussion doesn't retrigger
        assert!(o.perc_env <= faded);
        o.set(50, 1.0);
        o.note_on(67, 100);
        assert_eq!(o.perc_env, 1.0, "Mono retriggers on every key");
        // tied to preset key B only
        o.set(51, 0.0);
        o.set(70, 5.0);
        o.perc_env = 0.0;
        o.all_off();
        o.note_on(60, 100);
        assert_eq!(o.perc_env, 0.0);
        // slow at maximum: no decay
        o.set(51, 1.0);
        o.set(46, 0.0);
        o.set(48, 6.0);
        assert_eq!(o.perc_coef, 1.0);
    }

    #[test]
    fn rotor_speed_switching() {
        let mut o = TonewheelB3::new(48000.0);
        assert_eq!(o.get(18), 2.0);
        o.controller(1, 127); // ModWheel: upper third fast
        assert_eq!(o.get(18), 3.0);
        o.controller(1, 64);
        assert_eq!(o.get(18), 1.0);
        o.controller(1, 0);
        assert_eq!(o.get(18), 2.0);
        // sustain pedal toggles, and a long hold brakes
        o.set(27, 6.0);
        o.controller(64, 127);
        assert_eq!(o.get(18), 3.0);
        o.controller(64, 0);
        assert_eq!(o.get(18), 3.0);
        o.controller(64, 127);
        play(&mut o, 1.2);
        o.controller(64, 0);
        assert_eq!(o.get(18), 1.0, "held a second: brake");
        o.controller(64, 127);
        o.controller(64, 0);
        assert!(o.get(18) > 1.0);
        // aftertouch
        o.set(27, 5.0);
        o.set(18, 2.0);
        o.pressure(100);
        assert_eq!(o.get(18), 3.0);
        o.pressure(0);
        assert_eq!(o.get(18), 2.0);
        // motor modes
        o.set(18, 2.0);
        o.set(83, 2.0);
        assert_eq!(o.rotor_targets().1, 0.0, "910: the drum stands still at slow");
        o.set(83, 1.0);
        let (h, d) = o.rotor_targets();
        assert!(h > d, "Inv: horn fast, drum slow in slow mode");
        // brake with dry output: the cabinet is bypassed after a second
        o.set(83, 0.0);
        o.set(85, 0.0);
        o.set(23, 0.0);
        o.set(18, 1.0);
        play(&mut o, 2.5);
        assert!(o.brake_dry > 0.9);
    }

    #[test]
    fn effects_change_the_sound() {
        let base = {
            let mut o = organ();
            o.set(17, 0.0);
            o.note_on(60, 100);
            level(&mut o)
        };
        for (what, set) in [("eq", vec![(101, 1.0), (102, 12.0), (105, 6.0)]), ("wah", vec![(106, 1.0)]), ("dist", vec![(17, 1.0), (112, 2.0)]), ("reverb", vec![(114, 1.0), (116, 1.0)]), ("spring", vec![(114, 1.0), (115, 6.0), (116, 1.0)])] {
            let mut o = organ();
            o.set(17, 0.0);
            for (id, v) in &set {
                o.set(*id, *v);
            }
            o.note_on(60, 100);
            let x = level(&mut o);
            assert!((x - base).abs() > base * 0.03, "{what}: {x} vs {base}");
            // the master switch bypasses them all
            o.set(100, 0.0);
            let off = level(&mut o);
            assert!((off - base).abs() < base * 0.03, "{what} off: {off} vs {base}");
        }
    }

    #[test]
    fn hardware_controller_maps() {
        let mut o = organ();
        o.set(139, 1.0); // Roland VK / Korg CX
        o.controller(70, 0);
        assert_eq!(o.get(0), 8.0);
        o.controller(70, 127);
        assert_eq!(o.get(0), 0.0, "faders are reversed like the drawbars");
        o.controller(94, 127);
        assert_eq!(o.get(9), 1.0);
        o.set(139, 3.0); // B4D
        o.controller(21, 0);
        assert_eq!(o.get(LOWER), 8.0);
        o.set(139, 4.0); // Nord Electro
        o.controller(16, 60);
        assert_eq!(o.get(UPPER), 4.0);
        o.set(139, 5.0); // Crumar
        o.controller(33, 0);
        assert_eq!(o.get(PEDAL), 8.0);
        o.set(139, 0.0);
        o.controller(33, 127);
        assert_eq!(o.get(PEDAL), 8.0, "Off: the controllers aren't mapped");
    }

    #[test]
    fn learn_assigns_and_times_out() {
        let mut o = organ();
        o.set(110, 30.0);
        o.set(110, CTL_LEARN);
        o.controller(44, 90);
        assert_eq!(o.get(110), 44.0);
        o.set(110, CTL_LEARN);
        assert_eq!(o.learn.as_ref().unwrap().left, 20 * 48000);
        o.learn.as_mut().unwrap().left = 1000;
        play(&mut o, 0.1);
        assert_eq!(o.get(110), 44.0, "after the 20 s the old assignment is back");
    }

    #[test]
    fn organ_model_controls() {
        let mut o = organ();
        // Maximum Wheels folds the top down
        o.set(132, 48.0);
        assert!(o.wheel_for(100) <= 48);
        // Pitchbend Down all the way stops the wheels
        let mut o = organ();
        o.set(123, 13.0);
        o.note_on(60, 100);
        o.pitch_bend(-8192);
        let stopped = level(&mut o);
        assert!(stopped < 0.002, "{stopped}");
        // sustain: the note rings on after release
        let mut o = organ();
        o.set(124, 2.0);
        o.set(127, 1.0);
        o.note_on(60, 100);
        play(&mut o, 0.2);
        o.note_off(60);
        play(&mut o, 0.1);
        assert!(rms(&play(&mut o, 0.1).0) > 0.002);
        // lower volume
        let mut o = organ();
        o.note_on_ch(1, 60, 100);
        let a = level(&mut o);
        o.set(133, -24.0);
        let b = level(&mut o);
        assert!(b < a * 0.2);
    }

    #[test]
    fn settings_round_trip() {
        let mut a = organ();
        a.set(70, 3.0);
        a.set(4, 6.0);
        a.set(80, 4.0);
        a.set(112, 5.0);
        let saved: Vec<(u32, f32)> = params().iter().map(|p| (p.id, a.get(p.id))).collect();
        let mut b = TonewheelB3::new(48000.0);
        for (id, v) in &saved {
            b.set(*id, *v);
        }
        for (id, v) in &saved {
            assert_eq!(b.get(*id), *v, "param {id}");
        }
    }

    #[test]
    fn cabinets_differ() {
        let mut outs = Vec::new();
        for cab in 0..8 {
            let mut o = TonewheelB3::new(48000.0);
            o.set(80, cab as f32);
            o.note_on(72, 100);
            let (l, r) = play(&mut o, 1.0);
            assert!(l.iter().chain(r.iter()).all(|x| x.is_finite()));
            outs.push((rms(&l), rms(&r)));
        }
        for i in 1..8 {
            assert!(outs[i] != outs[i - 1], "cabinet {i} sounds like {}", i - 1);
        }
        // Split: the treble rotor to the right, the bass rotor a little to the left
        let mut o = TonewheelB3::new(48000.0);
        o.set(80, 4.0);
        for d in 0..3 {
            o.set(d, 0.0);
        }
        o.set(8, 8.0);
        o.note_on(84, 100);
        let (l, r) = play(&mut o, 1.0);
        assert!(rms(&r) > rms(&l) * 1.2);
        assert!(outs[4].0 > outs[4].1);
    }
}
