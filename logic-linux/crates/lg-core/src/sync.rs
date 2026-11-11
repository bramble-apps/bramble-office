//! Synchronization over MIDI, as Logic does it: MIDI Time Code (quarter frames and full-frame
//! messages, sent and received, with Logic's frame-rate interpretation), MIDI Machine Control
//! (transport, locate and record-enable commands, sent and received) and MIDI Clock with Song
//! Position Pointer (sent only: Logic can't receive MIDI Clock).

use crate::events::PPQ;

/// A frame rate: frames per second and drop-frame counting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rate {
    pub fps: f64,
    pub drop: bool,
}

impl Rate {
    /// From Logic's project setting text ("25", "29.97d", …).
    pub fn parse(s: &str) -> Rate {
        Rate { fps: s.trim_end_matches('d').parse().unwrap_or(25.0), drop: s.ends_with('d') }
    }

    pub fn name(&self) -> String {
        let n = if self.fps.fract() == 0.0 { format!("{:.0}", self.fps) } else { format!("{}", self.fps) };
        if self.drop { format!("{n}d") } else { n }
    }

    /// The MTC rate code (0 = 24, 1 = 25, 2 = 30 drop, 3 = 30): the MIDI spec has no 29.97, so
    /// 29.97 is sent as 30 and 29.97 drop as 30 drop.
    pub fn mtc_code(&self) -> u8 {
        match (self.fps.round() as i32, self.drop) {
            (24, _) | (23, _) => 0,
            (25, _) => 1,
            (30, true) | (29, true) => 2,
            _ => 3,
        }
    }

    /// How Logic interprets an incoming MTC rate code: 24 (23.976 too) as 24, 25 as 25, 30 drop as
    /// 29.97 drop, 30 as 29.97.
    pub fn from_mtc_code(code: u8) -> Rate {
        match code & 3 {
            0 => Rate { fps: 24.0, drop: false },
            1 => Rate { fps: 25.0, drop: false },
            2 => Rate { fps: 29.97, drop: true },
            _ => Rate { fps: 29.97, drop: false },
        }
    }

    /// Whole frames per second as counted in the time code (30 for 29.97).
    pub fn nominal(&self) -> u32 {
        self.fps.round() as u32
    }
}

/// A time code: hours, minutes, seconds, frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Tc {
    pub h: u8,
    pub m: u8,
    pub s: u8,
    pub f: u8,
}

impl Tc {
    /// The time code of a moment (seconds), counting frames as the rate does (drop-frame skips
    /// frames 0 and 1 of every minute except each tenth).
    pub fn from_secs(secs: f64, r: Rate) -> Tc {
        let secs = secs.max(0.0);
        let nom = r.nominal() as u64;
        let mut frames = (secs * r.fps + 1e-6).floor() as u64;
        if r.drop {
            // (frames dropped per minute: 2 at 30, 4 at 60)
            let d = (nom / 15).max(2);
            let per10 = (nom * 600) - 9 * d;
            let per1 = nom * 60 - d;
            let tens = frames / per10;
            let rem = frames % per10;
            frames += 9 * d * tens + if rem > d { d * ((rem - d) / per1) } else { 0 };
        }
        let f = (frames % nom) as u8;
        let total_s = frames / nom;
        Tc { h: ((total_s / 3600) % 24) as u8, m: ((total_s / 60) % 60) as u8, s: (total_s % 60) as u8, f }
    }

    /// Back to seconds.
    pub fn to_secs(&self, r: Rate) -> f64 {
        let nom = r.nominal() as u64;
        let mins = self.h as u64 * 60 + self.m as u64;
        let mut frames = (mins * 60 + self.s as u64) * nom + self.f as u64;
        if r.drop {
            let d = (nom / 15).max(2);
            frames -= d * (mins - mins / 10);
        }
        frames as f64 / r.fps
    }

    pub fn text(&self, drop: bool) -> String {
        format!("{:02}:{:02}:{:02}{}{:02}", self.h, self.m, self.s, if drop { ';' } else { ':' }, self.f)
    }
}

/// The MTC full-frame message (sent on locate): F0 7F 7F 01 01 hh mm ss ff F7.
pub fn mtc_full(tc: Tc, r: Rate) -> Vec<u8> {
    vec![0xF0, 0x7F, 0x7F, 0x01, 0x01, (r.mtc_code() << 5) | (tc.h & 31), tc.m, tc.s, tc.f, 0xF7]
}

/// The eight quarter-frame messages (F1 nd) describing a time code; piece n goes out every
/// quarter frame, so a whole time code takes two frames.
pub fn mtc_quarters(tc: Tc, r: Rate) -> [[u8; 2]; 8] {
    let v = [tc.f & 15, tc.f >> 4, tc.s & 15, tc.s >> 4, tc.m & 15, tc.m >> 4, tc.h & 15, ((tc.h >> 4) & 1) | (r.mtc_code() << 1)];
    std::array::from_fn(|n| [0xF1, ((n as u8) << 4) | v[n]])
}

/// Receives MTC: quarter frames and full frames give the time code and its rate.
#[derive(Clone, Debug, Default)]
pub struct MtcIn {
    pieces: [u8; 8],
    seen: u8,
    last_piece: Option<u8>,
    /// Going backwards (pieces in reverse order).
    pub reverse: bool,
}

impl MtcIn {
    /// One MIDI message in; Some((seconds, rate)) when a time code is complete.
    pub fn feed(&mut self, msg: &[u8]) -> Option<(f64, Rate)> {
        match msg {
            [0xF1, d, ..] => {
                let n = d >> 4;
                if n > 7 {
                    return None;
                }
                if let Some(l) = self.last_piece {
                    self.reverse = n + 1 == l || (l == 0 && n == 7);
                }
                self.last_piece = Some(n);
                self.pieces[n as usize] = d & 15;
                self.seen |= 1 << n;
                // complete after piece 7 (forwards) or 0 (backwards), once all eight arrived
                if self.seen == 0xFF && ((n == 7 && !self.reverse) || (n == 0 && self.reverse)) {
                    let p = self.pieces;
                    let code = (p[7] >> 1) & 3;
                    let r = Rate::from_mtc_code(code);
                    let tc = Tc { f: p[0] | (p[1] << 4), s: p[2] | (p[3] << 4), m: p[4] | (p[5] << 4), h: p[6] | ((p[7] & 1) << 4) };
                    // (the code was sent over two frames: it is two frames old now)
                    let secs = tc.to_secs(r) + if self.reverse { 0.0 } else { 2.0 / r.fps };
                    return Some((secs, r));
                }
                None
            }
            [0xF0, 0x7F, _, 0x01, 0x01, hr, m, s, f, ..] => {
                let r = Rate::from_mtc_code(hr >> 5);
                let tc = Tc { h: hr & 31, m: *m, s: *s, f: *f };
                self.seen = 0;
                Some((tc.to_secs(r), r))
            }
            _ => None,
        }
    }
}

/// MMC commands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mmc {
    Stop,
    Play,
    DeferredPlay,
    FastForward,
    Rewind,
    RecordStrobe,
    RecordExit,
    Pause,
    /// Locate to a time code (seconds at the rate given).
    Locate(Tc),
    /// Track record-ready bits (bit 0 = track 1).
    Arm(u64),
}

/// An MMC message for device `id` (127 = all).
pub fn mmc(id: u8, cmd: Mmc, r: Rate) -> Vec<u8> {
    let mut v = vec![0xF0, 0x7F, id & 127, 0x06];
    match cmd {
        Mmc::Stop => v.push(0x01),
        Mmc::Play => v.push(0x02),
        Mmc::DeferredPlay => v.push(0x03),
        Mmc::FastForward => v.push(0x04),
        Mmc::Rewind => v.push(0x05),
        Mmc::RecordStrobe => v.push(0x06),
        Mmc::RecordExit => v.push(0x07),
        Mmc::Pause => v.push(0x09),
        Mmc::Locate(tc) => v.extend([0x44, 0x06, 0x01, (r.mtc_code() << 5) | (tc.h & 31), tc.m, tc.s, tc.f, 0x00]),
        Mmc::Arm(bits) => {
            // WRITE Track Record Ready: the standard bitmap (byte 0: video, -, time code, aux A,
            // aux B, track 1, track 2; then seven tracks per byte)
            let mut map = vec![0u8; 1 + 64usize.div_ceil(7)];
            for t in 0..64u32 {
                if bits >> t & 1 == 1 {
                    let pos = t + 5;
                    map[(pos / 7) as usize] |= 1 << (pos % 7);
                }
            }
            while map.len() > 1 && *map.last().unwrap() == 0 {
                map.pop();
            }
            v.extend([0x40, map.len() as u8 + 2, 0x4F, map.len() as u8]);
            v.extend(map);
        }
    }
    v.push(0xF7);
    v
}

/// Reads an MMC message (for device `id`, or any when the message is for all, 127).
pub fn mmc_parse(msg: &[u8], id: Option<u8>) -> Option<Mmc> {
    if msg.len() < 6 || msg[0] != 0xF0 || msg[1] != 0x7F || msg[3] != 0x06 {
        return None;
    }
    if let Some(id) = id {
        if msg[2] != 127 && msg[2] != id {
            return None;
        }
    }
    Some(match msg[4] {
        0x01 => Mmc::Stop,
        0x02 => Mmc::Play,
        0x03 => Mmc::DeferredPlay,
        0x04 => Mmc::FastForward,
        0x05 => Mmc::Rewind,
        0x06 => Mmc::RecordStrobe,
        0x07 => Mmc::RecordExit,
        0x09 => Mmc::Pause,
        0x44 if msg.len() >= 12 => Mmc::Locate(Tc { h: msg[7] & 31, m: msg[8], s: msg[9], f: msg[10] }),
        0x40 if msg.len() >= 9 && msg[6] == 0x4F => {
            let n = msg[7] as usize;
            let mut bits = 0u64;
            for (k, b) in msg[8..(8 + n).min(msg.len() - 1)].iter().enumerate() {
                for bit in 0..7 {
                    let pos = k * 7 + bit;
                    if b >> bit & 1 == 1 && pos >= 5 && pos - 5 < 64 {
                        bits |= 1 << (pos - 5);
                    }
                }
            }
            Mmc::Arm(bits)
        }
        _ => return None,
    })
}

/// MIDI Clock: 24 clocks per quarter note.
pub const CLOCK_TICKS: f64 = PPQ as f64 / 24.0;

/// Song Position Pointer for a position in ticks (16th notes, rounded down).
pub fn spp(ticks: f64) -> [u8; 3] {
    let n = (ticks.max(0.0) / (PPQ as f64 / 4.0)).floor() as u32 & 0x3FFF;
    [0xF2, (n & 127) as u8, (n >> 7) as u8]
}

/// MIDI Clock sending: given the song position each poll, the clock bytes due (F8s, and Start /
/// Continue / Stop / SPP on transport changes).
#[derive(Clone, Debug, Default)]
pub struct ClockOut {
    playing: bool,
    /// The next clock's position (ticks).
    next: f64,
    last: f64,
}

impl ClockOut {
    /// `song`: ticks; `jumped`: a locate or cycle jump since the last poll. `song_mode`: send SPP
    /// (Song mode) rather than only Start (Pattern mode).
    pub fn poll(&mut self, playing: bool, song: f64, jumped: bool, song_mode: bool) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let jumped = jumped || (playing && (song < self.last - 1.0 || song > self.last + PPQ as f64 * 2.0));
        if playing && !self.playing {
            if song_mode {
                out.push(spp(song).to_vec());
                out.push(vec![if song <= 0.5 { 0xFA } else { 0xFB }]);
            } else {
                out.push(vec![0xFA]);
            }
            self.next = (song / CLOCK_TICKS).ceil() * CLOCK_TICKS;
        } else if !playing && self.playing {
            out.push(vec![0xFC]);
        } else if playing && jumped && song_mode {
            // (a cycle jump: stop, the new position, continue)
            out.push(vec![0xFC]);
            out.push(spp(song).to_vec());
            out.push(vec![0xFB]);
            self.next = (song / CLOCK_TICKS).ceil() * CLOCK_TICKS;
        }
        if playing {
            let mut n = 0;
            while self.next <= song && n < 96 {
                out.push(vec![0xF8]);
                self.next += CLOCK_TICKS;
                n += 1;
            }
        }
        self.playing = playing;
        self.last = song;
        out
    }
}

/// MTC sending: quarter frames as time passes, a full frame on locate.
#[derive(Clone, Debug, Default)]
pub struct MtcOut {
    playing: bool,
    /// The next quarter frame's time (seconds) and its piece.
    next: f64,
    piece: u8,
    tc: Tc,
    last: f64,
}

impl MtcOut {
    pub fn poll(&mut self, playing: bool, secs: f64, r: Rate) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let qf = 1.0 / (r.fps * 4.0);
        let jumped = (secs - self.last).abs() > 0.25 && !(playing && self.playing && secs > self.last && secs - self.last < 0.25);
        if (playing && !self.playing) || (!playing && jumped) || (playing && jumped) {
            out.push(mtc_full(Tc::from_secs(secs, r), r));
            self.next = secs;
            self.piece = 0;
        }
        if playing {
            let mut n = 0;
            while self.next <= secs && n < 256 {
                if self.piece == 0 {
                    self.tc = Tc::from_secs(self.next, r);
                }
                let q = mtc_quarters(self.tc, r);
                out.push(q[self.piece as usize].to_vec());
                self.piece = (self.piece + 1) % 8;
                self.next += qf;
                n += 1;
            }
        }
        self.playing = playing;
        self.last = secs;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timecode_round_trip() {
        let r = Rate::parse("25");
        let tc = Tc::from_secs(3661.52, r);
        assert_eq!(tc, Tc { h: 1, m: 1, s: 1, f: 13 });
        assert!((tc.to_secs(r) - 3661.52).abs() < 0.001);
        // drop frame: 00:01:00;02 follows 00:00:59;29
        let d = Rate::parse("29.97d");
        let a = Tc { h: 0, m: 0, s: 59, f: 29 }.to_secs(d);
        let b = Tc::from_secs(a + 1.0 / 29.97, d);
        assert_eq!(b, Tc { h: 0, m: 1, s: 0, f: 2 });
        for k in [0.0, 12.3, 600.0, 3599.9] {
            let tc = Tc::from_secs(k, d);
            assert!((tc.to_secs(d) - k).abs() < 0.04, "{k}");
        }
    }

    #[test]
    fn mtc_interpretation() {
        // 30 → 29.97; 30 drop → 29.97 drop; 24 → 24; 25 → 25
        assert_eq!(Rate::from_mtc_code(3), Rate { fps: 29.97, drop: false });
        assert_eq!(Rate::from_mtc_code(2), Rate { fps: 29.97, drop: true });
        assert_eq!(Rate::from_mtc_code(0).fps, 24.0);
        assert_eq!(Rate::parse("23.976").mtc_code(), 0);
        assert_eq!(Rate::from_mtc_code(1).fps, 25.0);
    }

    #[test]
    fn mtc_quarter_frames_decode() {
        let r = Rate::parse("25");
        let tc = Tc { h: 1, m: 2, s: 3, f: 4 };
        let mut d = MtcIn::default();
        let mut got = None;
        for q in mtc_quarters(tc, r) {
            got = got.or(d.feed(&q));
        }
        let (secs, rate) = got.unwrap();
        assert_eq!(rate.fps, 25.0);
        assert!((secs - (tc.to_secs(r) + 2.0 / 25.0)).abs() < 1e-6);
        let (s2, _) = d.feed(&mtc_full(tc, r)).unwrap();
        assert!((s2 - tc.to_secs(r)).abs() < 1e-6);
    }

    #[test]
    fn mmc_messages() {
        let r = Rate::parse("25");
        assert_eq!(mmc(127, Mmc::Play, r), vec![0xF0, 0x7F, 0x7F, 0x06, 0x02, 0xF7]);
        let loc = mmc(127, Mmc::Locate(Tc { h: 1, m: 0, s: 10, f: 5 }), r);
        assert_eq!(mmc_parse(&loc, Some(3)), Some(Mmc::Locate(Tc { h: 1, m: 0, s: 10, f: 5 })));
        let arm = mmc(127, Mmc::Arm(0b1011), r);
        assert_eq!(mmc_parse(&arm, None), Some(Mmc::Arm(0b1011)));
        assert_eq!(mmc_parse(&mmc(5, Mmc::Stop, r), Some(6)), None);
    }

    #[test]
    fn clock_and_spp() {
        let mut c = ClockOut::default();
        let out = c.poll(true, 960.0, false, true);
        assert_eq!(out[0], spp(960.0).to_vec());
        assert_eq!(spp(960.0), [0xF2, 4, 0]);
        assert_eq!(out[1], vec![0xFB]);
        // one quarter note later: 24 clocks
        let out = c.poll(true, 1920.0, false, true);
        assert_eq!(out.iter().filter(|m| m[0] == 0xF8).count(), 24);
        assert_eq!(c.poll(false, 1920.0, false, true), vec![vec![0xFC]]);
        let mut p = ClockOut::default();
        assert_eq!(p.poll(true, 0.0, false, false)[0], vec![0xFA]);
    }

    #[test]
    fn mtc_out_runs() {
        let r = Rate::parse("25");
        let mut m = MtcOut::default();
        let out = m.poll(true, 10.0, r);
        assert_eq!(out[0][0], 0xF0);
        // a second later, polled every 10 ms: 100 quarter frames
        let mut out = Vec::new();
        for k in 1..=100 {
            out.extend(m.poll(true, 10.0 + k as f64 * 0.01, r));
        }
        assert!(out.iter().filter(|x| x[0] == 0xF1).count() >= 60);
        let mut d = MtcIn::default();
        let mut last = None;
        for x in &out {
            if let Some(v) = d.feed(x) {
                last = Some(v);
            }
        }
        assert!((last.unwrap().0 - 11.0).abs() < 0.2);
    }
}
