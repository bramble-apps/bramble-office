//! Keydeck Remote (Keynote Remote): a phone or tablet running Keydeck controls the slideshow
//! played here, over the local network. Settings > Remotes enables it and lists the remotes
//! that are looking (Link shows a four-digit code on both devices, Confirm links them) and the
//! ones linked before (Unlink). A linked remote plays, moves and ends the show, sees the current
//! and next slides and the presenter notes, and draws (a laser pointer, coloured highlights)
//! over the slide as it plays here.
//!
//! The wire: remotes find Keydeck by a UDP broadcast to port 51737 ("KDRM?" and their id and
//! name as JSON), answered with {keydeck, id, name, port}; then one TCP connection, JSON lines
//! both ways (hello / welcome, code, linked, state, thumbs; cmd, laser, draw, undo, clear).
//!
//! A browser can be a remote too (remote_web.rs): a small web page served over HTTP, with a
//! secret key in its link instead of the code.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use egui::{Color32, Painter, Pos2, Rect, Stroke};
use serde_json::{Value, json};

use crate::app::App;

#[path = "remote_web.rs"]
mod web;

pub const DISCOVERY_PORT: u16 = 51737;
pub const PORT: u16 = 51738;

/// A remote linked before (remotes.json).
#[derive(Clone)]
struct Linked {
    id: String,
    name: String,
    token: String,
}

/// A remote connected now.
struct Conn {
    id: String,
    name: String,
    linked: bool,
    tx: Sender<String>,
}

/// What a linked remote asked of the show.
pub enum Cmd {
    Next,
    Prev,
    Goto(usize),
    Stop,
    Play,
}

/// A highlight drawn from a remote: its colour and its points (fractions of the slide).
struct Mark {
    id: u64,
    color: Color32,
    points: Vec<[f32; 2]>,
}

#[derive(Default)]
struct Shared {
    enabled: bool,
    id: String,
    name: String,
    port: u16,
    linked: Vec<Linked>,
    conns: HashMap<u64, Conn>,
    next_conn: u64,
    /// Link clicked: the connection and the code both show.
    pending: Option<(u64, String)>,
    cmds: Vec<Cmd>,
    laser: Option<([f32; 2], Instant)>,
    marks: Vec<Mark>,
    /// When the highlights last changed (the show redraws for a moment after).
    marked: Option<Instant>,
    /// What was sent last (playing, slide, step, revision), and a remote that wants it all again.
    sent: String,
    resend: bool,
    /// The last state and slide list sent: a remote that connects while the window isn't
    /// being drawn (no frames, so nothing is published) gets them at once.
    last_state: Option<String>,
    last_thumbs: Option<String>,
    thumbs_asked: bool,
    ctx: Option<egui::Context>,
    /// The web remote's secret (in its link) and its port (0: not started).
    web_key: String,
    web_port: u16,
}

fn shared() -> MutexGuard<'static, Shared> {
    static S: OnceLock<Mutex<Shared>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(load())).lock().unwrap_or_else(|e| e.into_inner())
}

fn file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("keydeck").join("remotes.json"))
}

/// A random hex string (no random crate: the std hasher's random keys and the clock).
fn random(n: usize) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    while out.len() < n {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        h.write_usize(out.len());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out.truncate(n);
    out
}

fn host_name() -> String {
    let n = std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    n.or_else(|| std::env::var("HOSTNAME").ok()).or_else(|| std::env::var("COMPUTERNAME").ok()).unwrap_or_else(|| "Keydeck".into())
}

fn load() -> Shared {
    let v: Value = file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    let linked = v["linked"]
        .as_array()
        .map(|a| a.iter().map(|l| Linked { id: l["id"].as_str().unwrap_or_default().into(), name: l["name"].as_str().unwrap_or_default().into(), token: l["token"].as_str().unwrap_or_default().into() }).collect())
        .unwrap_or_default();
    let web_key = v["web_key"].as_str().filter(|k| k.len() >= 16).map(str::to_string);
    let s = Shared {
        web_key: web_key.clone().unwrap_or_else(|| random(32)),
        enabled: v["enabled"].as_bool().unwrap_or(false) || std::env::var_os("KN_REMOTE_LINK").is_some(),
        id: v["id"].as_str().map(str::to_string).unwrap_or_else(|| random(16)),
        name: host_name(),
        linked,
        ..Default::default()
    };
    if web_key.is_none() {
        save(&s);
    }
    s
}

fn save(s: &Shared) {
    let Some(f) = file() else { return };
    let v = json!({
        "enabled": s.enabled,
        "id": s.id,
        "web_key": s.web_key,
        "linked": s.linked.iter().map(|l| json!({ "id": l.id, "name": l.name, "token": l.token })).collect::<Vec<_>>(),
    });
    let _ = std::fs::create_dir_all(f.parent().unwrap_or(std::path::Path::new(".")));
    let _ = std::fs::write(f, v.to_string());
}

fn send(c: &Conn, v: Value) {
    let _ = c.tx.send(v.to_string());
}

fn wake(s: &Shared) {
    if let Some(c) = &s.ctx {
        c.request_repaint();
    }
}

/// The listeners, started once (they answer only while Remotes is enabled).
fn start(ctx: &egui::Context) {
    static STARTED: OnceLock<()> = OnceLock::new();
    shared().ctx = Some(ctx.clone());
    STARTED.get_or_init(|| {
        // the control port (the next ones if it's taken)
        let listener = (0..10).find_map(|k| TcpListener::bind(("0.0.0.0", PORT + k)).ok());
        let Some(listener) = listener else { return };
        shared().port = listener.local_addr().map(|a| a.port()).unwrap_or(PORT);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || serve(stream));
            }
        });
        // discovery
        std::thread::spawn(|| {
            let Ok(sock) = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)) else { return };
            let _ = sock.set_broadcast(true);
            let mut buf = [0u8; 2048];
            while let Ok((n, from)) = sock.recv_from(&mut buf) {
                let msg = String::from_utf8_lossy(&buf[..n]).to_string();
                if !msg.starts_with("KDRM?") {
                    continue;
                }
                let reply = {
                    let s = shared();
                    if !s.enabled {
                        continue;
                    }
                    json!({ "keydeck": 1, "id": s.id, "name": s.name, "port": s.port }).to_string()
                };
                let _ = sock.send_to(reply.as_bytes(), from);
            }
        });
        web::start();
    });
}

/// One remote's connection: its hello, then what it asks.
fn serve(stream: TcpStream) {
    let _ = stream.set_nodelay(true);
    let Ok(mut writer) = stream.try_clone() else { return };
    let (tx, rx) = channel::<String>();
    std::thread::spawn(move || {
        for line in rx {
            if writer.write_all(line.as_bytes()).and_then(|_| writer.write_all(b"\n")).is_err() {
                break;
            }
        }
        let _ = writer.shutdown(std::net::Shutdown::Both);
    });
    let mut lines = BufReader::new(stream).lines();
    let Some(Ok(first)) = lines.next() else { return };
    let hello: Value = serde_json::from_str(&first).unwrap_or(Value::Null);
    let (id, name, token) = (hello["id"].as_str().unwrap_or_default().to_string(), hello["name"].as_str().unwrap_or("Remote").to_string(), hello["token"].as_str().unwrap_or_default().to_string());
    let key = {
        let mut s = shared();
        if !s.enabled || id.is_empty() {
            let _ = tx.send(json!({ "type": "disabled" }).to_string());
            return;
        }
        let linked = s.linked.iter().any(|l| l.id == id && !token.is_empty() && l.token == token);
        let k = s.next_conn;
        s.next_conn += 1;
        let _ = tx.send(json!({ "type": "welcome", "id": s.id, "name": s.name, "linked": linked }).to_string());
        s.conns.insert(k, Conn { id: id.clone(), name: name.clone(), linked, tx });
        if linked {
            s.resend = true;
            if let Some(c) = s.conns.get(&k) {
                for v in [&s.last_state, &s.last_thumbs].into_iter().flatten() {
                    let _ = c.tx.send(v.clone());
                }
            }
        }
        wake(&s);
        k
    };
    // (automated checks: KN_REMOTE_LINK links whoever connects, after showing the code)
    if std::env::var_os("KN_REMOTE_LINK").is_some() && !shared().conns.get(&key).is_some_and(|c| c.linked) {
        link(key);
        std::thread::sleep(Duration::from_millis(1500));
        confirm();
    }
    for line in lines {
        let Ok(line) = line else { break };
        let v: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let mut s = shared();
        if !s.enabled || !s.conns.get(&key).is_some_and(|c| c.linked) {
            continue;
        }
        handle(&mut s, &v);
    }
    let mut s = shared();
    s.conns.remove(&key);
    if s.pending.as_ref().is_some_and(|p| p.0 == key) {
        s.pending = None;
    }
    wake(&s);
}

/// What a linked remote (or the web remote) asked.
fn handle(s: &mut Shared, v: &Value) {
    let f = |k: &str| v[k].as_f64().unwrap_or(0.0) as f32;
    match v["type"].as_str().unwrap_or_default() {
        "cmd" => {
            let c = match v["cmd"].as_str().unwrap_or_default() {
                "next" => Some(Cmd::Next),
                "prev" => Some(Cmd::Prev),
                "stop" => Some(Cmd::Stop),
                "play" => Some(Cmd::Play),
                "goto" => v["slide"].as_u64().map(|i| Cmd::Goto(i as usize)),
                _ => None,
            };
            s.cmds.extend(c);
        }
        "laser" => s.laser = v["x"].is_number().then(|| ([f("x"), f("y")], Instant::now())),
        "draw" => {
            let id = v["id"].as_u64().unwrap_or(0);
            let color = parse_color(v["color"].as_str().unwrap_or("#ff3b30"));
            let points: Vec<[f32; 2]> = v["points"].as_array().map(|a| a.iter().map(|p| [p[0].as_f64().unwrap_or(0.0) as f32, p[1].as_f64().unwrap_or(0.0) as f32]).collect()).unwrap_or_default();
            match s.marks.iter_mut().find(|m| m.id == id) {
                Some(m) => m.points = points,
                None => s.marks.push(Mark { id, color, points }),
            }
        }
        "undo" => {
            s.marks.pop();
        }
        "clear" => s.marks.clear(),
        "thumbs" => s.thumbs_asked = true,
        "state" => s.resend = true,
        _ => {}
    }
    if matches!(v["type"].as_str(), Some("draw" | "undo" | "clear")) {
        s.marked = Some(Instant::now());
    }
    wake(s);
}

fn parse_color(c: &str) -> Color32 {
    let h = c.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0xff3b30);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Link clicked next to a remote: a four-digit code on both devices.
fn link(key: u64) {
    let mut s = shared();
    let digits: String = random(12).chars().filter_map(|c| c.to_digit(16)).map(|d| char::from(b'0' + (d % 10) as u8)).take(4).collect();
    if let Some(c) = s.conns.get(&key) {
        send(c, json!({ "type": "code", "code": digits }));
    }
    s.pending = Some((key, digits));
    wake(&s);
}

/// Confirm: the codes match; the remote is linked (and remembered).
fn confirm() {
    let mut s = shared();
    let Some((key, _)) = s.pending.take() else { return };
    let token = random(32);
    let Some((id, name)) = s.conns.get(&key).map(|c| (c.id.clone(), c.name.clone())) else { return };
    s.linked.retain(|l| l.id != id);
    s.linked.push(Linked { id, name, token: token.clone() });
    if let Some(c) = s.conns.get_mut(&key) {
        c.linked = true;
        send(c, json!({ "type": "linked", "token": token }));
    }
    s.resend = true;
    save(&s);
    wake(&s);
}

/// The show's commands from remotes since last frame.
pub fn take_cmds() -> Vec<Cmd> {
    std::mem::take(&mut shared().cmds)
}

/// Each frame: the listeners running, a Play from a remote starting the show here, and what
/// the remotes see sent when it changed (while not playing: the current slide).
pub fn tick(app: &mut App, ctx: &egui::Context) {
    start(ctx);
    if app.play.is_some() {
        return;
    }
    let play = {
        let mut s = shared();
        let cmds = std::mem::take(&mut s.cmds);
        cmds.iter().any(|c| matches!(c, Cmd::Play))
    };
    if play && app.doc.is_some() {
        app.start_slideshow(app.cur);
        return;
    }
    publish(app, None);
}

/// Base64 of bytes (the pictures sent in JSON).
fn b64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

/// Slide `index` with its builds up to `step` as a JPEG `width` pixels wide.
fn picture(app: &mut App, index: usize, step: usize, width: u32) -> Option<String> {
    let sc = app.scene(index)?;
    let st = crate::builds::state_at(&sc, step);
    let sc2 = st.scene_for(&sc);
    let sc = sc2.as_ref().unwrap_or(&sc);
    let doc = app.doc.as_ref()?;
    let h = (width as f32 * sc.size.h / sc.size.w).round().max(1.0) as u32;
    let mut pm = kn_render::tiny_skia::Pixmap::new(width, h)?;
    let k = width as f32 / sc.size.w;
    crate::builds::draw_state(&mut app.renderer, doc, sc, &st, &mut pm, kn_render::tiny_skia::Transform::from_scale(k, k), &|_| true, true);
    let mut rgb = image::RgbImage::new(width, h);
    for (i, p) in pm.pixels().iter().enumerate() {
        let c = p.demultiply();
        rgb.put_pixel(i as u32 % width, i as u32 / width, image::Rgb([c.red(), c.green(), c.blue()]));
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80).encode_image(&rgb).ok()?;
    Some(b64(&out))
}

fn steps_of(app: &mut App, index: usize) -> usize {
    app.scene(index).map(|s| s.build_order.len()).unwrap_or(0)
}

/// What the remotes see (sent when it changes): the show's slide and build `at` (None: not
/// playing, the slide being edited), the pictures of now and next, the notes.
pub fn publish(app: &mut App, at: Option<(usize, usize)>) {
    let (any, resend, thumbs) = {
        let mut s = shared();
        // while Remotes is on, kept current even with no remote connected, so one that
        // connects while the window isn't drawn still gets the slide at once
        let any = s.enabled;
        (any, std::mem::take(&mut s.resend), std::mem::take(&mut s.thumbs_asked))
    };
    if !any {
        return;
    }
    if app.doc.is_none() {
        // nothing open: the remotes say so (once) instead of showing an empty slide
        let mut s = shared();
        if resend || s.sent != "nodoc" {
            s.sent = "nodoc".into();
            s.last_state = Some(json!({ "type": "nodoc" }).to_string());
            for c in s.conns.values().filter(|c| c.linked) {
                send(c, json!({ "type": "nodoc" }));
            }
        }
        return;
    }
    let count = app.slides.len();
    if thumbs {
        let list: Vec<Value> = (0..count).map(|i| json!({ "index": i, "skipped": app.slides[i].skipped, "image": picture(app, i, usize::MAX, 240) })).collect();
        let mut s = shared();
        let v = json!({ "type": "thumbs", "slides": list });
        s.last_thumbs = Some(v.to_string());
        for c in s.conns.values().filter(|c| c.linked) {
            send(c, v.clone());
        }
    }
    let (index, step) = at.unwrap_or((app.cur, usize::MAX));
    let index = index.min(count.saturating_sub(1));
    let rev = app.slides.get(index).map(|s| app.slide_rev(s.slide)).unwrap_or(0);
    let key = format!("{}:{index}:{step}:{count}:{rev}", at.is_some());
    if !resend && shared().sent == key {
        return;
    }
    let steps = steps_of(app, index);
    let step_n = step.min(steps);
    // next: the next build here, else the next slide that isn't skipped
    let next = if at.is_some() && step_n < steps { Some((index, step_n + 1)) } else { (index + 1..count).find(|&i| !app.slides[i].skipped).map(|i| (i, if at.is_some() { 0 } else { usize::MAX })) };
    let current = picture(app, index, step_n, 960);
    let next_pic = next.and_then(|(i, s)| picture(app, i, s, 960));
    let number = app.slides[..=index].iter().filter(|s| !s.skipped).count();
    let notes = app.slides.get(index).and_then(|s| app.doc.as_ref().map(|d| d.notes(s.slide))).unwrap_or_default();
    let size = app.doc.as_ref().map(|d| d.slide_size()).unwrap_or(kn_model::Size { w: 1920.0, h: 1080.0 });
    let title = app.tab_name(app.active_tab);
    let v = json!({
        "type": "state", "playing": at.is_some(), "slide": index, "step": step_n, "steps": steps, "count": count,
        "number": number, "visible": app.slides.iter().filter(|s| !s.skipped).count(), "notes": notes, "title": title,
        "current": current, "next": next_pic, "aspect": size.h / size.w,
    });
    let mut s = shared();
    if s.sent.split(':').nth(1) != key.split(':').nth(1) || !key.starts_with("true") {
        // a new slide: the highlights go (as Keynote's)
        s.marks.clear();
        s.laser = None;
    }
    s.sent = key;
    s.last_state = Some(v.to_string());
    for c in s.conns.values().filter(|c| c.linked) {
        send(c, v.clone());
    }
}

/// Each show frame: the remotes' commands applied to the show, its state sent when it changed,
/// and the laser and highlights drawn over the slide (in `rect`).
pub fn draw_overlay(painter: &Painter, rect: Rect) {
    let s = shared();
    let at = |p: [f32; 2]| Pos2::new(rect.min.x + p[0] * rect.width(), rect.min.y + p[1] * rect.height());
    let w = (rect.width() / 240.0).max(2.0);
    for m in &s.marks {
        let pts: Vec<Pos2> = m.points.iter().map(|p| at(*p)).collect();
        if pts.len() == 1 {
            painter.circle_filled(pts[0], w, m.color);
        } else {
            painter.add(egui::Shape::line(pts, Stroke::new(w * 1.6, m.color)));
        }
    }
    // (drawn every frame for a moment after a change: wakes from other threads come in bursts)
    if s.marked.is_some_and(|t| t.elapsed() < Duration::from_millis(600)) {
        if let Some(ctx) = &s.ctx {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
    if let Some((p, t)) = s.laser {
        if t.elapsed() < Duration::from_millis(1500) {
            let c = at(p);
            let r = (rect.width() / 160.0).max(5.0);
            painter.circle_filled(c, r * 2.2, Color32::from_rgba_unmultiplied(255, 40, 30, 50));
            painter.circle_filled(c, r * 1.4, Color32::from_rgba_unmultiplied(255, 40, 30, 110));
            painter.circle_filled(c, r, Color32::from_rgb(255, 59, 48));
            painter.circle_filled(c, r * 0.45, Color32::from_rgb(255, 210, 200));
            if let Some(ctx) = &s.ctx {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }
    }
}

/// Settings > Remotes: Enable; the remotes looking for this computer (Link, then the code and
/// Confirm); the remotes linked (Unlink).
pub fn settings_tab(ui: &mut egui::Ui) {
    let (mut enabled, name) = {
        let s = shared();
        (s.enabled, s.name.clone())
    };
    if ui.checkbox(&mut enabled, "Enable").changed() {
        let mut s = shared();
        s.enabled = enabled;
        if !enabled {
            for c in s.conns.values() {
                send(c, json!({ "type": "disabled" }));
            }
            s.conns.retain(|_, c| !c.id.starts_with("web:"));
        }
        save(&s);
    }
    ui.label(egui::RichText::new(format!("Use a phone or tablet running Keydeck to control slideshows on \u{201c}{name}\u{201d}: open Keydeck there, choose Play > Keydeck Remote, then click Link next to it below.")).size(12.0).color(crate::theme::TEXT_DIM()));
    ui.add_space(8.0);
    if !enabled {
        return;
    }
    let mut act: Option<(&str, u64, String)> = None;
    {
        let s = shared();
        let looking: Vec<(u64, String)> = s.conns.iter().filter(|(_, c)| !c.linked).map(|(k, c)| (*k, c.name.clone())).collect();
        ui.label(egui::RichText::new("Remotes nearby").strong());
        if looking.is_empty() {
            ui.label(egui::RichText::new("No remotes are looking for this computer.").color(crate::theme::TEXT_DIM()));
        }
        for (k, n) in looking {
            ui.horizontal(|ui| {
                ui.label(&n);
                match &s.pending {
                    Some((pk, code)) if *pk == k => {
                        ui.label(egui::RichText::new(code).size(20.0).strong());
                        if ui.button("Confirm").clicked() {
                            act = Some(("confirm", k, String::new()));
                        }
                        if ui.button("Cancel").clicked() {
                            act = Some(("cancel", k, String::new()));
                        }
                    }
                    _ => {
                        if ui.button("Link").clicked() {
                            act = Some(("link", k, String::new()));
                        }
                    }
                }
            });
        }
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Linked remotes").strong());
        if s.linked.is_empty() {
            ui.label(egui::RichText::new("None yet.").color(crate::theme::TEXT_DIM()));
        }
        for l in &s.linked {
            let on = s.conns.values().any(|c| c.linked && c.id == l.id);
            ui.horizontal(|ui| {
                ui.colored_label(if on { Color32::from_rgb(0x34, 0xc7, 0x59) } else { crate::theme::TEXT_DIM() }, "\u{25cf}");
                ui.label(&l.name);
                if ui.button("Unlink").clicked() {
                    act = Some(("unlink", 0, l.id.clone()));
                }
            });
        }
    }
    match act {
        Some(("link", k, _)) => link(k),
        Some(("confirm", _, _)) => confirm(),
        Some(("cancel", k, _)) => {
            let mut s = shared();
            s.pending = None;
            if let Some(c) = s.conns.get(&k) {
                send(c, json!({ "type": "codeCancel" }));
            }
        }
        Some(("unlink", _, id)) => {
            let mut s = shared();
            s.linked.retain(|l| l.id != id);
            for c in s.conns.values_mut().filter(|c| c.id == id) {
                c.linked = false;
                send(c, json!({ "type": "unlinked" }));
            }
            save(&s);
        }
        _ => {}
    }
    web::settings(ui);
    ui.ctx().request_repaint_after(Duration::from_millis(500));
}
