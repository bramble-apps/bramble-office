//! The mixer as a project saves it (format-analysis §15, §23, §24 and the E07 send decode):
//! channel strips, their routing (sends, inputs, outputs), groups.
//!
//! - AuCO = one channel object per strip; Logic pre-allocates hundreds. Offsets from the chunk's
//!   start (36-byte header included): @0x28 type (0x40 Audio, 0x41 Input, 0x42 Aux, 0x43
//!   Instrument, 0x44 Output, 0x45 Bus, 0x46 Master), @0x2A its number (0 = "Aux 1"), @0x60 the
//!   name ("Aux 1"), @0x79 volume on the 0..127 fader scale (90 = 0 dB), @0x7A in use, @0x7D pan
//!   0..127 (64 = centre), @0x7E bit 0 mute, @0x7C bit 0 solo; the header's u32 @12 >> 16 is the
//!   channel number the strip's AuCU plug-ins name.
//! - Each strip has a 16-byte UUID after an 0xFF marker; a strip routed to a bus names that bus's
//!   UUID after it: a track's (or an aux's first) reference is its output bus, an aux's last
//!   one its input bus (E07, E10, E22, E23).
//! - A send is a 112-byte AuCU on the sending channel (slot 0 the first send, 1 the next): payload @0x2C the send's own
//!   UUID, @0x3C the destination bus's UUID. Its level is read from the float @0x20 (0.0 in every
//!   sample: a new send at 0 dB; not yet checked with a changed level) and @0x10 bit 0 as pre
//!   fader (unchecked).
//! - Group membership is the track's Envi payload byte 0x18 (group number, 0 = none); a group's
//!   settings are an unnamed global MSeq (id 0) whose payload bytes 0x46..0x49 are the link mask.

use std::collections::BTreeMap;

use crate::chunks::{self, Chunk};
use crate::plugin::Plugin;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripKind {
    Audio,
    Input,
    Aux,
    Instrument,
    Output,
    Bus,
    Master,
    Other(u8),
}

impl StripKind {
    fn from_code(c: u8) -> StripKind {
        match c {
            0x40 => StripKind::Audio,
            0x41 => StripKind::Input,
            0x42 => StripKind::Aux,
            0x43 => StripKind::Instrument,
            0x44 => StripKind::Output,
            0x45 => StripKind::Bus,
            0x46 => StripKind::Master,
            c => StripKind::Other(c),
        }
    }
}

/// One channel strip (AuCO) in use.
#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    pub kind: StripKind,
    /// Its number in its kind from 0 ("Aux 1" = 0).
    pub number: u16,
    /// The channel number its plug-ins (AuCU) give.
    pub channel: u32,
    pub name: String,
    pub volume_db: f32,
    /// -1 (left) .. 1 (right).
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Buses (from 0 = "Bus 1"): an aux's input; where the strip's output goes (None: the
    /// Stereo Out).
    pub input_bus: Option<usize>,
    pub output_bus: Option<usize>,
    /// The strip's plug-ins in slot order (instrument slot 2, Audio FX from 3).
    pub plugins: Vec<Plugin>,
}

/// A send: from a strip (index in `Mixer::strips`) to a bus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Send {
    pub from: usize,
    pub bus: usize,
    pub level_db: f32,
    pub pre: bool,
}

/// A mixer group: its number (1-based), name and Logic's link mask (MSeq bytes 0x46..0x49).
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub number: u8,
    pub name: String,
    pub mask: [u8; 4],
}

/// A new group's mask: Volume and Mute linked, Automation Mode on (format-analysis §25).
pub const DEFAULT_GROUP_MASK: [u8; 4] = [0x05, 0x00, 0x40, 0x81];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mixer {
    /// The strips in use (tracks', auxes', buses', outputs'), in file order.
    pub strips: Vec<Strip>,
    pub sends: Vec<Send>,
    pub groups: Vec<Group>,
    /// Per track (index in Project::tracks): its strip, and its group number (0 none).
    pub track_strip: Vec<Option<usize>>,
    pub track_group: Vec<u8>,
}

impl Mixer {
    /// The aux strips (indexes in `strips`), in their number order.
    pub fn auxes(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.strips.len()).filter(|&i| self.strips[i].kind == StripKind::Aux).collect();
        v.sort_by_key(|&i| self.strips[i].number);
        v
    }

    /// The sends of strip `s`.
    pub fn sends_of(&self, s: usize) -> Vec<Send> {
        self.sends.iter().filter(|x| x.from == s).copied().collect()
    }
}

/// Logic's 0..127 fader value to dB (90 = 0 dB, 127 = +6 dB, 63 ≈ −6 dB).
pub fn fader_to_db(v: f32) -> f32 {
    if v <= 0.5 {
        -90.0
    } else if v >= 90.0 {
        (v - 90.0) * 6.0 / 37.0
    } else {
        40.0 * (v / 90.0).log10()
    }
}

pub fn db_to_fader(db: f32) -> f32 {
    if db <= -89.0 {
        0.0
    } else if db >= 0.0 {
        (90.0 + db * 37.0 / 6.0).min(127.0)
    } else {
        (90.0 * 10f32.powf(db / 40.0)).clamp(0.0, 90.0)
    }
}

fn uuid_after_marker(r: &[u8]) -> Option<(usize, [u8; 16])> {
    // the strip's own UUID: the first 0xFF marker followed by 16 non-zero bytes past the name
    (0xA0..r.len().saturating_sub(16)).find_map(|o| {
        let u: [u8; 16] = r.get(o + 1..o + 17)?.try_into().ok()?;
        (r[o] == 0xFF && u.iter().filter(|b| **b != 0).count() >= 8 && u[0] != 0xEE).then_some((o + 1, u))
    })
}

/// Reads the mixer from a project's chunks into `p.mixer`; sets the tracks' volume, pan, mute
/// and solo from their strips.
pub fn read(live: &[&Chunk], p: &mut crate::Project) {
    let mut m = Mixer::default();
    // the strips in use, and every strip's UUID (a bus is named by its UUID even when unused)
    let objects: Vec<&&Chunk> = live.iter().filter(|c| c.is("AuCO") && c.raw.len() > 0xC0).collect();
    let mut uuids: Vec<([u8; 16], StripKind, u16)> = Vec::new();
    for c in &objects {
        if let Some((_, u)) = uuid_after_marker(c.raw) {
            uuids.push((u, StripKind::from_code(c.raw[0x28]), u16::from_le_bytes([c.raw[0x2A], c.raw[0x2B]])));
        }
    }
    let known: std::collections::HashSet<[u8; 16]> = uuids.iter().map(|x| x.0).collect();
    let mut refs: Vec<Vec<[u8; 16]>> = Vec::new();
    for c in &objects {
        let r = c.raw;
        let kind = StripKind::from_code(r[0x28]);
        let number = u16::from_le_bytes([r[0x2A], r[0x2B]]);
        if r[0x7A] == 0 || kind == StripKind::Master {
            continue;
        }
        let name = r[0x61..0x70].split(|b| *b == 0).next().map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_default();
        // the other strips' UUIDs named after the strip's own one
        let mut rs = Vec::new();
        if let Some((at, _)) = uuid_after_marker(r) {
            let mut o = at + 16;
            while o + 16 <= r.len() {
                let u: [u8; 16] = r[o..o + 16].try_into().unwrap();
                if known.contains(&u) {
                    rs.push(u);
                    o += 16;
                } else {
                    o += 1;
                }
            }
        }
        refs.push(rs);
        m.strips.push(Strip { kind, number, channel: c.u32_at(12).unwrap_or(0) >> 16, name, volume_db: fader_to_db(r[0x79] as f32), pan: ((r[0x7D] as f32 - 64.0) / 63.0).clamp(-1.0, 1.0), mute: r[0x7E] & 1 != 0, solo: r[0x7C] & 1 != 0, input_bus: None, output_bus: None, plugins: Vec::new() });
    }
    let bus_of = |u: &[u8; 16]| uuids.iter().find(|x| &x.0 == u && x.1 == StripKind::Bus).map(|x| x.2 as usize);
    for (s, rs) in m.strips.iter_mut().zip(&refs) {
        let buses: Vec<usize> = rs.iter().filter_map(bus_of).collect();
        if s.kind == StripKind::Aux {
            s.input_bus = buses.last().copied();
            if buses.len() > 1 {
                s.output_bus = buses.first().copied();
            }
        } else {
            s.output_bus = buses.first().copied();
        }
    }
    // plug-ins by channel; sends (slot 0)
    for c in live.iter().filter(|c| c.is("AuCU")) {
        let ch = c.u32_at(12).unwrap_or(0) >> 16;
        let slot = c.u32_at(16).unwrap_or(0) >> 16;
        let Some(si) = m.strips.iter().position(|s| s.channel == ch && s.kind != StripKind::Bus) else { continue };
        // (a send unit: 112 bytes naming a bus; slots 0, 1 … are the sends in order)
        if c.raw.len() == 112 && c.payload.len() >= 0x4C {
            let pl = c.payload;
            let dest: [u8; 16] = pl[0x3C..0x4C].try_into().unwrap();
            if let Some(bus) = bus_of(&dest) {
                let level = f32::from_le_bytes(pl[0x20..0x24].try_into().unwrap());
                let level_db = if level.is_finite() { level.clamp(-90.0, 6.0) } else { 0.0 };
                m.sends.push(Send { from: si, bus, level_db, pre: false });
            }
            continue;
        }
        if let Some(mut u) = crate::plugin::read_unit(c.payload) {
            u.channel = ch;
            u.slot = slot;
            m.strips[si].plugins.push(u);
        }
    }
    for s in &mut m.strips {
        s.plugins.sort_by_key(|u| u.slot);
    }
    // aux names: the aux tracks' environment objects (type 0x1224) in order, when they match
    let aux_envi: Vec<String> = live.iter().filter(|c| c.is("Envi") && c.payload.get(0x94..0x96) == Some(&[0x24, 0x12])).filter_map(|c| chunks::short_string(c.payload, 158)).collect();
    let auxes = m.auxes();
    if aux_envi.len() == auxes.len() {
        for (k, a) in auxes.into_iter().enumerate() {
            if !aux_envi[k].is_empty() {
                m.strips[a].name = aux_envi[k].clone();
            }
        }
    }
    // the tracks' strips: the instrument tracks (channel-object order) take the instrument
    // strips in channel order (as project.rs gives them their plug-ins), the audio tracks the
    // audio strips
    let mut track_strip = vec![None; p.tracks.len()];
    for (want_audio, kind) in [(false, StripKind::Instrument), (true, StripKind::Audio)] {
        let mut order: Vec<usize> = (0..p.tracks.len()).filter(|&i| (p.tracks[i].kind == crate::TrackKind::Audio) == want_audio).collect();
        order.sort_by_key(|&i| p.tracks[i].channel);
        let mut chans: Vec<u32> = Vec::new();
        for &i in &order {
            if !chans.contains(&p.tracks[i].channel) {
                chans.push(p.tracks[i].channel);
            }
        }
        let mut strips: Vec<usize> = (0..m.strips.len()).filter(|&s| m.strips[s].kind == kind).collect();
        if kind == StripKind::Instrument {
            strips.retain(|&s| m.strips[s].plugins.iter().any(|u| u.name != "Klopfgeist"));
        }
        strips.sort_by_key(|&s| m.strips[s].channel);
        for (k, ch) in chans.iter().enumerate() {
            let Some(&s) = strips.get(k) else { break };
            for i in order.iter().copied().filter(|&i| p.tracks[i].channel == *ch) {
                track_strip[i] = Some(s);
            }
        }
    }
    for (i, s) in track_strip.iter().enumerate() {
        if let Some(s) = s {
            let st = &m.strips[*s];
            let t = &mut p.tracks[i];
            t.volume_db = st.volume_db;
            t.pan = st.pan;
            t.mute = st.mute;
            t.solo = st.solo;
        }
    }
    m.track_strip = track_strip;
    // groups: membership from each track's Envi, settings from the group sequences
    let mut member: BTreeMap<u32, u8> = BTreeMap::new();
    for c in live.iter().filter(|c| c.is("Envi")) {
        if let Some(&g) = c.payload.get(0x18) {
            member.insert(c.id >> 16, g);
        }
    }
    m.track_group = p.tracks.iter().map(|t| member.get(&t.channel).copied().filter(|g| *g < 33).unwrap_or(0)).collect();
    let settings: Vec<(String, [u8; 4])> = live
        .iter()
        .filter(|c| c.is("MSeq") && c.id == 0)
        .filter_map(|c| {
            let mask: [u8; 4] = c.payload.get(0x46..0x4A)?.try_into().ok()?;
            let name = chunks::short_string(c.payload, 16).unwrap_or_default();
            (mask[3] & 0x80 != 0 && name != "Untitled" && name != "TRASH").then_some((name, mask))
        })
        .collect();
    let top = m.track_group.iter().copied().max().unwrap_or(0);
    for g in 1..=top {
        let (name, mask) = settings.get(g as usize - 1).cloned().unwrap_or((String::new(), DEFAULT_GROUP_MASK));
        m.groups.push(Group { number: g, name, mask });
    }
    p.mixer = m;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str) -> Option<crate::Project> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata-private/sample-files").join(name);
        dir.exists().then(|| crate::open(&dir).expect("opens"))
    }

    #[test]
    fn fader_scale() {
        assert_eq!(fader_to_db(90.0), 0.0);
        assert!((fader_to_db(63.0) + 6.2).abs() < 0.3);
        assert!((fader_to_db(127.0) - 6.0).abs() < 0.01);
        assert!((db_to_fader(fader_to_db(41.0)) - 41.0).abs() < 0.01);
    }

    #[test]
    fn reads_a_send_and_its_aux() {
        let Some(p) = sample("E07-send-bus1.logicx") else { return };
        let m = &p.mixer;
        let auxes = m.auxes();
        assert_eq!(auxes.len(), 1);
        let aux = &m.strips[auxes[0]];
        assert_eq!(aux.input_bus, Some(0), "Aux 1 takes Bus 1");
        assert_eq!(m.sends.len(), 1);
        let s = m.sends[0];
        assert_eq!(s.bus, 0);
        assert_eq!(m.strips[s.from].kind, StripKind::Instrument);
        // the sending strip is the instrument track's
        let t = m.track_strip.iter().position(|x| *x == Some(s.from));
        assert!(t.is_some());
        assert_eq!(s.level_db, 0.0);
    }

    #[test]
    fn reads_a_summing_stack_output() {
        let Some(p) = sample("E10-summing-stack.logicx") else { return };
        let m = &p.mixer;
        let names: Vec<&str> = m.auxes().iter().map(|&a| m.strips[a].name.as_str()).collect();
        assert_eq!(names, ["Aux 1", "Sum 2"]);
        let inst: Vec<&Strip> = m.strips.iter().filter(|s| s.kind == StripKind::Instrument).collect();
        assert!(inst.len() >= 2);
        assert!(inst.iter().all(|s| s.output_bus == Some(1)), "both stack members go to Bus 2");
    }

    #[test]
    fn reads_drum_machine_sends_and_aux_chain() {
        let Some(p) = sample("E22-drum-machine-designer.logicx") else { return };
        let m = &p.mixer;
        // the pads send to the kit's reverb (Bus 4), some also to its delay (Bus 5)
        let to = |b| m.sends.iter().filter(|s| s.bus == b).count();
        assert!(to(3) >= 20, "{:?}", m.sends);
        assert!(to(4) >= 1, "{:?}", m.sends);
        assert_eq!(to(3) + to(4), m.sends.len());
        // Aux 4 takes Bus 4 and returns into the kit's sum on Bus 3
        let a4 = m.strips.iter().find(|s| s.kind == StripKind::Aux && s.number == 3).unwrap();
        assert_eq!((a4.input_bus, a4.output_bus), (Some(3), Some(2)));
    }

    #[test]
    fn reads_volume_mute_and_groups() {
        if let Some(p) = sample("E23-live-loops-base.logicx") {
            let s = p.mixer.strips.iter().find(|s| s.kind == StripKind::Instrument && s.number == 0).unwrap();
            assert!(s.mute);
            assert!((s.volume_db - fader_to_db(41.0)).abs() < 1e-4);
        }
        if let Some(p) = sample("E24-groups.logicx") {
            let m = &p.mixer;
            let g: Vec<u8> = m.track_group.clone();
            assert!(g.contains(&1) && g.contains(&2), "{g:?}");
            assert_eq!(m.groups.len(), 2);
            assert_eq!(m.groups[0].mask, [0x0f, 0x81, 0xb7, 0x84]);
            assert_eq!(m.groups[1].mask, DEFAULT_GROUP_MASK);
        }
    }
}
