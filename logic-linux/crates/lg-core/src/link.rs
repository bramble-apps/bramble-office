//! Ableton Link's wire format and timeline maths (Studio's own implementation of the open
//! protocol): peers announce themselves by UDP multicast (224.76.78.75:20808) with their node
//! and session ids, timeline (tempo, beat origin, time origin in the session's "ghost" time),
//! start/stop state and measurement endpoint; a joining peer measures a session member's ghost
//! time over unicast ping / pong and takes on the session's timeline. All values big-endian;
//! times in microseconds, beats in micro-beats, tempo as microseconds per beat.

pub const MULTICAST: [u8; 4] = [224, 76, 78, 75];
pub const PORT: u16 = 20808;
const DISCOVERY: &[u8; 8] = b"_asdp_v\x01";
const MEASURE: &[u8; 8] = b"_link_v\x01";

pub const ALIVE: u8 = 1;
pub const RESPONSE: u8 = 2;
pub const BYEBYE: u8 = 3;
pub const PING: u8 = 1;
pub const PONG: u8 = 2;

pub type NodeId = [u8; 8];

/// A session timeline: beats are `beat_origin + (t - time_origin) / tempo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeline {
    pub tempo_us: i64,
    pub beat_origin: i64,
    pub time_origin: i64,
}

impl Timeline {
    pub fn from_bpm(bpm: f64, beat: f64, at_us: i64) -> Timeline {
        Timeline { tempo_us: (60_000_000.0 / bpm.max(1.0)).round() as i64, beat_origin: (beat * 1e6).round() as i64, time_origin: at_us }
    }
    pub fn bpm(&self) -> f64 {
        60_000_000.0 / self.tempo_us.max(1) as f64
    }
    /// The beat at a (ghost) time.
    pub fn beat_at(&self, t_us: i64) -> f64 {
        self.beat_origin as f64 / 1e6 + (t_us - self.time_origin) as f64 / self.tempo_us.max(1) as f64
    }
    /// The (ghost) time of a beat.
    pub fn time_at(&self, beat: f64) -> i64 {
        self.time_origin + ((beat - self.beat_origin as f64 / 1e6) * self.tempo_us as f64).round() as i64
    }
    /// A new tempo from a time on, keeping the beat there.
    pub fn retempo(&self, bpm: f64, at_us: i64) -> Timeline {
        Timeline::from_bpm(bpm, self.beat_at(at_us), at_us)
    }
}

/// The phase within a quantum (Logic: one bar) at a beat, 0 ≤ phase < quantum.
pub fn phase(beat: f64, quantum: f64) -> f64 {
    beat.rem_euclid(quantum.max(1e-9))
}

/// Start / stop state (for apps that sync start and stop).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartStop {
    pub playing: bool,
    pub beats: i64,
    pub time: i64,
}

/// What a peer announces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerState {
    pub node: NodeId,
    pub session: NodeId,
    pub timeline: Timeline,
    pub start_stop: Option<StartStop>,
    pub endpoint: Option<([u8; 4], u16)>,
}

fn entry(out: &mut Vec<u8>, key: &[u8; 4], val: &[u8]) {
    out.extend_from_slice(key);
    out.extend_from_slice(&(val.len() as u32).to_be_bytes());
    out.extend_from_slice(val);
}

fn entries(mut b: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut v = Vec::new();
    while b.len() >= 8 {
        let key = [b[0], b[1], b[2], b[3]];
        let n = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
        if b.len() < 8 + n {
            break;
        }
        v.push((key, &b[8..8 + n]));
        b = &b[8 + n..];
    }
    v
}

fn i64_at(b: &[u8], k: usize) -> i64 {
    b.get(k..k + 8).map_or(0, |x| i64::from_be_bytes(x.try_into().unwrap()))
}

/// A discovery message (Alive, Response or ByeBye) carrying a peer's state.
pub fn encode(kind: u8, ttl: u8, s: &PeerState) -> Vec<u8> {
    let mut v = DISCOVERY.to_vec();
    v.push(kind);
    v.push(ttl);
    v.extend_from_slice(&0u16.to_be_bytes());
    v.extend_from_slice(&s.node);
    if kind == BYEBYE {
        return v;
    }
    let mut t = Vec::new();
    t.extend_from_slice(&s.timeline.tempo_us.to_be_bytes());
    t.extend_from_slice(&s.timeline.beat_origin.to_be_bytes());
    t.extend_from_slice(&s.timeline.time_origin.to_be_bytes());
    entry(&mut v, b"tmln", &t);
    entry(&mut v, b"sess", &s.session);
    if let Some(ss) = s.start_stop {
        let mut t = vec![ss.playing as u8];
        t.extend_from_slice(&ss.beats.to_be_bytes());
        t.extend_from_slice(&ss.time.to_be_bytes());
        entry(&mut v, b"stst", &t);
    }
    if let Some((ip, port)) = s.endpoint {
        let mut t = ip.to_vec();
        t.extend_from_slice(&port.to_be_bytes());
        entry(&mut v, b"mep4", &t);
    }
    v
}

/// A discovery message: (kind, ttl, state; ByeBye carries only the node).
pub fn decode(b: &[u8]) -> Option<(u8, u8, PeerState)> {
    if b.len() < 20 || &b[..8] != DISCOVERY {
        return None;
    }
    let (kind, ttl) = (b[8], b[9]);
    let node: NodeId = b[12..20].try_into().ok()?;
    let mut s = PeerState { node, session: node, timeline: Timeline { tempo_us: 500_000, beat_origin: 0, time_origin: 0 }, start_stop: None, endpoint: None };
    for (k, v) in entries(&b[20..]) {
        match &k {
            b"tmln" if v.len() >= 24 => s.timeline = Timeline { tempo_us: i64_at(v, 0), beat_origin: i64_at(v, 8), time_origin: i64_at(v, 16) },
            b"sess" if v.len() >= 8 => s.session = v[..8].try_into().ok()?,
            b"stst" if v.len() >= 17 => s.start_stop = Some(StartStop { playing: v[0] != 0, beats: i64_at(v, 1), time: i64_at(v, 9) }),
            b"mep4" if v.len() >= 6 => s.endpoint = Some(([v[0], v[1], v[2], v[3]], u16::from_be_bytes([v[4], v[5]]))),
            _ => {}
        }
    }
    Some((kind, ttl, s))
}

/// A measurement ping: our host time.
pub fn ping(host_us: i64) -> Vec<u8> {
    let mut v = MEASURE.to_vec();
    v.push(PING);
    entry(&mut v, b"__ht", &host_us.to_be_bytes());
    v
}

/// The pong for a ping: our ghost time, and the ping's payload echoed.
pub fn pong(ping_msg: &[u8], ghost_us: i64) -> Option<Vec<u8>> {
    if ping_msg.len() < 9 || &ping_msg[..8] != MEASURE || ping_msg[8] != PING {
        return None;
    }
    let mut v = MEASURE.to_vec();
    v.push(PONG);
    entry(&mut v, b"__gt", &ghost_us.to_be_bytes());
    v.extend_from_slice(&ping_msg[9..]);
    Some(v)
}

/// A pong's (their ghost time, our host time when we pinged).
pub fn read_pong(b: &[u8]) -> Option<(i64, i64)> {
    if b.len() < 9 || &b[..8] != MEASURE || b[8] != PONG {
        return None;
    }
    let (mut gt, mut ht) = (None, None);
    for (k, v) in entries(&b[9..]) {
        match &k {
            b"__gt" => gt = Some(i64_at(v, 0)),
            b"__ht" => ht = Some(i64_at(v, 0)),
            _ => {}
        }
    }
    Some((gt?, ht?))
}

/// The ghost-time offset (ghost = host + offset) from a ping sent at `sent`, answered with
/// their ghost time `gt`, received at `recv` (the round trip's middle).
pub fn offset(sent: i64, gt: i64, recv: i64) -> i64 {
    gt - (sent + recv) / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let s = PeerState { node: *b"abcdefgh", session: *b"sessionX", timeline: Timeline::from_bpm(120.0, 4.0, 1_000_000), start_stop: Some(StartStop { playing: true, beats: 8_000_000, time: 2_000_000 }), endpoint: Some(([192, 168, 1, 5], 40000)) };
        let m = encode(ALIVE, 5, &s);
        assert_eq!(&m[..8], b"_asdp_v\x01");
        assert_eq!(decode(&m), Some((ALIVE, 5, s.clone())));
        let bye = encode(BYEBYE, 0, &s);
        assert_eq!(decode(&bye).unwrap().0, BYEBYE);
        let p = ping(1234);
        let q = pong(&p, 999_000).unwrap();
        assert_eq!(read_pong(&q), Some((999_000, 1234)));
        assert_eq!(offset(1000, 51_000, 1200), 49_900);
    }

    #[test]
    fn timeline_maths() {
        let t = Timeline::from_bpm(120.0, 0.0, 0);
        assert_eq!(t.tempo_us, 500_000);
        assert_eq!(t.beat_at(1_000_000), 2.0);
        assert_eq!(t.time_at(3.0), 1_500_000);
        let u = t.retempo(60.0, 1_000_000);
        assert_eq!(u.beat_at(2_000_000), 3.0);
        assert!((u.bpm() - 60.0).abs() < 1e-9);
        assert_eq!(phase(9.5, 4.0), 1.5);
        assert_eq!(phase(-0.5, 4.0), 3.5);
    }
}
