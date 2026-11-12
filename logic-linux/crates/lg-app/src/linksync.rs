//! Ableton Link (lg_core::link has the protocol): choosing Ableton Link in the Sync button's
//! shortcut menu joins a Link session on the local network (or this computer). Studio takes on
//! the session's tempo (the Tempo track is deactivated while Link tempo differs; Tempo ▸
//! Reactivate Tempo Track brings it back and sends its tempo to the peers), a tempo change here
//! or by any peer becomes everyone's, Play waits for the bar's phase so it starts in sync, and
//! start / stop is shared with apps that sync it. The Sync button shows the phase within the bar
//! as a circular progress bar with the number of other participants in the middle; clicking it
//! leaves the session without disturbing the others.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use lg_core::link::{self as lk, NodeId, PeerState, StartStop, Timeline};

use crate::app::App;

pub const COMMANDS: &[&str] = &["Reactivate Tempo Track", "Deactivate Tempo Track"];

#[derive(Default)]
pub struct State {
    pub on: bool,
    multi: Option<UdpSocket>,
    uni: Option<UdpSocket>,
    node: NodeId,
    session: NodeId,
    timeline: Option<Timeline>,
    /// ghost time = host time + offset
    ghost_off: i64,
    pub peers: BTreeMap<NodeId, (PeerState, Instant)>,
    last_alive: Option<Instant>,
    pinging: Option<(NodeId, i64)>,
    joined: bool,
    applied_bpm: f64,
    /// The Tempo track is deactivated (Link tempo rules); its map kept to bring it back.
    pub tempo_off: Option<Vec<(u32, f64)>>,
    start_stop: Option<StartStop>,
    last_playing: bool,
    /// Play pressed: starting when the phase reaches the bar line (ghost time).
    pub waiting: Option<i64>,
}

/// Microseconds on the system's monotonic clock (shared by Link apps on this computer).
pub fn host_us() -> i64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec * 1_000_000 + ts.tv_nsec / 1000
}

/// The multicast socket, bound to Link's port with address reuse (other Link apps share it).
fn multicast_socket() -> Option<UdpSocket> {
    use std::os::fd::FromRawFd;
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if fd < 0 {
            return None;
        }
        let one: libc::c_int = 1;
        let sz = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_REUSEADDR, &one as *const _ as *const libc::c_void, sz);
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_REUSEPORT, &one as *const _ as *const libc::c_void, sz);
        let addr = libc::sockaddr_in { sin_family: libc::AF_INET as libc::sa_family_t, sin_port: lk::PORT.to_be(), sin_addr: libc::in_addr { s_addr: 0 }, sin_zero: [0; 8] };
        if libc::bind(fd, &addr as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t) != 0 {
            libc::close(fd);
            return None;
        }
        let s = UdpSocket::from_raw_fd(fd);
        let g = Ipv4Addr::from(lk::MULTICAST);
        s.join_multicast_v4(&g, &Ipv4Addr::UNSPECIFIED).ok()?;
        let _ = s.set_multicast_loop_v4(true);
        s.set_nonblocking(true).ok()?;
        Some(s)
    }
}

fn group() -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::from(lk::MULTICAST), lk::PORT)
}

/// The address peers reach us at (the interface a multicast route would use).
fn local_ip() -> [u8; 4] {
    UdpSocket::bind("0.0.0.0:0").ok().and_then(|s| s.connect(group()).ok().and_then(|_| s.local_addr().ok())).and_then(|a| match a.ip() {
        std::net::IpAddr::V4(v) if !v.is_unspecified() => Some(v.octets()),
        _ => None,
    }).unwrap_or([127, 0, 0, 1])
}

fn random_id() -> NodeId {
    let t = host_us() as u64 ^ (std::process::id() as u64).rotate_left(32);
    let mut x = t.wrapping_mul(0x9E3779B97F4A7C15);
    let mut id = [0u8; 8];
    for b in id.iter_mut() {
        x ^= x >> 29;
        x = x.wrapping_mul(0xBF58476D1CE4E5B9);
        *b = b'a' + (x % 26) as u8;
    }
    id
}

/// Is Link the chosen, active sync source?
pub fn wanted(app: &App) -> bool {
    app.hub.sync.ext && app.proj.get("Synchronization / General / Sync Mode:") == "Ableton Link"
}

fn ghost_now(app: &App) -> i64 {
    host_us() + app.hub.link.ghost_off
}

fn quantum(app: &App) -> f64 {
    app.project.as_ref().map_or(4.0, |p| p.signature.0 as f64 * 4.0 / p.signature.1.max(1) as f64)
}

fn state_of(app: &App) -> PeerState {
    let l = &app.hub.link;
    let ep = l.uni.as_ref().and_then(|u| u.local_addr().ok()).map(|a| (local_ip(), a.port()));
    PeerState { node: l.node, session: l.session, timeline: l.timeline.unwrap_or(Timeline::from_bpm(120.0, 0.0, 0)), start_stop: l.start_stop, endpoint: ep }
}

fn broadcast(app: &mut App, kind: u8) {
    let msg = lk::encode(kind, if kind == lk::BYEBYE { 0 } else { 5 }, &state_of(app));
    if let Some(u) = app.hub.link.uni.as_ref() {
        let _ = u.send_to(&msg, group());
    }
    app.hub.link.last_alive = Some(Instant::now());
}

fn join(app: &mut App) {
    let l = &mut app.hub.link;
    l.multi = multicast_socket();
    l.uni = UdpSocket::bind("0.0.0.0:0").ok().filter(|u| u.set_nonblocking(true).is_ok());
    l.node = random_id();
    l.session = l.node;
    l.ghost_off = -host_us();
    l.peers.clear();
    l.joined = false;
    l.on = true;
    // a session of our own until one is found: our tempo from now
    let bpm = app.tempo();
    let g = ghost_now(app);
    let beat = app.position() / lg_core::events::PPQ as f64;
    app.hub.link.timeline = Some(Timeline::from_bpm(bpm, beat, g));
    app.hub.link.applied_bpm = bpm;
    if app.hub.link.multi.is_none() {
        app.tell("Ableton Link: the network port is not available");
    }
    broadcast(app, lk::ALIVE);
}

/// Leaves the session (ByeBye), the Tempo track active again.
pub fn leave(app: &mut App) {
    if !app.hub.link.on {
        return;
    }
    broadcast(app, lk::BYEBYE);
    reactivate(app);
    let l = &mut app.hub.link;
    l.on = false;
    l.multi = None;
    l.uni = None;
    l.peers.clear();
    l.waiting = None;
}

/// Link tempo here: the project plays at it, the Tempo track deactivated (kept to bring back).
fn take_tempo(app: &mut App, bpm: f64) {
    if (app.tempo() - bpm).abs() < 0.005 {
        app.hub.link.applied_bpm = bpm;
        return;
    }
    let dirty = app.dirty;
    if app.hub.link.tempo_off.is_none() {
        app.hub.link.tempo_off = app.project.as_ref().map(|p| p.tempo_map.clone());
    }
    app.set_tempo(bpm);
    app.dirty = dirty;
    app.hub.link.applied_bpm = app.tempo();
}

/// Tempo ▸ Reactivate Tempo Track: the project's tempo map again (sent to the peers as it plays).
pub fn reactivate(app: &mut App) {
    let Some(map) = app.hub.link.tempo_off.take() else { return };
    let dirty = app.dirty;
    if let Some(p) = app.project.as_mut() {
        p.tempo = map.first().map_or(p.tempo, |x| x.1);
        p.tempo_map = map.clone();
    }
    if let Ok(mut e) = app.engine.lock() {
        e.tempo = map.first().map_or(e.tempo, |x| x.1);
        e.tempo_map = map;
    }
    app.dirty = dirty;
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Reactivate Tempo Track" => reactivate(app),
        "Deactivate Tempo Track" => {
            if app.hub.link.tempo_off.is_none() {
                let bpm = app.tempo();
                app.hub.link.tempo_off = app.project.as_ref().map(|p| p.tempo_map.clone());
                let dirty = app.dirty;
                if let Some(p) = app.project.as_mut() {
                    p.tempo_map = vec![(0, bpm)];
                    p.tempo = bpm;
                }
                app.dirty = dirty;
            }
        }
        _ => return false,
    }
    true
}

pub fn checked(app: &App, label: &str) -> Option<bool> {
    match label {
        "Deactivate Tempo Track" => Some(app.hub.link.tempo_off.is_some()),
        _ => None,
    }
}

/// Each frame: discovery, measurement, tempo and start / stop.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    let want = wanted(app);
    if want && !app.hub.link.on {
        join(app);
    } else if !want && app.hub.link.on {
        leave(app);
    }
    if !app.hub.link.on {
        return;
    }
    ctx.request_repaint_after(Duration::from_millis(30));
    let mut buf = [0u8; 1500];
    // discovery
    let mut msgs = Vec::new();
    if let Some(m) = app.hub.link.multi.as_ref() {
        while let Ok((n, _)) = m.recv_from(&mut buf) {
            msgs.push(buf[..n].to_vec());
        }
    }
    for m in msgs {
        let Some((kind, ttl, s)) = lk::decode(&m) else { continue };
        if s.node == app.hub.link.node {
            continue;
        }
        if kind == lk::BYEBYE {
            app.hub.link.peers.remove(&s.node);
            continue;
        }
        let life = Duration::from_secs(ttl.max(1) as u64);
        app.hub.link.peers.insert(s.node, (s, Instant::now() + life));
    }
    app.hub.link.peers.retain(|_, (_, until)| *until > Instant::now());
    // measurement: pings in (answered with our ghost time), pongs back
    let mut uni_in = Vec::new();
    if let Some(u) = app.hub.link.uni.as_ref() {
        while let Ok((n, from)) = u.recv_from(&mut buf) {
            uni_in.push((buf[..n].to_vec(), from));
        }
    }
    for (m, from) in uni_in {
        if let Some(reply) = lk::pong(&m, ghost_now(app)) {
            if let Some(u) = app.hub.link.uni.as_ref() {
                let _ = u.send_to(&reply, from);
            }
        } else if let Some((gt, sent)) = lk::read_pong(&m) {
            if let Some((node, _)) = app.hub.link.pinging.take() {
                // join that peer's session: its ghost time and timeline
                if let Some((p, _)) = app.hub.link.peers.get(&node).cloned() {
                    app.hub.link.ghost_off = lk::offset(sent, gt, host_us());
                    app.hub.link.session = p.session;
                    app.hub.link.timeline = Some(p.timeline);
                    app.hub.link.joined = true;
                    take_tempo(app, p.timeline.bpm());
                    broadcast(app, lk::ALIVE);
                }
            }
        }
    }
    // a session found that isn't ours (we haven't joined one): measure a member to join it
    if !app.hub.link.joined && app.hub.link.pinging.is_none() {
        let mine = app.hub.link.session;
        if let Some((node, ep)) = app.hub.link.peers.values().find(|(p, _)| p.session != mine).and_then(|(p, _)| p.endpoint.map(|e| (p.node, e))) {
            let sent = host_us();
            if let Some(u) = app.hub.link.uni.as_ref() {
                let _ = u.send_to(&lk::ping(sent), SocketAddrV4::new(Ipv4Addr::from(ep.0), ep.1));
            }
            app.hub.link.pinging = Some((node, sent));
        }
    }
    // a peer in our session changed the timeline (tempo): everyone's tempo
    let ours = app.hub.link.timeline;
    let session = app.hub.link.session;
    let changed = app.hub.link.peers.values().find(|(p, _)| p.session == session && Some(p.timeline) != ours && (p.timeline.bpm() - ours.map_or(0.0, |t| t.bpm())).abs() > 0.004).map(|(p, _)| p.timeline);
    if let Some(t) = changed {
        app.hub.link.timeline = Some(t);
        take_tempo(app, t.bpm());
    } else if (app.tempo() - app.hub.link.applied_bpm).abs() > 0.004 {
        // a tempo change here (the LCD, or the reactivated Tempo track playing): to the peers
        let g = ghost_now(app);
        let bpm = app.tempo();
        app.hub.link.timeline = Some(app.hub.link.timeline.map_or(Timeline::from_bpm(bpm, 0.0, g), |t| t.retempo(bpm, g)));
        app.hub.link.applied_bpm = bpm;
        broadcast(app, lk::ALIVE);
    }
    // start / stop sync
    let playing = app.playing();
    let peer_ss = app.hub.link.peers.values().filter(|(p, _)| p.session == session).filter_map(|(p, _)| p.start_stop).max_by_key(|s| s.time);
    if let Some(ss) = peer_ss {
        if app.hub.link.start_stop.is_none_or(|m| ss.time > m.time) {
            app.hub.link.start_stop = Some(ss);
            if ss.playing != playing && app.hub.link.waiting.is_none() {
                app.play_stop();
            }
        }
    }
    let playing = app.playing();
    if playing != app.hub.link.last_playing {
        // Play: wait for the phase to come round to the bar line, then start in sync
        if playing && app.hub.link.waiting.is_none() && !app.hub.link.peers.is_empty() {
            if let Some(t) = app.hub.link.timeline {
                let q = quantum(app);
                let g = ghost_now(app);
                let b = t.beat_at(g);
                let next = (b / q).floor() * q + q;
                app.halt();
                app.hub.link.waiting = Some(t.time_at(next));
            }
        } else {
            let g = ghost_now(app);
            let beats = app.hub.link.timeline.map_or(0, |t| (t.beat_at(g) * 1e6) as i64);
            app.hub.link.start_stop = Some(StartStop { playing, beats, time: g });
            broadcast(app, lk::ALIVE);
        }
    }
    if let Some(at) = app.hub.link.waiting {
        if ghost_now(app) >= at {
            app.hub.link.waiting = None;
            app.play_stop();
            let g = ghost_now(app);
            let beats = app.hub.link.timeline.map_or(0, |t| (t.beat_at(g) * 1e6) as i64);
            app.hub.link.start_stop = Some(StartStop { playing: true, beats, time: g });
            broadcast(app, lk::ALIVE);
        }
    }
    app.hub.link.last_playing = app.playing();
    if app.hub.link.last_alive.is_none_or(|t| t.elapsed() > Duration::from_millis(250)) {
        broadcast(app, lk::ALIVE);
    }
}

/// The phase within the bar (0–1) and the number of other participants (the Sync button).
pub fn display(app: &App) -> Option<(f32, usize)> {
    if !app.hub.link.on {
        return None;
    }
    let t = app.hub.link.timeline?;
    let q = quantum(app);
    Some(((lk::phase(t.beat_at(ghost_now(app)), q) / q) as f32, app.hub.link.peers.len()))
}

/// Draws the Link progress circle with the participant count (in the Sync button).
pub fn paint(app: &App, p: &egui::Painter, r: egui::Rect) -> bool {
    let Some((ph, n)) = display(app) else { return false };
    let c = r.center();
    let rad = r.height().min(r.width()) * 0.5 - 2.0;
    p.circle_stroke(c, rad, egui::Stroke::new(1.5, crate::look::text_dim().gamma_multiply(0.5)));
    let steps = 32;
    let pts: Vec<egui::Pos2> = (0..=((ph * steps as f32).ceil() as usize)).map(|k| {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * (k as f32 / steps as f32).min(ph);
        c + egui::vec2(a.cos(), a.sin()) * rad
    }).collect();
    if pts.len() > 1 {
        p.add(egui::Shape::line(pts, egui::Stroke::new(2.0, if app.hub.link.tempo_off.is_some() { egui::Color32::from_rgb(0x3a, 0x8e, 0xe6) } else { crate::look::accent() })));
    }
    p.text(c, egui::Align2::CENTER_CENTER, n.to_string(), crate::look::regular(9.5), crate::look::text());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deactivate_and_reactivate_tempo_track() {
        let mut app = crate::tm::test_app();
        if let Some(p) = app.project.as_mut() {
            p.tempo_map = vec![(0, 100.0), (3840, 140.0)];
            p.tempo = 100.0;
        }
        take_tempo(&mut app, 120.0);
        assert!(app.hub.link.tempo_off.is_some());
        assert_eq!(app.project.as_ref().unwrap().tempo_map, vec![(0, 120.0)]);
        assert_eq!(checked(&app, "Deactivate Tempo Track"), Some(true));
        assert!(command(&mut app, "Reactivate Tempo Track"));
        assert_eq!(app.project.as_ref().unwrap().tempo_map, vec![(0, 100.0), (3840, 140.0)]);
        assert!(host_us() > 0);
        assert_eq!(random_id().len(), 8);
    }
}
