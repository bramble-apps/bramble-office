//! Sync: two libraries kept in step through a WebDAV server (a small one run here).

mod common;

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use nt_core::model::*;
use nt_core::store::Store;
use nt_core::sync::{self, Force, Settings};

/// A WebDAV server in memory: PUT, GET, DELETE, MKCOL, PROPFIND (depth 1), ETags, Basic auth.
struct Dav {
    files: Mutex<HashMap<String, Vec<u8>>>,
    dirs: Mutex<std::collections::HashSet<String>>,
    requests: Mutex<usize>,
    faults: Mutex<Faults>,
    in_flight: std::sync::atomic::AtomicUsize,
}

/// What the server does wrong, and what it saw.
#[derive(Default)]
struct Faults {
    /// every this-many-th write is answered 429 (Retry-After: 0)
    busy_every: usize,
    /// writes after this many fail (500)
    fail_after: Option<usize>,
    /// added to every answer
    latency: std::time::Duration,
    writes: usize,
    busy_sent: usize,
    /// paths written, in order
    written: Vec<String>,
    most_at_once: usize,
    /// attachments (files/…) are read out at this many bytes a second
    rate: Option<usize>,
    /// and taken in at this many
    write_rate: Option<usize>,
    /// these attachments' first read stops halfway and goes quiet this long, then drops
    stall: HashMap<String, std::time::Duration>,
    /// these attachments are read back damaged
    damaged: HashSet<String>,
    /// attachment reads after this many fail (500)
    fail_reads_after: Option<usize>,
    reads: usize,
    /// reads that asked for the rest of a file (Range)
    ranged: usize,
}

fn etag(b: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for x in b {
        h = (h ^ u64::from(*x)).wrapping_mul(0x100000001b3);
    }
    format!("\"{h:x}-{}\"", b.len())
}

fn decode(p: &str) -> String {
    let b = p.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            out.push(u8::from_str_radix(&p[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

impl Dav {
    fn start() -> (Arc<Dav>, String) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/dav", l.local_addr().unwrap());
        let d = Arc::new(Dav { files: Mutex::default(), dirs: Mutex::new(["/dav".to_string()].into()), requests: Mutex::new(0), faults: Mutex::default(), in_flight: Default::default() });
        let srv = d.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let srv = srv.clone();
                std::thread::spawn(move || srv.serve(s));
            }
        });
        (d, url)
    }

    fn serve(&self, s: TcpStream) {
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut w = s;
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let mut parts = line.split_whitespace();
            let (method, target) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
            let mut headers = HashMap::new();
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                let h = h.trim_end();
                if h.is_empty() {
                    break;
                }
                if let Some((k, v)) = h.split_once(':') {
                    headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                }
            }
            let mut body = vec![0; headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0)];
            match self.faults.lock().unwrap().write_rate.filter(|_| target.contains("/files/")) {
                Some(rate) => {
                    for part in body.chunks_mut((rate / 20).max(1)) {
                        r.read_exact(part).unwrap();
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
                None => r.read_exact(&mut body).unwrap(),
            }
            *self.requests.lock().unwrap() += 1;
            let at_once = self.in_flight.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            let latency = {
                let mut f = self.faults.lock().unwrap();
                f.most_at_once = f.most_at_once.max(at_once);
                f.latency
            };
            std::thread::sleep(latency);
            let path = decode(target.trim_end_matches('/'));
            let fault = (method == "PUT").then(|| {
                let mut f = self.faults.lock().unwrap();
                f.writes += 1;
                if f.fail_after.is_some_and(|n| f.writes > n) {
                    Some(("500 Internal Server Error", String::new()))
                } else if f.busy_every > 0 && f.writes % f.busy_every == 0 {
                    f.busy_sent += 1;
                    Some(("429 Too Many Requests", "Retry-After: 0\r\n".to_string()))
                } else {
                    f.written.push(path.clone());
                    None
                }
            }).flatten();
            // a server that moved: /old/… redirects to /dav/… (before asking for the password)
            let (status, extra, out): (&str, String, Vec<u8>) = if let Some(rest) = target.strip_prefix("/old") {
                ("308 Permanent Redirect", format!("Location: /dav{rest}\r\n"), Vec::new())
            } else if headers.get("authorization").map(String::as_str) != Some("Basic dXNlcjpzZWNyZXQ=") {
                ("401 Unauthorized", String::new(), Vec::new())
            } else if let Some((status, extra)) = fault {
                (status, extra, Vec::new())
            } else {
                self.handle(&method, &path, body)
            };
            self.in_flight.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            // attachments: read faults, the rest of a file (Range), slow reads, a read that stalls
            let (mut status, mut extra, mut out) = (status, extra, out);
            let mut stall = None;
            let mut rate = None;
            if method == "GET" && path.contains("/files/") && status.starts_with("200") {
                let mut f = self.faults.lock().unwrap();
                f.reads += 1;
                rate = f.rate;
                if f.fail_reads_after.is_some_and(|n| f.reads > n) {
                    (status, extra, out) = ("500 Internal Server Error", String::new(), Vec::new());
                } else {
                    if f.damaged.contains(&path) {
                        out[0] ^= 0xff;
                    }
                    stall = f.stall.remove(&path);
                    if let Some(from) = headers.get("range").and_then(|r| r.strip_prefix("bytes=")).and_then(|r| r.trim_end_matches('-').parse::<usize>().ok()) {
                        f.ranged += 1;
                        let total = out.len();
                        out = out.split_off(from.min(total));
                        status = "206 Partial Content";
                        extra = format!("{extra}Content-Range: bytes {from}-{}/{total}\r\n", total.saturating_sub(1));
                    }
                }
            }
            let resp = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}\r\n", out.len());
            if w.write_all(resp.as_bytes()).is_err() {
                return;
            }
            if let Some(quiet) = stall {
                // half, then nothing, then the connection drops
                let _ = w.write_all(&out[..out.len() / 2]);
                let _ = w.flush();
                std::thread::sleep(quiet);
                return;
            }
            let chunk = rate.map(|r| (r / 20).max(1)).unwrap_or(out.len().max(1));
            for part in out.chunks(chunk) {
                if w.write_all(part).is_err() {
                    return;
                }
                if rate.is_some() {
                    let _ = w.flush();
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    }

    fn handle(&self, method: &str, path: &str, body: Vec<u8>) -> (&'static str, String, Vec<u8>) {
        let parent = |p: &str| p.rsplit_once('/').map(|(a, _)| a.to_string()).unwrap_or_default();
        match method {
            "MKCOL" => {
                let mut dirs = self.dirs.lock().unwrap();
                if dirs.contains(path) {
                    ("405 Method Not Allowed", String::new(), Vec::new())
                } else if !dirs.contains(&parent(path)) {
                    ("409 Conflict", String::new(), Vec::new())
                } else {
                    dirs.insert(path.to_string());
                    ("201 Created", String::new(), Vec::new())
                }
            }
            "PUT" => {
                if !self.dirs.lock().unwrap().contains(&parent(path)) {
                    return ("409 Conflict", String::new(), Vec::new());
                }
                let e = etag(&body);
                self.files.lock().unwrap().insert(path.to_string(), body);
                ("201 Created", format!("ETag: {e}\r\n"), Vec::new())
            }
            "GET" => match self.files.lock().unwrap().get(path) {
                Some(b) => ("200 OK", format!("ETag: {}\r\n", etag(b)), b.clone()),
                None => ("404 Not Found", String::new(), Vec::new()),
            },
            "DELETE" => match self.files.lock().unwrap().remove(path) {
                Some(_) => ("204 No Content", String::new(), Vec::new()),
                None => ("404 Not Found", String::new(), Vec::new()),
            },
            "PROPFIND" => {
                if !self.dirs.lock().unwrap().contains(path) {
                    return ("404 Not Found", String::new(), Vec::new());
                }
                let mut xml = String::from(r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:">"#);
                xml.push_str(&format!("<d:response><d:href>{path}/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>"));
                for d in self.dirs.lock().unwrap().iter().filter(|d| parent(d) == path) {
                    xml.push_str(&format!("<d:response><d:href>{}/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>", d.replace(' ', "%20")));
                }
                for (f, b) in self.files.lock().unwrap().iter().filter(|(f, _)| parent(f) == path) {
                    xml.push_str(&format!(
                        "<d:response><d:href>{}</d:href><d:propstat><d:prop><d:getetag>{}</d:getetag><d:getcontentlength>{}</d:getcontentlength><d:resourcetype/></d:prop></d:propstat></d:response>",
                        f.replace(' ', "%20"),
                        etag(b).replace('"', "&quot;"),
                        b.len()
                    ));
                }
                xml.push_str("</d:multistatus>");
                ("207 Multi-Status", "Content-Type: application/xml\r\n".into(), xml.into_bytes())
            }
            _ => ("405 Method Not Allowed", String::new(), Vec::new()),
        }
    }

    fn requests(&self) -> usize {
        *self.requests.lock().unwrap()
    }
}

fn settings(url: &str) -> Settings {
    Settings { on: true, url: url.into(), user: "user".into(), password: "secret".into(), folder: "Bramble Notes".into() }
}

fn texts(st: &Store) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = st.library().unwrap().notes.iter().map(|n| (n.id.clone(), n.body.text())).collect();
    v.sort();
    v
}

fn note_with(st: &mut Store, lines: &[&str]) -> Note {
    let mut b = Body::from_lines(lines[0], &lines[1..]);
    st.create_note(None, &mut b).unwrap()
}

#[test]
fn two_libraries_stay_in_step() {
    let (srv, url) = Dav::start();
    let files = srv.clone();
    scenario("sync", &url, move || files.files.lock().unwrap().values().cloned().collect(), move || srv.requests());
}

/// The same against a real WebDAV server (`rclone serve webdav`), when rclone is installed.
#[test]
fn two_libraries_stay_in_step_on_rclone() {
    let Ok(_) = std::process::Command::new("rclone").arg("version").output() else { return eprintln!("skipped: rclone not installed") };
    let dir = common::scratch("sync-rclone-server");
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut child = std::process::Command::new("rclone")
        .args(["serve", "webdav", "--addr", &format!("127.0.0.1:{port}"), "--user", "user", "--pass", "secret"])
        .arg(&dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let d = dir.clone();
    let result = std::panic::catch_unwind(|| {
        scenario("sync-rclone", &format!("http://127.0.0.1:{port}"), move || walk_dir(&d), || 0);
    });
    let _ = child.kill();
    let _ = child.wait();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

fn walk_dir(d: &std::path::Path) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut todo = vec![d.to_path_buf()];
    while let Some(p) = todo.pop() {
        for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
            if e.path().is_dir() {
                todo.push(e.path());
            } else {
                out.push(std::fs::read(e.path()).unwrap());
            }
        }
    }
    out
}

fn scenario(name: &str, url: &str, server_files: impl Fn() -> Vec<Vec<u8>>, requests: impl Fn() -> usize) {
    let s = settings(url);
    assert!(sync::test_connection(&Settings { password: "wrong".into(), ..s.clone() }).is_some_and(|e| e.contains("refused")));
    assert_eq!(sync::test_connection(&s), None);

    let (da, db) = (common::scratch(&format!("{name}-a")), common::scratch(&format!("{name}-b")));
    let mut a = Store::open(&da).unwrap();
    let mut b = Store::open(&db).unwrap();

    // library A: text, a picture, a table, a drawing, a folder, a Smart Folder, a locked note
    let n1 = note_with(&mut a, &["Groceries", "jelly and bread"]);
    let png = da.join("pic.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x03\x08\x02\0\0\0pixels").unwrap();
    let pic = a.add_attachment(&n1.id, &png).unwrap();
    let tid = a.add_table(&n1.id, &Table::from_text(&[&["a", "b"], &["c", ""]])).unwrap();
    let mut n = a.note(&n1.id).unwrap();
    n.body.paragraphs.push(Paragraph { spans: vec![Span::token(&pic.id, &pic.uti)], ..Default::default() });
    n.body.paragraphs.push(Paragraph { spans: vec![Span::token(&tid, UTI_TABLE)], ..Default::default() });
    let ink = Drawing { width: 768.0, height: 100.0, strokes: vec![Stroke { id: "s".into(), ink: InkKind::Pen, color: [0.0, 0.0, 0.0, 1.0], width: 2.6, points: (0..4).map(|i| InkPoint { x: 10.0 * i as f32, y: 20.0, t: 0.01 * i as f32, size: 2.6, force: 1.0, azimuth: 0.0, altitude: 1.5, opacity: 1.0 }).collect(), transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], mask: None }], shapes: Vec::new() };
    let did = nt_core::pb::new_uuid();
    n.attachments.push(Attachment { id: did.clone(), uti: UTI_PAPER.into(), kind: AttachmentKind::Drawing, drawing: Some(ink.clone()), width: 768.0, height: 100.0, ..Default::default() });
    n.body.paragraphs.push(Paragraph { spans: vec![Span::token(&did, UTI_PAPER)], ..Default::default() });
    a.save_note(&mut n).unwrap();
    let folder = a.create_folder("Work", None).unwrap();
    let smart = a.create_smart_folder("Pinned", r#"{"type":{"and":[{"deleted":false},{"and":[{"pinned":true}]}]},"entity":"note"}"#).unwrap();
    let n2 = note_with(&mut a, &["Plans", "walk in the hills"]);
    a.move_note(&n2.id, &folder).unwrap();
    a.set_pinned(&n2.id, true).unwrap();
    let n3 = note_with(&mut a, &["Secret", "the code is 1234"]);
    a.set_password("pw", None).unwrap();
    a.lock_note(&n3.id, "pw").unwrap();

    // first syncs: A up, B down
    let r = sync::sync_now(&mut a, &s, Force::None).unwrap();
    assert!(r.sent >= 4, "{r:?}");
    let r = sync::sync_now(&mut b, &s, Force::None).unwrap();
    assert_eq!((r.received, r.copies), (4, 0), "{r:?}"); // 3 notes + folders
    assert_eq!(texts(&a), texts(&b));
    let bn = b.note(&n1.id).unwrap();
    assert_eq!(std::fs::read(bn.attachment(&pic.id).unwrap().file.as_ref().unwrap()).unwrap(), std::fs::read(&png).unwrap());
    assert_eq!(bn.attachment(&tid).unwrap().table, a.note(&n1.id).unwrap().attachment(&tid).unwrap().table);
    assert_eq!(bn.attachment(&did).unwrap().drawing.as_ref().unwrap().strokes.len(), 1);
    let bp = b.note(&n2.id).unwrap();
    assert!(bp.pinned && bp.folder == folder && (bp.created - n2.created).abs() < 1e-3);
    let bl = b.library().unwrap();
    assert_eq!(bl.folder(&folder).unwrap().name, "Work");
    assert!(bl.folder(&smart).unwrap().smart.is_some());
    // the locked note came over still locked, and its password opens it there
    let locked = b.note(&n3.id).unwrap();
    assert!(locked.locked && locked.body.paragraphs.is_empty());
    assert!(b.unlock_note(&n3.id, "pw").unwrap().body.text().contains("1234"));
    for f in server_files() {
        assert!(!f.windows(4).any(|w| w == b"1234"), "the locked note's text is on the server");
    }

    // all quiet: a few requests only
    let before = requests();
    assert_eq!(sync::sync_now(&mut a, &s, Force::None).unwrap().summary(), "Up to date");
    assert_eq!(sync::sync_now(&mut b, &s, Force::None).unwrap().summary(), "Up to date");
    assert!(requests() - before <= 4, "{} requests", requests() - before);

    // an edit each way
    let mut e = Body::from_lines("Groceries", &["jelly, bread and milk"]);
    a.update_note(&n1.id, &mut e).unwrap();
    sync::sync_now(&mut a, &s, Force::None).unwrap();
    sync::sync_now(&mut b, &s, Force::None).unwrap();
    assert!(b.note(&n1.id).unwrap().body.text().contains("milk"));
    let mut e = Body::from_lines("Plans", &["walk in the hills, then tea"]);
    b.update_note(&n2.id, &mut e).unwrap();
    b.rename_folder(&folder, "Work stuff").unwrap();
    sync::sync_now(&mut b, &s, Force::None).unwrap();
    sync::sync_now(&mut a, &s, Force::None).unwrap();
    assert!(a.note(&n2.id).unwrap().body.text().contains("tea"));
    assert_eq!(a.library().unwrap().folder(&folder).unwrap().name, "Work stuff");
    assert_eq!(texts(&a), texts(&b));

    // both edit the same note: the newer wins, the other is kept as "(other copy)" on both
    let mut ea = Body::from_lines("Plans", &["from A"]);
    a.update_note(&n2.id, &mut ea).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let mut eb = Body::from_lines("Plans", &["from B, later"]);
    b.update_note(&n2.id, &mut eb).unwrap();
    sync::sync_now(&mut a, &s, Force::None).unwrap();
    let r = sync::sync_now(&mut b, &s, Force::None).unwrap();
    assert_eq!(r.copies, 1, "{r:?}");
    sync::sync_now(&mut a, &s, Force::None).unwrap();
    assert!(a.note(&n2.id).unwrap().body.text().contains("from B, later"));
    let copies: Vec<String> = a.library().unwrap().notes.iter().filter(|n| n.body.text().contains("(other copy)")).map(|n| n.body.text()).collect();
    assert_eq!(copies.len(), 1);
    assert!(copies[0].contains("from A"));
    assert_eq!(texts(&a), texts(&b));

    // both edit the same note in different places: merged, no copy
    let base_note = a.note(&n2.id).unwrap();
    let mut na = base_note.clone();
    na.body.paragraphs.push(Paragraph::plain(ParaStyle::Body, "added on A"));
    a.save_note(&mut na).unwrap();
    let mut nb = b.note(&n2.id).unwrap();
    nb.body.paragraphs[0].spans = vec![Span::plain("Plans for the weekend")];
    b.save_note(&mut nb).unwrap();
    sync::sync_now(&mut a, &s, Force::None).unwrap();
    let r = sync::sync_now(&mut b, &s, Force::None).unwrap();
    assert_eq!((r.merged, r.copies), (1, 0), "{r:?}");
    sync::sync_now(&mut a, &s, Force::None).unwrap();
    for st in [&a, &b] {
        let t = st.note(&n2.id).unwrap().body.text();
        assert!(t.starts_with("Plans for the weekend") && t.contains("added on A"), "{t}");
    }
    assert_eq!(texts(&a), texts(&b));

    // deleted for good on one side: gone on the other
    a.erase_note(&n1.id).unwrap();
    sync::sync_now(&mut a, &s, Force::None).unwrap();
    let r = sync::sync_now(&mut b, &s, Force::None).unwrap();
    assert_eq!(r.removed, 1, "{r:?}");
    assert!(b.note(&n1.id).is_err());
    // the server keeps a tombstone (so a device that never saw the note learns it too)
    assert!(server_files().iter().any(|f| String::from_utf8_lossy(f).contains(&format!("\"deleted\":true,\"id\":\"{}\"", n1.id))));
    // deleted on one side while edited on the other: the edit wins
    b.erase_note(&n2.id).unwrap();
    let mut e = Body::from_lines("Plans", &["kept by the edit"]);
    a.update_note(&n2.id, &mut e).unwrap();
    sync::sync_now(&mut b, &s, Force::None).unwrap();
    sync::sync_now(&mut a, &s, Force::None).unwrap();
    sync::sync_now(&mut b, &s, Force::None).unwrap();
    assert!(b.note(&n2.id).unwrap().body.text().contains("kept by the edit"));
    assert_eq!(texts(&a), texts(&b));
    // Recently Deleted travels as a folder move
    a.delete_note(&n2.id).unwrap();
    sync::sync_now(&mut a, &s, Force::None).unwrap();
    sync::sync_now(&mut b, &s, Force::None).unwrap();
    assert!(b.note(&n2.id).unwrap().deleted);

    // the status the Settings page shows
    let st = sync::Status::load(&db);
    assert!(!st.failed && st.last_sync.is_some(), "{st:?}");
    nt_core::lock::forget_unlocked();
}

#[test]
fn a_failing_server_changes_nothing_here() {
    let s = Settings { on: true, url: "http://127.0.0.1:9/dav".into(), user: "u".into(), password: "p".into(), folder: "x".into() };
    let d = common::scratch("sync-unreachable");
    let mut st = Store::open(&d).unwrap();
    note_with(&mut st, &["Kept", "here"]);
    assert!(sync::sync_now(&mut st, &s, Force::None).is_err());
    let status = sync::Status::load(&d);
    assert!(status.failed && status.summary.starts_with("Sync failed"));
    assert_eq!(st.library().unwrap().notes.len(), 1);
    // settings are kept readable by the user only
    s.save(&d).unwrap();
    assert_eq!(Settings::load(&d), s);
    #[cfg(unix)]
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(d.join("sync.json")).unwrap().permissions()) & 0o777, 0o600);
}

#[test]
fn native_libraries_sync_as_files() {
    let (_srv, url) = Dav::start();
    let s = settings(&url);
    let dir = common::scratch("sync-native");
    let mut a = Store::open_library(&dir.join("a")).unwrap();
    let mut b = Store::open_library(&dir.join("b")).unwrap();
    let n = note_with(&mut a, &["Groceries", "jelly and bread"]);
    sync::sync_now(&mut a, &s, Force::None).unwrap();
    sync::sync_now(&mut b, &s, Force::None).unwrap();
    // received by sync: in b's index and b's files alike
    assert!(b.note(&n.id).unwrap().body.text().contains("jelly"));
    assert!(dir.join(format!("b/notes/{}.json", n.id)).is_file());
    // erased on b: a tombstone in b's files, then gone from a
    b.erase_note(&n.id).unwrap();
    sync::sync_now(&mut b, &s, Force::None).unwrap();
    sync::sync_now(&mut a, &s, Force::None).unwrap();
    assert!(a.note(&n.id).is_err());
    let t: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join(format!("a/notes/{}.json", n.id))).unwrap()).unwrap();
    assert_eq!(t["deleted"], true);
    drop((a, b));
    // and they stay so when reopened
    let a = Store::open_library(&dir.join("a")).unwrap();
    assert!(a.note(&n.id).is_err());
}

#[test]
fn a_redirecting_server_still_gets_the_password() {
    let (_srv, url) = Dav::start();
    let moved = url.replace("/dav", "/old");
    let dir = common::scratch("sync-redirect");
    let mut a = Store::open(&dir.join("a")).unwrap();
    note_with(&mut a, &["Moved", "to a new address"]);
    let r = sync::sync_now(&mut a, &settings(&moved), Force::None).unwrap();
    assert!(r.sent >= 1);
    // and the other device, at the old address too, gets it
    let mut b = Store::open(&dir.join("b")).unwrap();
    sync::sync_now(&mut b, &settings(&moved), Force::None).unwrap();
    assert_eq!(texts(&b), texts(&a));
    assert!(sync::test_connection(&settings(&moved)).is_none());
}

#[test]
fn network_errors_read_as_such() {
    // nothing listens here
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let dir = common::scratch("sync-no-server");
    let mut a = Store::open(&dir.join("a")).unwrap();
    let e = sync::sync_now(&mut a, &settings(&format!("http://127.0.0.1:{port}/dav")), Force::None).unwrap_err();
    assert!(matches!(e, nt_core::Error::Network(_)), "{e:?}");
    assert!(e.to_string().starts_with("Couldn’t reach the service"), "{e}");
}

/// A library of `n` notes, every third with a picture of its own.
fn library_of(st: &mut Store, dir: &std::path::Path, n: usize) {
    for i in 0..n {
        let note = note_with(st, &[&format!("Note {i}"), &format!("line {i}")]);
        if i % 3 == 0 {
            let p = dir.join(format!("pic{i}.png"));
            std::fs::write(&p, [b"\x89PNG\r\n\x1a\n".as_slice(), format!(" picture {i} {}", "x".repeat(i * 100)).as_bytes()].concat()).unwrap();
            let pic = st.add_attachment(&note.id, &p).unwrap();
            let mut n = st.note(&note.id).unwrap();
            n.body.paragraphs.push(Paragraph { spans: vec![Span::token(&pic.id, &pic.uti)], ..Default::default() });
            st.save_note(&mut n).unwrap();
        }
    }
}

fn pictures(st: &Store) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = st.library().unwrap().notes.iter().flat_map(|n| n.attachments.iter().filter_map(|a| a.file.as_ref().map(|f| std::fs::read(f).unwrap())).collect::<Vec<_>>()).collect();
    v.sort();
    v
}

#[test]
fn a_first_sync_goes_through_a_busy_server() {
    let (srv, url) = Dav::start();
    {
        let mut f = srv.faults.lock().unwrap();
        f.busy_every = 4;
        // (long enough for requests to overlap)
        f.latency = std::time::Duration::from_millis(5);
    }
    let dir = common::scratch("sync-busy");
    let mut a = Store::open(&dir.join("a")).unwrap();
    library_of(&mut a, &dir, 30);
    let seen = Mutex::new(Vec::new());
    let r = sync::sync_now_with(&mut a, &settings(&url), Force::None, &|p| seen.lock().unwrap().push(p.clone())).unwrap();
    assert_eq!(r.sent, 31, "30 notes and the folders");
    let f = srv.faults.lock().unwrap();
    assert!(f.busy_sent > 5, "the server said 429 ({} times)", f.busy_sent);
    assert!(f.most_at_once > 1, "several at once");
    // attachments before every note, folders before both, the marker last
    let written = &f.written;
    let last_file = written.iter().rposition(|p| p.contains("/files/")).unwrap();
    let first_note = written.iter().position(|p| p.contains("/notes/")).unwrap();
    assert!(last_file < first_note, "files first");
    assert!(written.last().unwrap().ends_with("stamp.json"));
    drop(f);
    // progress: each phase counts up to its total
    let seen = seen.into_inner().unwrap();
    let up = seen.iter().filter(|p| p.phase == sync::Phase::Uploading).last().unwrap();
    assert_eq!((up.done, up.total), (40, 40), "10 pictures and 30 notes");
    assert_eq!(up.bytes_done, up.bytes_total);
    assert!(up.message().starts_with("Uploading 40 of 40 · "), "{}", up.message());
    assert!(sync::progress().is_none(), "none once done");

    let mut b = Store::open(&dir.join("b")).unwrap();
    let r = sync::sync_now(&mut b, &settings(&url), Force::None).unwrap();
    assert_eq!(r.received, 31);
    assert_eq!(texts(&b), texts(&a));
    assert_eq!(pictures(&b), pictures(&a));
}

#[test]
fn a_sync_cut_short_goes_on_where_it_stopped() {
    let (srv, url) = Dav::start();
    // meta.json, the 10 pictures, then a few notes, and the server fails
    srv.faults.lock().unwrap().fail_after = Some(16);
    let dir = common::scratch("sync-cut");
    let mut a = Store::open(&dir.join("a")).unwrap();
    library_of(&mut a, &dir, 30);
    assert!(sync::sync_now(&mut a, &settings(&url), Force::None).is_err());
    let first: Vec<String> = srv.faults.lock().unwrap().written.clone();
    assert!(!first.iter().any(|p| p.ends_with("stamp.json")), "no marker: no other device takes it for done");
    assert_eq!(first.iter().filter(|p| p.contains("/files/")).count(), 10);
    let notes_then = first.iter().filter(|p| p.contains("/notes/")).count();
    assert!(notes_then > 0 && notes_then < 30);

    srv.faults.lock().unwrap().fail_after = None;
    // another device meanwhile gets the whole notes there are
    let mut c = Store::open(&dir.join("c")).unwrap();
    sync::sync_now(&mut c, &settings(&url), Force::None).unwrap();
    assert_eq!(c.library().unwrap().notes.len(), notes_then);
    assert_eq!(pictures(&c).len(), c.library().unwrap().notes.iter().filter(|n| !n.attachments.is_empty()).count());

    srv.faults.lock().unwrap().written.clear();
    sync::sync_now(&mut a, &settings(&url), Force::None).unwrap();
    let second: Vec<String> = srv.faults.lock().unwrap().written.clone();
    assert_eq!(second.iter().filter(|p| p.contains("/files/")).count(), 0, "no picture sent twice");
    assert_eq!(second.iter().filter(|p| p.contains("/notes/")).count(), 30 - notes_then, "only the notes left");
    assert!(second.last().unwrap().ends_with("stamp.json"));

    let mut b = Store::open(&dir.join("b")).unwrap();
    sync::sync_now(&mut b, &settings(&url), Force::None).unwrap();
    assert_eq!(texts(&b), texts(&a));
    assert_eq!(pictures(&b), pictures(&a));
    sync::sync_now(&mut c, &settings(&url), Force::None).unwrap();
    assert_eq!(texts(&c), texts(&a));
}

/// A first sync of a real-sized library (NT_BENCH_LIBRARY: a copy, as it's synced) to this
/// server with NT_BENCH_LATENCY_MS added to every request; NT_SYNC_TRANSFERS at once. Timings
/// and counts only. `cargo test --release --test sync_test -- --ignored first_sync_timing --nocapture`
#[test]
#[ignore]
fn first_sync_timing() {
    let Some(lib) = std::env::var_os("NT_BENCH_LIBRARY") else { return };
    let ms: u64 = std::env::var("NT_BENCH_LATENCY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    let (srv, url) = Dav::start();
    srv.faults.lock().unwrap().latency = std::time::Duration::from_millis(ms);
    let mut st = Store::open_library(std::path::Path::new(&lib)).unwrap();
    let t = std::time::Instant::now();
    let last = Mutex::new(sync::Progress::default());
    let r = sync::sync_now_with(&mut st, &settings(&url), Force::None, &|p| {
        if p.phase == sync::Phase::Uploading {
            *last.lock().unwrap() = p.clone();
        }
    })
    .unwrap();
    let secs = t.elapsed().as_secs_f64();
    let f = srv.faults.lock().unwrap();
    let bytes: usize = srv.files.lock().unwrap().values().map(Vec::len).sum();
    println!(
        "transfers {:?}, latency {ms} ms: {secs:.1} s; {} sent; {} requests ({} writes), {} at most at once; {:.0} MB on the server; last progress: {}",
        std::env::var("NT_SYNC_TRANSFERS").ok(),
        r.sent,
        srv.requests(),
        f.writes,
        f.most_at_once,
        bytes as f64 / 1e6,
        last.lock().unwrap().message()
    );
}

/// The time a transfer may go quiet, for one test at a time (it's the process's): back to the
/// usual once the test is done.
fn idle(secs: u64) -> impl Drop {
    static ONE: Mutex<()> = Mutex::new(());
    struct Back(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);
    impl Drop for Back {
        fn drop(&mut self) {
            sync::set_idle_timeout(std::time::Duration::from_secs(120));
        }
    }
    let g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    sync::set_idle_timeout(std::time::Duration::from_secs(secs));
    Back(g)
}

/// `library_of` plus one note with a big picture; its server path and content.
fn library_with_big(st: &mut Store, dir: &std::path::Path, size: usize) -> (String, Vec<u8>) {
    library_of(st, dir, 12);
    let big: Vec<u8> = [b"\x89PNG\r\n\x1a\n".as_slice(), &(0..size).map(|i| (i * 31 % 251) as u8).collect::<Vec<u8>>()].concat();
    let p = dir.join("big.png");
    std::fs::write(&p, &big).unwrap();
    let note = note_with(st, &["Big", "picture"]);
    let pic = st.add_attachment(&note.id, &p).unwrap();
    let mut n = st.note(&note.id).unwrap();
    n.body.paragraphs.push(Paragraph { spans: vec![Span::token(&pic.id, &pic.uti)], ..Default::default() });
    st.save_note(&mut n).unwrap();
    (format!("/dav/Bramble Notes/files/{}", sync::content_hash(&big)), big)
}

#[test]
fn a_slow_download_that_keeps_moving_never_times_out() {
    let _quiet = idle(1);
    let (srv, url) = Dav::start();
    let dir = common::scratch("sync-slow");
    let mut a = Store::open(&dir.join("a")).unwrap();
    library_with_big(&mut a, &dir, 1_000_000);
    sync::sync_now(&mut a, &settings(&url), Force::None).unwrap();
    // 200 KB a second: the big picture takes 5 s, five times the time allowed to go quiet
    srv.faults.lock().unwrap().rate = Some(200_000);
    let mut b = Store::open(&dir.join("b")).unwrap();
    let t = std::time::Instant::now();
    let r = sync::sync_now(&mut b, &settings(&url), Force::None).unwrap();
    assert!(t.elapsed().as_secs_f64() > 4.0);
    assert_eq!((r.received, r.failed), (14, 0));
    assert_eq!(pictures(&b), pictures(&a));
}

#[test]
fn a_download_that_stalls_goes_on_from_where_it_stopped() {
    let _quiet = idle(1);
    let (srv, url) = Dav::start();
    let dir = common::scratch("sync-stall");
    let mut a = Store::open(&dir.join("a")).unwrap();
    let (path, _) = library_with_big(&mut a, &dir, 2_000_000);
    sync::sync_now(&mut a, &settings(&url), Force::None).unwrap();
    // half of it, then nothing for 3 s, then the connection drops
    srv.faults.lock().unwrap().stall.insert(path, std::time::Duration::from_secs(3));
    let mut b = Store::open(&dir.join("b")).unwrap();
    let r = sync::sync_now(&mut b, &settings(&url), Force::None).unwrap();
    assert_eq!((r.received, r.failed), (14, 0));
    assert_eq!(srv.faults.lock().unwrap().ranged, 1, "the rest asked for, not all of it again");
    assert_eq!(pictures(&b), pictures(&a));
}

#[test]
fn one_bad_file_doesnt_stop_the_rest() {
    let (srv, url) = Dav::start();
    let dir = common::scratch("sync-damaged");
    let mut a = Store::open(&dir.join("a")).unwrap();
    let (path, _) = library_with_big(&mut a, &dir, 1000);
    sync::sync_now(&mut a, &settings(&url), Force::None).unwrap();
    srv.faults.lock().unwrap().damaged.insert(path.clone());
    let mut b = Store::open(&dir.join("b")).unwrap();
    let r = sync::sync_now(&mut b, &settings(&url), Force::None).unwrap();
    assert_eq!((r.received, r.failed), (13, 1), "all but the note with the damaged picture");
    let status = sync::Status::load(&dir.join("b"));
    assert!(status.failed && status.summary.contains("couldn’t be synced"), "{}", status.summary);
    assert!(!b.library().unwrap().notes.iter().any(|n| n.title == "Big"));
    // mended on the server: the next sync brings it
    srv.faults.lock().unwrap().damaged.clear();
    let r = sync::sync_now(&mut b, &settings(&url), Force::None).unwrap();
    assert_eq!((r.received, r.failed), (1, 0));
    assert_eq!(texts(&b), texts(&a));
    assert_eq!(pictures(&b), pictures(&a));
}

#[test]
fn a_first_download_cut_short_goes_on_where_it_stopped() {
    let (srv, url) = Dav::start();
    let dir = common::scratch("sync-cut-down");
    let mut a = Store::open(&dir.join("a")).unwrap();
    library_with_big(&mut a, &dir, 1000);
    sync::sync_now(&mut a, &settings(&url), Force::None).unwrap();
    // 5 pictures (of 5) read, then the server fails every read
    srv.faults.lock().unwrap().fail_reads_after = Some(2);
    let mut b = Store::open(&dir.join("b")).unwrap();
    let r = sync::sync_now(&mut b, &settings(&url), Force::None).unwrap();
    assert_eq!(r.failed, 3, "3 notes wait for their pictures");
    {
        let mut f = srv.faults.lock().unwrap();
        f.fail_reads_after = None;
        f.reads = 0;
    }
    let r = sync::sync_now(&mut b, &settings(&url), Force::None).unwrap();
    assert_eq!((r.received, r.failed), (3, 0));
    assert_eq!(srv.faults.lock().unwrap().reads, 3, "only the pictures not read before");
    assert_eq!(texts(&b), texts(&a));
    assert_eq!(pictures(&b), pictures(&a));
}

#[test]
fn an_empty_meta_json_on_the_server_is_taken_as_missing() {
    let (srv, url) = Dav::start();
    let dir = common::scratch("sync-empty-meta");
    let mut a = Store::open(&dir.join("a")).unwrap();
    note_with(&mut a, &["Kept", "here"]);
    sync::sync_now(&mut a, &settings(&url), Force::None).unwrap();
    srv.files.lock().unwrap().insert("/dav/Bramble Notes/meta.json".into(), Vec::new());
    let mut b = Store::open(&dir.join("b")).unwrap();
    sync::sync_now(&mut b, &settings(&url), Force::None).unwrap();
    assert_eq!(texts(&b), texts(&a));
}

#[test]
fn progress_moves_within_a_big_file() {
    let _quiet = idle(30);
    let (srv, url) = Dav::start();
    let dir = common::scratch("sync-progress-bytes");
    let mut a = Store::open(&dir.join("a")).unwrap();
    library_with_big(&mut a, &dir, 1_500_000);
    // taken in and read out at 500 KB a second: 3 s each way for the big picture
    {
        let mut f = srv.faults.lock().unwrap();
        f.write_rate = Some(500_000);
        f.rate = Some(500_000);
    }
    let steps = |p: &[sync::Progress], phase: sync::Phase| -> Vec<u64> {
        // the bytes done while a file was still on its way (fewer files done than there are)
        let mut v: Vec<u64> = p.iter().filter(|p| p.phase == phase && p.bytes_total > 1_000_000 && p.done < p.total).map(|p| p.bytes_done).collect();
        v.dedup();
        v
    };
    let seen = Mutex::new(Vec::new());
    let ra = sync::sync_now_with(&mut a, &settings(&url), Force::None, &|p| seen.lock().unwrap().push(p.clone())).unwrap();
    assert_eq!(ra.failed, 0);
    let up = steps(&seen.lock().unwrap(), sync::Phase::Uploading);
    assert!(up.len() >= 8, "the upload's bar moved {} times", up.len());
    let mut b = Store::open(&dir.join("b")).unwrap();
    let seen = Mutex::new(Vec::new());
    sync::sync_now_with(&mut b, &settings(&url), Force::None, &|p| seen.lock().unwrap().push(p.clone())).unwrap();
    let seen = seen.into_inner().unwrap();
    let down = steps(&seen, sync::Phase::Downloading);
    assert!(down.len() >= 5, "the download's bar moved {} times", down.len());
    assert!(down.windows(2).all(|w| w[0] <= w[1]), "never back");
    let last = seen.iter().filter(|p| p.phase == sync::Phase::Downloading).last().unwrap();
    assert_eq!(last.bytes_done, last.bytes_total);
    assert_eq!(pictures(&b), pictures(&a));
}
