//! The web remote: a browser on a phone (or anything on the same network) controls the show.
//! A small HTTP server on port 51739 (the next ones if it's taken) serves one page; the page
//! gets what the other remotes get by Server-Sent Events (GET /events) and sends its commands
//! by POST /cmd (JSON, as the other remotes' lines). Every request carries the secret key
//! (?k=…) that Settings > Remotes shows in the link; New Link replaces it.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use egui::{Color32, Rect, Vec2};
use serde_json::{Value, json};

use super::{Conn, handle, random, save, send, shared, wake};

pub const PORT: u16 = 51739;

const PAGE: &str = include_str!("remote_web.html");
/// The Android remote's font (Carlito, SIL Open Font License), for the page to look the same.
const FONT: &[u8] = include_bytes!("../../kn-render/fonts/carlito/Carlito-Regular.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../../kn-render/fonts/carlito/Carlito-Bold.ttf");

pub(super) fn start() {
    let Some(listener) = (0..10).find_map(|k| TcpListener::bind(("0.0.0.0", PORT + k)).ok()) else { return };
    shared().web_port = listener.local_addr().map(|a| a.port()).unwrap_or(PORT);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve(stream));
        }
    });
}

/// A request: its method, path, key and body.
#[derive(Debug, PartialEq)]
struct Req {
    method: String,
    path: String,
    key: String,
    len: usize,
}

/// The request line and headers (up to the blank line).
fn parse_head(head: &str) -> Option<Req> {
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let target = first.next()?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let key = query.split('&').find_map(|p| p.strip_prefix("k=")).unwrap_or_default().to_string();
    let len = lines
        .find_map(|l| {
            let (n, v) = l.split_once(':')?;
            n.trim().eq_ignore_ascii_case("content-length").then(|| v.trim().parse().ok()).flatten()
        })
        .unwrap_or(0);
    Some(Req { method, path: path.to_string(), key, len })
}

/// The key given matches (in the same time whatever it is).
fn key_ok(given: &str, key: &str) -> bool {
    !key.is_empty() && given.len() == key.len() && given.bytes().zip(key.bytes()).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0
}

fn respond(mut stream: &TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n", body.len());
    let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body));
}

fn serve(stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let mut r = BufReader::new((&stream).take(80_000));
    let mut head = String::new();
    loop {
        let mut line = String::new();
        match r.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            _ if line.trim_end().is_empty() => break,
            _ => head.push_str(&line),
        }
    }
    let Some(req) = parse_head(&head) else { return };
    let mut body = vec![0u8; req.len.min(64_000)];
    if r.read_exact(&mut body).is_err() {
        return;
    }
    let (enabled, key) = {
        let s = shared();
        (s.enabled, s.web_key.clone())
    };
    let text = "text/plain; charset=utf-8";
    if !enabled {
        return respond(&stream, "503 Service Unavailable", text, "Remotes are turned off: in Keydeck choose Settings > Remotes, then select Enable.".as_bytes());
    }
    if !key_ok(&req.key, &key) {
        return respond(&stream, "403 Forbidden", text, "This link no longer works: open the one in Keydeck's Settings > Remotes.".as_bytes());
    }
    match (req.method.as_str(), req.path.as_str()) {
        // the key goes into the font links here: browsers other than Chrome refuse a page
        // script rewriting @font-face (and the page's script stopped there: a black page)
        ("GET", "/") => respond(&stream, "200 OK", "text/html; charset=utf-8", PAGE.replace("font?k=KEY", &format!("font?k={key}")).as_bytes()),
        ("GET", "/events") => events(stream),
        ("GET", "/ping") => respond(&stream, "204 No Content", text, b""),
        ("GET", "/font") => respond(&stream, "200 OK", "font/ttf", FONT),
        ("GET", "/font-bold") => respond(&stream, "200 OK", "font/ttf", FONT_BOLD),
        ("POST", "/cmd") => {
            let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            if matches!(v["type"].as_str(), Some("cmd" | "laser" | "draw" | "undo" | "clear" | "thumbs" | "state")) {
                handle(&mut shared(), &v);
                respond(&stream, "204 No Content", text, b"");
            } else {
                respond(&stream, "400 Bad Request", text, b"");
            }
        }
        _ => respond(&stream, "404 Not Found", text, b""),
    }
}

/// GET /events: the page joins the remotes; what they're sent goes to it as events (and a
/// comment every 15 s, which finds a page gone).
fn events(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(None);
    if stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n: ok\n\n").is_err() {
        return;
    }
    let (tx, rx) = channel::<String>();
    let key = {
        let mut s = shared();
        let k = s.next_conn;
        s.next_conn += 1;
        let _ = tx.send(json!({ "type": "welcome", "id": s.id, "name": s.name, "linked": true }).to_string());
        s.conns.insert(k, Conn { id: format!("web:{k}"), name: "Web Remote".into(), linked: true, tx });
        s.resend = true;
        if let Some(c) = s.conns.get(&k) {
            for v in [&s.last_state, &s.last_thumbs].into_iter().flatten() {
                let _ = c.tx.send(v.clone());
            }
        }
        wake(&s);
        k
    };
    loop {
        let out = match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(line) => format!("data: {line}\n\n"),
            Err(RecvTimeoutError::Timeout) => ": ping\n\n".to_string(),
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if stream.write_all(out.as_bytes()).is_err() {
            break;
        }
    }
    shared().conns.remove(&key);
}

/// This computer's address on the network (the interface of the default route: connecting a
/// UDP socket sends nothing).
fn lan_ip() -> String {
    UdpSocket::bind(("0.0.0.0", 0))
        .and_then(|s| {
            s.connect(("8.8.8.8", 80))?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .ok()
        .filter(|ip| ip != "0.0.0.0")
        .unwrap_or_else(|| "127.0.0.1".into())
}

/// Settings > Remotes > Web Remote: the QR code on a white tile and, beside it (under it when
/// the window is narrow), the address, Copy Link, New Link and the browsers connected.
pub(super) fn settings(ui: &mut egui::Ui) {
    let (key, port, browsers) = {
        let s = shared();
        (s.web_key.clone(), s.web_port, s.conns.values().filter(|c| c.id.starts_with("web:")).count())
    };
    super::title(ui, "Web Remote");
    if port == 0 {
        super::group(ui, |ui| super::row(ui, |ui| { ui.label(egui::RichText::new("The web remote couldn\u{2019}t start: its network ports are in use.").size(12.5)); }, |_| {}));
        return;
    }
    let ip = lan_ip();
    let url = format!("http://{ip}:{port}/?k={key}");
    let mut new_link = false;
    super::group(ui, |ui| {
        let wide = ui.available_width() >= 360.0;
        let side = if wide { 148.0 } else { ui.available_width().min(200.0) };
        let info = |ui: &mut egui::Ui, new_link: &mut bool| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(egui::RichText::new("Scan with a phone or tablet").size(13.0));
            ui.label(egui::RichText::new("Or open this address in a browser on the same network:").size(11.5).color(crate::theme::TEXT_DIM()));
            ui.add(egui::Label::new(egui::RichText::new(format!("{ip}:{port}")).size(12.5).monospace()).selectable(true));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Copy Link").clicked() {
                    ui.ctx().copy_text(url.clone());
                }
                if ui.add(egui::Button::new(egui::RichText::new("New Link").color(Color32::from_rgb(0xff, 0x45, 0x3a)))).on_hover_text("Make a new link; pages opened with the old one stop working").clicked() {
                    *new_link = true;
                }
            });
            let (dot, t) = if browsers > 0 {
                (Color32::from_rgb(0x34, 0xc7, 0x59), format!("{browsers} browser{} connected", if browsers == 1 { "" } else { "s" }))
            } else {
                (crate::theme::TEXT_DIM(), "No browsers connected".to_string())
            };
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 3.5, dot);
                ui.label(egui::RichText::new(t).size(11.5).color(crate::theme::TEXT_DIM()));
            });
        };
        ui.add_space(10.0);
        if wide {
            ui.horizontal_top(|ui| {
                qr(ui, &url, side);
                ui.add_space(14.0);
                ui.vertical(|ui| info(ui, &mut new_link));
            });
        } else {
            ui.vertical_centered(|ui| qr(ui, &url, side));
            ui.add_space(10.0);
            info(ui, &mut new_link);
        }
        ui.add_space(10.0);
    });
    super::note(ui, "Anyone with the link can control the slideshow.");
    if new_link {
        new_key();
    }
}

/// New Link: a new key; the pages open with the old one are dropped.
fn new_key() {
    let mut s = shared();
    s.web_key = random(32);
    for c in s.conns.values().filter(|c| c.id.starts_with("web:")) {
        send(c, json!({ "type": "expired" }));
    }
    s.conns.retain(|_, c| !c.id.starts_with("web:"));
    save(&s);
}

/// The link's QR code, `side` points square: black modules on a white rounded tile (with the
/// quiet zone), whole pixels per module so it stays sharp.
fn qr(ui: &mut egui::Ui, text: &str, side: f32) {
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else { return };
    let n = code.width();
    let colors = code.to_colors();
    let ppp = ui.ctx().pixels_per_point();
    let cell = ((side / (n + 8) as f32 * ppp).floor().max(1.0)) / ppp;
    let side = (n + 8) as f32 * cell;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::hover());
    let rect = Rect::from_min_size(egui::pos2((rect.min.x * ppp).round() / ppp, (rect.min.y * ppp).round() / ppp), rect.size());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 10.0, Color32::WHITE);
    for (i, c) in colors.iter().enumerate() {
        if *c == qrcode::Color::Dark {
            let min = rect.min + Vec2::new((i % n + 4) as f32 * cell, (i / n + 4) as f32 * cell);
            p.rect_filled(Rect::from_min_size(min, Vec2::splat(cell)), 0.0, Color32::BLACK);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requests() {
        let r = parse_head("POST /cmd?k=abc123&x=1 HTTP/1.1\r\nHost: h\r\nContent-length: 27\r\n").unwrap();
        assert_eq!(r, Req { method: "POST".into(), path: "/cmd".into(), key: "abc123".into(), len: 27 });
        let r = parse_head("GET / HTTP/1.1\r\n").unwrap();
        assert_eq!((r.path.as_str(), r.key.as_str(), r.len), ("/", "", 0));
        assert!(parse_head("").is_none());
    }

    #[test]
    fn checks_keys() {
        assert!(key_ok("abcd", "abcd"));
        assert!(!key_ok("abce", "abcd"));
        assert!(!key_ok("abc", "abcd"));
        assert!(!key_ok("", ""));
    }

    #[test]
    fn page_is_self_contained() {
        assert!(!PAGE.contains("http://") && !PAGE.contains("https://"));
        assert!(PAGE.contains("events?k=") && PAGE.contains("cmd?k=") && !PAGE.contains("xmlns"));
    }
}
