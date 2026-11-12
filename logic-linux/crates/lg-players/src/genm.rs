//! A Session Player region's settings as Logic stores them, read into and written from this
//! crate's parameter types.
//!
//! In a project's ProjectData (LOGICX-FORMAT, research DIFFS E09 and the SP-* projects):
//! - **GenM chunk** per player region. Payload: u32 payload length | u32 0 | u32 kind (0x17 for a
//!   player region's settings; 0x14 browser state, 0x0B a bplist) | u32 the region's EvSq id >> 16
//!   (the link to its region) | 8 × 0xFF | u32 count (5) | u32 JSON length (= payload length - 36)
//!   | u32 0 | JSON. The JSON's top level holds the live values ("CharacterIdentifier", "rComp" =
//!   complexity 0-100, "intensity" 0-100, "ghostNotes", "pushPull" -100..100, "fillsAmount",
//!   "swing", "humanize", "dynamics", per-player Details keys …), the factory "Preset" it came
//!   from, and a "GeneratorMemento" whose "Seeds" (i32, "0".."4") Regenerate changes.
//! - **Region chords**: 0x70 event records in the region's EvSq. Unit 1: 70 00 00 00 | u32 pos |
//!   00 00 00 00 | 67 00 00 00. Unit 2: u16 interval mask (0x91 major, 0x89 minor) | 07 0F 02 |
//!   root pitch class | 00 B2 (continuation) | 00 00 00 00 00 7F 00 00. A player follows its
//!   region's chords, or the chord track's (an EvSq of its own, same records) when it has none.
//! - The performance itself is pre-rendered into the region's EvSq as ordinary notes, which is
//!   what Logic plays; it does not re-generate on open.
//!
//! Known gaps (not decoded yet): which Details popup values map to which key values in a few
//! cases (marked "presumed" below), byte 2-4 and 6 of the chord unit (07 0F 02 / 00: probably
//! the chord's bass note and display flags), and whether Logic accepts a GenM whose memento
//! strings ("MementoParameters") disagree with the top-level values (we leave the memento as it
//! was and update the top level and the seeds).

use crate::bass::{BassParams, BassStyle};
use crate::chords::{Chord, ChordAt};
use crate::drummer::{DrummerParams, KitSection, SnareMode, SwingType, TempoFeel};
use crate::json::Json;
use crate::keys::{KeysParams, KeysStyle, LeftHand};
use lg_core::chunks;
use lg_core::events::{self, BAR_ONE};

/// The drummer's Dynamics value that matches our 1.0 (Half-pipe's 59, where we measured).
pub const DRUM_DYNAMICS: f32 = 59.0;

/// The GenM kind of a player region's settings.
pub const KIND_PLAYER: u32 = 0x17;

/// A GenM record.
#[derive(Clone, Debug, PartialEq)]
pub struct GenmRecord {
    /// The chunk's id (header u32 at 8)
    pub chunk_id: u32,
    pub kind: u32,
    /// The region's EvSq id >> 16
    pub sequence: u32,
    pub count: u32,
    pub json: Json,
}

impl GenmRecord {
    /// The EvSq id of the region it belongs to.
    pub fn evsq_id(&self) -> u32 {
        self.sequence << 16
    }
}

/// Reads a GenM payload: (kind, sequence, count, JSON). None when it holds no JSON.
pub fn parse_payload(p: &[u8]) -> Option<(u32, u32, u32, Json)> {
    let u = |o: usize| p.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let kind = u(8)?;
    let seq = u(12)?;
    let count = u(24)?;
    let len = u(28)? as usize;
    let body = p.get(36..36 + len).or_else(|| p.get(36..))?;
    let start = body.iter().position(|b| *b == b'{' || *b == b'[')?;
    let (json, _) = Json::parse_prefix(&body[start..])?;
    Some((kind, seq, count, json))
}

/// A GenM payload for a JSON record.
pub fn payload(kind: u32, sequence: u32, count: u32, json: &Json) -> Vec<u8> {
    let text = json.to_string_compact();
    let mut p = Vec::with_capacity(36 + text.len());
    p.extend(((36 + text.len()) as u32).to_le_bytes());
    p.extend(0u32.to_le_bytes());
    p.extend(kind.to_le_bytes());
    p.extend(sequence.to_le_bytes());
    p.extend([0xFF; 8]);
    p.extend(count.to_le_bytes());
    p.extend((text.len() as u32).to_le_bytes());
    p.extend(0u32.to_le_bytes());
    p.extend(text.as_bytes());
    p
}

/// A whole GenM chunk (36-byte chunk header + payload), as the samples have them.
pub fn chunk(chunk_id: u32, payload: &[u8]) -> Vec<u8> {
    let mut c = Vec::with_capacity(36 + payload.len());
    c.extend(*b"MneG");
    c.extend(1u16.to_le_bytes());
    c.extend(30u16.to_le_bytes());
    c.extend(chunk_id.to_le_bytes());
    c.extend([0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 2, 0, 0, 0, 2, 0]);
    c.extend((payload.len() as u64).to_le_bytes());
    c.extend_from_slice(payload);
    c
}

/// The GenM records of a ProjectData (undo history skipped).
pub fn records(project_data: &[u8]) -> Vec<GenmRecord> {
    let Ok(all) = chunks::walk(project_data) else { return Vec::new() };
    all.iter()
        .filter(|c| c.is("GenM"))
        .filter_map(|c| {
            let (kind, sequence, count, json) = parse_payload(c.payload)?;
            Some(GenmRecord { chunk_id: c.id, kind, sequence, count, json })
        })
        .collect()
}

/// A player region's settings.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerSettings {
    Drummer(DrummerParams),
    Bass(BassParams),
    Keys(KeysParams),
}

fn num(j: &Json, k: &str) -> Option<f32> {
    j.get(k).and_then(Json::num).map(|v| v as f32)
}

fn pct(j: &Json, k: &str, default: f32) -> f32 {
    num(j, k).map_or(default, |v| v / 100.0)
}

fn tempo_of(j: &Json) -> TempoFeel {
    // presumed: 1 half, 2 double (the presets use 0, 2, 3)
    match num(j, "HalfTimeDoubleTime").unwrap_or(0.0) as i32 {
        1 => TempoFeel::Half,
        2 => TempoFeel::Double,
        // presumed: 3 automatic (presets use it; unverified)
        3 => TempoFeel::Automatic,
        _ => TempoFeel::Standard,
    }
}

fn tempo_value(t: TempoFeel) -> f64 {
    match t {
        TempoFeel::Standard => 0.0,
        TempoFeel::Half => 1.0,
        TempoFeel::Double => 2.0,
        TempoFeel::Automatic => 3.0,
    }
}

fn swing_type_of(j: &Json) -> SwingType {
    // presumed: 0 sixteenths, 1 eighths
    if num(j, "swingType").unwrap_or(0.0) as i32 == 1 { SwingType::Eighths } else { SwingType::Sixteenths }
}

fn swing_type_value(s: SwingType) -> f64 {
    match s {
        SwingType::Sixteenths => 0.0,
        SwingType::Eighths => 1.0,
    }
}

/// The region's seed (GeneratorMemento.Seeds."0", an i32).
pub fn seed_of(j: &Json) -> Option<u64> {
    let s = j.get("GeneratorMemento")?.get("Seeds")?.get("0")?.num()?;
    Some(s as i64 as i32 as u32 as u64)
}

impl PlayerSettings {
    /// Reads a GenM JSON record (None when it is not a player region's).
    pub fn from_json(j: &Json) -> Option<PlayerSettings> {
        let ty = j.get("RegionType").and_then(Json::str).unwrap_or("");
        let id = j.get("CharacterIdentifier").and_then(Json::str).unwrap_or("").to_string();
        let seed = seed_of(j).unwrap_or(1);
        if ty.contains("Drummer") || ty.contains("Percussion") {
            let d = DrummerParams::default();
            let kick = num(j, "kickEnabled").unwrap_or(1.0) != 0.0 || num(j, "snareEnabled").unwrap_or(1.0) != 0.0;
            let perc = num(j, "percStyle").unwrap_or(0.0) as u8;
            let snare = match num(j, "snareArticulation").unwrap_or(0.0) as i32 {
                // presumed order of the Snare popup
                1 => SnareMode::Snare,
                2 => SnareMode::SideStick,
                3 => SnareMode::Rimshot,
                _ => SnareMode::Automatic,
            };
            let hihat = match num(j, "rightHandArticulation2").unwrap_or(0.0) as i32 {
                // presumed: 0 half-closed (Logic's default), 1 closed, 2 open
                1 => 0.1,
                2 => 0.7,
                _ => 0.35,
            };
            return Some(PlayerSettings::Drummer(DrummerParams {
                character: if id.is_empty() { d.character.clone() } else { id },
                complexity: pct(j, "rComp", d.complexity),
                intensity: pct(j, "intensity", d.intensity),
                kick_snare: KitSection { enabled: kick, variation: num(j, "systemVariation1").unwrap_or(0.0) as u8 },
                cymbals: KitSection { enabled: true, variation: num(j, "rightHandArticulation1").unwrap_or(0.0) as u8 },
                percussion: KitSection { enabled: perc > 0, variation: perc },
                fills: pct(j, "fillsAmount", d.fills),
                swing: pct(j, "swing", d.swing),
                swing_type: swing_type_of(j),
                feel: pct(j, "pushPull", d.feel),
                ghost_notes: pct(j, "ghostNotes", d.ghost_notes),
                hihat,
                snare,
                // (the drummer's Dynamics: Logic's 59 is the range we measured, our 1.0)
                dynamics: num(j, "dynamics").map_or(1.0, |v| v / DRUM_DYNAMICS).max(0.1),
                humanize: pct(j, "humanize", d.humanize),
                tempo: tempo_of(j),
                follow: if j.get("FollowEnabled").and_then(Json::bool).unwrap_or(false) { 1.0 } else { 0.0 },
                seed,
                channel: d.channel,
            }));
        }
        if ty.contains("Bass") {
            let d = BassParams::default();
            return Some(PlayerSettings::Bass(BassParams {
                style: BassStyle::find(&id).unwrap_or(d.style),
                complexity: pct(j, "rComp", d.complexity),
                intensity: pct(j, "intensity", d.intensity),
                fills: pct(j, "fillsAmount", d.fills),
                swing: pct(j, "swing", d.swing),
                swing_type: swing_type_of(j),
                feel: pct(j, "pushPull", d.feel),
                dead_notes: pct(j, "deadNotes", d.dead_notes),
                slides: pct(j, "slidesAmount", d.slides),
                pickup_hits: num(j, "pickupHits").unwrap_or(0.0) != 0.0,
                dynamic_mute: num(j, "dynamicMute").unwrap_or(1.0) != 0.0,
                align_root: num(j, "autoRoot").unwrap_or(1.0) != 0.0,
                double_stops: pct(j, "doubleStops", d.double_stops),
                mute_offset: pct(j, "muteGradeOffset", d.mute_offset),
                dynamics: pct(j, "dynamics", 1.0).max(0.1),
                humanize: pct(j, "humanize", d.humanize),
                lowest_note: num(j, "lowestNote").map_or(d.lowest_note, |v| v.round().clamp(20.0, 70.0) as u8),
                tempo: tempo_of(j),
                seed,
                channel: d.channel,
                ..d.clone()
            }));
        }
        if ty.contains("Piano") || ty.contains("Pad") || ty.contains("Keyboard") {
            let d = KeysParams::default();
            let style = KeysStyle::find(&id).unwrap_or(if ty.contains("Pad") { KeysStyle::SupportingPad } else { d.style });
            let lower_on = num(j, "lowerEnabled").unwrap_or(1.0) != 0.0;
            let left_hand = if !lower_on {
                LeftHand::Off
            } else {
                // presumed order of the left-hand playing styles
                // (Logic's 2 measured as single roots, struck again mid-bar)
                match num(j, "lowerPlayingStyle").unwrap_or(1.0) as i32 {
                    3 => LeftHand::Octaves,
                    4 => LeftHand::RootFifth,
                    5 => LeftHand::Chord,
                    6 => LeftHand::Moving,
                    7 => LeftHand::RootFifthOctave,
                    8 => LeftHand::Broken,
                    _ => LeftHand::Root,
                }
            };
            return Some(PlayerSettings::Keys(KeysParams {
                style,
                complexity: pct(j, "rComp", d.complexity),
                intensity: pct(j, "intensity", d.intensity),
                fills: pct(j, "fillsAmount", d.fills),
                swing: pct(j, "swing", d.swing),
                swing_type: swing_type_of(j),
                feel: pct(j, "pushPull", d.feel),
                grace_notes: pct(j, "graceNotesAmount", d.grace_notes),
                dynamics: pct(j, "dynamics", 1.0).max(0.1),
                humanize: pct(j, "humanize", d.humanize),
                upper_position: num(j, "upperPosition").map_or(d.upper_position, |v| v as u8),
                lower_position: num(j, "lowerPosition").map_or(d.lower_position, |v| v as u8),
                upper_voicing: num(j, "upperVoicingStyle").map_or(d.upper_voicing, |v| v as u8),
                left_hand,
                arp_direction: num(j, "arpDirection").map_or(d.arp_direction, |v| v as u8),
                tempo: tempo_of(j),
                seed,
                channel: d.channel,
                ..d.clone()
            }));
        }
        None
    }

    /// Writes the settings into a GenM JSON record (top-level values and the seeds; other keys
    /// are kept as they were).
    pub fn write_into(&self, j: &mut Json) {
        let n = |v: f32| Json::Num(v as f64 * 100.0);
        let b = |v: bool| Json::Num(v as u8 as f64);
        match self {
            PlayerSettings::Drummer(p) => {
                j.set("CharacterIdentifier", Json::Str(p.character().id.into()));
                j.set("RegionType", Json::Str(if p.character().electronic() { "Type_ElectronicDrummerV2" } else { "Type_AcousticDrummerV2" }.into()));
                j.set("rComp", n(p.complexity));
                j.set("intensity", n(p.intensity));
                j.set("kickEnabled", b(p.kick_snare.enabled));
                j.set("snareEnabled", b(p.kick_snare.enabled));
                j.set("systemVariation1", Json::Num(p.kick_snare.variation as f64));
                j.set("rightHandArticulation1", Json::Num(p.cymbals.variation as f64));
                j.set("percStyle", Json::Num(if p.percussion.enabled { p.percussion.variation.max(1) as f64 } else { 0.0 }));
                j.set("fillsAmount", n(p.fills));
                j.set("swing", n(p.swing));
                j.set("swingType", Json::Num(swing_type_value(p.swing_type)));
                j.set("pushPull", n(p.feel));
                j.set("ghostNotes", n(p.ghost_notes));
                j.set("rightHandArticulation2", Json::Num(if p.hihat < 0.2 { 1.0 } else if p.hihat > 0.55 { 2.0 } else { 0.0 }));
                j.set("snareArticulation", Json::Num(match p.snare {
                    SnareMode::Automatic => 0.0,
                    SnareMode::Snare => 1.0,
                    SnareMode::SideStick => 2.0,
                    SnareMode::Rimshot => 3.0,
                }));
                j.set("dynamics", Json::Num((p.dynamics * DRUM_DYNAMICS) as f64));
                j.set("humanize", n(p.humanize));
                j.set("HalfTimeDoubleTime", Json::Num(tempo_value(p.tempo)));
                j.set("FollowEnabled", Json::Bool(p.follow > 0.0));
                set_seed(j, p.seed);
            }
            PlayerSettings::Bass(p) => {
                j.set("CharacterIdentifier", Json::Str(p.style.id().into()));
                j.set("RegionType", Json::Str("Type_ElectricBassV2".into()));
                j.set("rComp", n(p.complexity));
                j.set("intensity", n(p.intensity));
                j.set("fillsAmount", n(p.fills));
                j.set("swing", n(p.swing));
                j.set("swingType", Json::Num(swing_type_value(p.swing_type)));
                j.set("pushPull", n(p.feel));
                j.set("deadNotes", n(p.dead_notes));
                j.set("slidesAmount", n(p.slides));
                j.set("pickupHits", b(p.pickup_hits));
                j.set("dynamicMute", b(p.dynamic_mute));
                j.set("autoRoot", b(p.align_root));
                j.set("doubleStops", n(p.double_stops));
                j.set("muteGradeOffset", n(p.mute_offset));
                j.set("dynamics", n(p.dynamics));
                j.set("humanize", n(p.humanize));
                j.set("lowestNote", Json::Num(p.lowest_note as f64));
                j.set("HalfTimeDoubleTime", Json::Num(tempo_value(p.tempo)));
                set_seed(j, p.seed);
            }
            PlayerSettings::Keys(p) => {
                j.set("CharacterIdentifier", Json::Str(p.style.id().into()));
                j.set("RegionType", Json::Str(if p.style == KeysStyle::SupportingPad { "Type_PadPlayerV2" } else { "Type_AcousticPianoV2" }.into()));
                j.set("rComp", n(p.complexity));
                j.set("intensity", n(p.intensity));
                j.set("fillsAmount", n(p.fills));
                j.set("swing", n(p.swing));
                j.set("swingType", Json::Num(swing_type_value(p.swing_type)));
                j.set("pushPull", n(p.feel));
                j.set("graceNotesAmount", n(p.grace_notes));
                j.set("dynamics", n(p.dynamics));
                j.set("humanize", n(p.humanize));
                j.set("upperPosition", Json::Num(p.upper_position as f64));
                j.set("lowerPosition", Json::Num(p.lower_position as f64));
                j.set("upperVoicingStyle", Json::Num(p.upper_voicing as f64));
                j.set("lowerEnabled", b(p.left_hand != LeftHand::Off));
                j.set("lowerPlayingStyle", Json::Num(match p.left_hand {
                    LeftHand::Off | LeftHand::Root => 2.0,
                    LeftHand::Octaves => 3.0,
                    LeftHand::RootFifth => 4.0,
                    LeftHand::Chord => 5.0,
                    LeftHand::Moving => 6.0,
                    // (Studio's own values; Logic's numbers for these are not known)
                    LeftHand::RootFifthOctave => 7.0,
                    LeftHand::Broken => 8.0,
                }));
                j.set("arpDirection", Json::Num(p.arp_direction as f64));
                j.set("HalfTimeDoubleTime", Json::Num(tempo_value(p.tempo)));
                set_seed(j, p.seed);
            }
        }
    }

    /// A fresh GenM JSON record for these settings.
    pub fn new_json(&self) -> Json {
        let mut j = Json::Obj(Vec::new());
        j.set("ParameterSetVersion", Json::Num(2.0));
        self.write_into(&mut j);
        j
    }
}

fn set_seed(j: &mut Json, seed: u64) {
    let s = Json::Num(seed as u32 as i32 as f64);
    if j.get("GeneratorMemento").is_none() {
        j.set("GeneratorMemento", Json::Obj(Vec::new()));
    }
    let Some(m) = j.get_mut("GeneratorMemento") else { return };
    if m.get("Seeds").is_none() {
        m.set("Seeds", Json::Obj(Vec::new()));
    }
    let Some(seeds) = m.get_mut("Seeds") else { return };
    for k in ["0", "1", "2", "3", "4", "Seeds_StyleGraphGlobalToMerged"] {
        seeds.set(k, s.clone());
    }
}

/// The settings of the player region whose events are EvSq `evsq_id` (lg_core's
/// `Region::source.seq`), from a ProjectData.
pub fn region_settings(project_data: &[u8], evsq_id: u32) -> Option<PlayerSettings> {
    records(project_data)
        .into_iter()
        .filter(|r| r.kind == KIND_PLAYER && r.evsq_id() == evsq_id)
        .next_back()
        .and_then(|r| PlayerSettings::from_json(&r.json))
}

/// All player regions' settings: (EvSq id, settings).
pub fn all_settings(project_data: &[u8]) -> Vec<(u32, PlayerSettings)> {
    let mut out: Vec<(u32, PlayerSettings)> = Vec::new();
    for r in records(project_data).into_iter().filter(|r| r.kind == KIND_PLAYER) {
        if let Some(s) = PlayerSettings::from_json(&r.json) {
            out.retain(|(id, _)| *id != r.evsq_id());
            out.push((r.evsq_id(), s));
        }
    }
    out
}

/// The chords of an event list (a region's or the chord track's), at song ticks in lg_core's
/// counting (Logic's position minus [`BAR_ONE`]).
pub fn chords_in(evsq_payload: &[u8]) -> Vec<ChordAt> {
    events::records(evsq_payload)
        .iter()
        .filter(|r| r.kind == 0x70 && r.units() >= 2)
        .map(|r| {
            let mask = r.u16_at(16) & 0x0FFF;
            let root = r.u8_at(21) % 12;
            ChordAt { pos: r.pos.saturating_sub(BAR_ONE), chord: Chord { root, mask: if mask == 0 { 0x91 } else { mask }, bass: None } }
        })
        .collect()
}

/// A chord as a 0x70 event record (32 bytes; `pos` in lg_core's counting).
pub fn chord_record(pos: u32, chord: &Chord) -> [u8; 32] {
    let mut b = [0u8; 32];
    b[0] = 0x70;
    b[4..8].copy_from_slice(&(pos + BAR_ONE).to_le_bytes());
    b[12] = 0x67;
    b[16..18].copy_from_slice(&(chord.mask & 0x0FFF).to_le_bytes());
    b[18] = 0x07;
    b[19] = 0x0F;
    b[20] = 0x02;
    b[21] = chord.root % 12;
    b[23] = 0xB2;
    b[29] = 0x7F;
    b
}

/// The chords of every event list in a ProjectData: (EvSq id, chords).
pub fn all_chords(project_data: &[u8]) -> Vec<(u32, Vec<ChordAt>)> {
    let Ok(all) = chunks::walk(project_data) else { return Vec::new() };
    let mut out: Vec<(u32, Vec<ChordAt>)> = Vec::new();
    for c in all.iter().filter(|c| c.is("EvSq") && c.id != 0) {
        let ch = chords_in(c.payload);
        if !ch.is_empty() {
            out.retain(|(id, _)| *id != c.id);
            out.push((c.id, ch));
        }
    }
    out
}

/// One value of Logic's compact memento strings ("name=<type><length>,<value>").
#[derive(Clone, Debug, PartialEq)]
pub enum CompactValue {
    Bool(bool),
    Num(f64),
    Str(String),
    Table(String),
}

/// Reads a compact memento string (e.g. GeneratorMemento.MementoParameters.regionStartParams).
pub fn parse_compact(s: &str) -> Vec<(String, CompactValue)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].to_string();
        let after = &rest[eq + 1..];
        let ty = after.chars().next().unwrap_or('s');
        let Some(comma) = after.find(',') else { break };
        let Ok(len) = after[1..comma].parse::<usize>() else { break };
        let Some(value) = after.get(comma + 1..comma + 1 + len) else { break };
        out.push((
            name,
            match ty {
                'b' => CompactValue::Bool(value == "true"),
                'n' => CompactValue::Num(value.parse().unwrap_or(0.0)),
                't' => CompactValue::Table(value.to_string()),
                _ => CompactValue::Str(value.to_string()),
            },
        ));
        rest = &after[comma + 1 + len..];
    }
    out
}

/// Writes a compact memento string.
pub fn to_compact(v: &[(String, CompactValue)]) -> String {
    let mut s = String::new();
    for (k, x) in v {
        let (ty, text) = match x {
            CompactValue::Bool(b) => ('b', b.to_string()),
            CompactValue::Num(n) => ('n', if n.fract() == 0.0 { format!("{}", *n as i64) } else { format!("{n}") }),
            CompactValue::Str(t) => ('s', t.clone()),
            CompactValue::Table(t) => ('t', t.clone()),
        };
        s.push_str(&format!("{k}={ty}{},{text}", text.len()));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_round_trip() {
        let s = PlayerSettings::Drummer(DrummerParams { complexity: 0.5, intensity: 0.25, ghost_notes: 0.6, seed: 0xDEAD_BEEF, ..Default::default() });
        let p = payload(KIND_PLAYER, 0x28, 5, &s.new_json());
        let (kind, seq, count, j) = parse_payload(&p).unwrap();
        assert_eq!((kind, seq, count), (KIND_PLAYER, 0x28, 5));
        assert_eq!(PlayerSettings::from_json(&j), Some(s));
        let c = chunk(0xC0000, &p);
        assert_eq!(&c[..4], b"MneG");
        assert_eq!(c.len(), 36 + p.len());
    }

    #[test]
    fn bass_and_keys_round_trip() {
        for s in [
            PlayerSettings::Bass(BassParams { style: BassStyle::Slap, dead_notes: 0.5, pickup_hits: true, lowest_note: 35, seed: 77, ..Default::default() }),
            PlayerSettings::Keys(KeysParams { style: KeysStyle::Arpeggiated, upper_voicing: 5, left_hand: LeftHand::Octaves, seed: 5, ..Default::default() }),
        ] {
            let j = s.new_json();
            let back = PlayerSettings::from_json(&Json::parse(&j.to_string_compact()).unwrap()).unwrap();
            assert_eq!(back, s);
        }
    }

    #[test]
    fn chord_records() {
        let c = Chord::parse("Am").unwrap();
        let rec = chord_record(3840, &c);
        assert_eq!(&rec[16..18], &[0x89, 0x00]);
        let mut list = rec.to_vec();
        let mut end = [0u8; 16];
        end[0] = 0xF1;
        end[4..8].copy_from_slice(&0x3FFF_FFFFu32.to_le_bytes());
        list.extend(end);
        assert_eq!(chords_in(&list), vec![ChordAt { pos: 3840, chord: c }]);
    }

    #[test]
    fn compact_strings() {
        let s = "kickEnabled=b4,truefillsLength=n2,57manualKick=s0,complexity=n5,84.79";
        let v = parse_compact(s);
        assert_eq!(v.len(), 4);
        assert_eq!(v[3], ("complexity".into(), CompactValue::Num(84.79)));
        assert_eq!(to_compact(&v), s);
    }
}
