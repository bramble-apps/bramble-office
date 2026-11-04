//! Sync: the library kept on a WebDAV service (Nextcloud, Koofr, a NAS…) and merged note by note
//! with every device using the same account. The design of ChordPro Presenter's sync
//! (bilingual_projector_web/lib/cloud_sync.dart). On the server, in the chosen folder:
//!
//! ```text
//! notes/<id>.json      {"v": 1, "note": {…}}   one per note: its body (Notes' protobuf, base64),
//!                      dates, folder, pin, attachments (tables, drawings…); a locked note's
//!                      encrypted rows as they are
//! files/<sha256>       attachment files, named by their content (they never change)
//! meta.json            {"folders": [...]} folders and Smart Folders
//! stamp.json           changed by every sync that changes something, so the regular checks read
//!                      just this when all's quiet
//! ```
//!
//! What was last in step is kept per note (this library's change key, the note's hash and the
//! server's ETag; `sync-state.json` in the library folder), so each side's changes and deletions
//! are told apart; when both changed the same note, the newer edit wins and the other is kept as a
//! copy ("(other copy)"); an edit wins over a deletion.
//!
//! The shell runs [`sync_now`] on a worker thread with its own [`Store`] (a Store is Send, not Sync),
//! a few seconds after changes and on a timer; [`Settings`] and [`Status`] are for its Settings page.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use sha2::Digest;

use crate::lock::{base64, unbase64};
use crate::model::*;
use crate::store::Store;
use crate::{body, paper, table, Error, Result};

// ------------------------------------------------------------------ settings

/// Ready-made WebDAV addresses (the user fills in their part).
pub const PRESETS: &[(&str, &str)] = &[
    ("Nextcloud", "https://YOUR-SERVER/remote.php/dav/files/YOUR-USERNAME"),
    ("Koofr", "https://app.koofr.net/dav/Koofr"),
    ("NAS (Synology)", "https://YOUR-NAS:5006/YOUR-SHARED-FOLDER"),
    ("NAS (QNAP)", "https://YOUR-NAS/webdav/YOUR-SHARED-FOLDER"),
];

/// The sync account (kept in `sync.json` in the library folder, readable by the user only).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub on: bool,
    pub url: String,
    pub user: String,
    pub password: String,
    /// the folder on the service the library goes in
    pub folder: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { on: false, url: String::new(), user: String::new(), password: String::new(), folder: "Bramble Notes".into() }
    }
}

impl Settings {
    pub fn ready(&self) -> bool {
        !self.url.trim().is_empty() && !self.user.trim().is_empty()
    }

    pub fn load(library: &Path) -> Settings {
        let Ok(j) = std::fs::read(library.join("sync.json")).map_err(|_| ()).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|_| ())) else {
            return Settings::default();
        };
        let s = |k: &str| j.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let d = Settings::default();
        Settings { on: j.get("on").and_then(|v| v.as_bool()).unwrap_or(false), url: s("url"), user: s("user"), password: s("password"), folder: Some(s("folder")).filter(|f| !f.is_empty()).unwrap_or(d.folder) }
    }

    pub fn save(&self, library: &Path) -> Result<()> {
        let j = json!({"on": self.on, "url": self.url, "user": self.user, "password": self.password, "folder": self.folder});
        write_private(&library.join("sync.json"), j.to_string().as_bytes())
    }

    fn folder(&self) -> String {
        let f = self.folder.trim().trim_matches('/');
        if f.is_empty() { "Bramble Notes".into() } else { f.to_string() }
    }
}

pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    std::io::Write::write_all(&mut o.open(&tmp)?, bytes)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// What happened last (for the Settings page): kept in `sync-status.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// seconds since 1970 of the last sync that finished
    pub last_sync: Option<f64>,
    /// "Up to date", "3 sent, 1 received", or the error
    pub summary: String,
    pub failed: bool,
}

impl Status {
    pub fn load(library: &Path) -> Status {
        let Ok(j) = std::fs::read(library.join("sync-status.json")).map_err(|_| ()).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|_| ())) else {
            return Status::default();
        };
        Status { last_sync: j.get("last").and_then(|v| v.as_f64()), summary: j.get("summary").and_then(|v| v.as_str()).unwrap_or_default().into(), failed: j.get("failed").and_then(|v| v.as_bool()).unwrap_or(false) }
    }

    fn save(&self, library: &Path) {
        let _ = write_private(&library.join("sync-status.json"), json!({"last": self.last_sync, "summary": self.summary, "failed": self.failed}).to_string().as_bytes());
    }
}

// ------------------------------------------------------------------ WebDAV

struct Response {
    status: u16,
    etag: Option<String>,
    body: Vec<u8>,
    /// how long the service asked us to wait (429, 503)
    retry_after: Option<Duration>,
}

/// Why a request failed: worth trying again (a dropped connection, a timeout), or not.
enum Failure {
    Busy(Error),
    Final(Error),
}
use Failure::{Busy, Final};

fn failure(e: ureq::Error) -> Failure {
    let text = e.to_string();
    let again = match &e {
        ureq::Error::Timeout(_) => true,
        ureq::Error::Io(io) => matches!(
            io.kind(),
            std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted
        ),
        _ => false,
    };
    let e = Error::Network(if text.contains("invalid peer certificate") {
        format!("The server’s certificate isn’t trusted on this device ({}). Add its certificate authority to the system’s certificates, or use an address with a trusted certificate.", text.rsplit(": ").next().unwrap_or(&text))
    } else {
        format!("Couldn’t reach the service ({text}).")
    });
    if again { Busy(e) } else { Final(e) }
}

/// How long before try `n + 1`: 0.5 s doubling, up to 30 s, give or take half (so workers that
/// were slowed together don't all come back at once).
fn backoff(n: u32) -> Duration {
    let base = (0.5 * f64::from(1u32 << n.min(6))).min(30.0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let jitter = 0.5 + f64::from(nanos % 1000) / 1000.0;
    Duration::from_secs_f64(base * jitter)
}

/// How long a transfer may go without a byte moving before it's given up (and tried again):
/// 120 s, room for a slow uplink to drain before the server answers (NT_SYNC_IDLE_S overrides,
/// for testing).
fn idle_timeout() -> Duration {
    match IDLE_MS.load(std::sync::atomic::Ordering::Relaxed) {
        0 => Duration::from_secs_f64(std::env::var("NT_SYNC_IDLE_S").ok().and_then(|v| v.parse().ok()).filter(|s: &f64| *s > 0.0).unwrap_or(120.0)),
        ms => Duration::from_millis(ms),
    }
}

static IDLE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How long a transfer may go quiet before it's tried again (for tests; 120 s otherwise).
pub fn set_idle_timeout(t: Duration) {
    IDLE_MS.store(t.as_millis().max(1) as u64, std::sync::atomic::Ordering::Relaxed);
}

/// Transfers at once (NT_SYNC_TRANSFERS overrides, for measuring).
fn transfers() -> usize {
    std::env::var("NT_SYNC_TRANSFERS").ok().and_then(|v| v.parse().ok()).filter(|n| *n >= 1).unwrap_or(MAX_TRANSFERS).min(16)
}

/// A WebDAV folder: list, read, write and delete files under `base`. Shared by the transfer
/// workers (its agent keeps their connections open).
pub(crate) struct Dav {
    base: String,
    auth: String,
    agent: ureq::Agent,
    /// how many transfers may run at once: 6, down to 2 and then 1 when the service asks us to slow
    /// down (429, 503, timeouts) and stays there for the rest of the sync
    lanes: std::sync::atomic::AtomicUsize,
    /// times it asked
    throttled: std::sync::atomic::AtomicUsize,
}

/// At most this many transfers at once.
pub const MAX_TRANSFERS: usize = 6;

/// Tries of one request before giving up (waiting longer each time).
const TRIES: u32 = 6;

/// What a request sends: bytes, or a file read as it goes (attachments are never held whole).
#[derive(Clone, Copy)]
enum Payload<'a> {
    Bytes(&'a [u8]),
    File(&'a Path),
}

/// Told the bytes a transfer has moved since it last said (for the progress bar during a big file).
type Moved<'a> = &'a (dyn Fn(u64) + Sync);

/// Told nothing.
fn unwatched(_: u64) {}

/// How often, in bytes, a transfer says how far it is.
const MOVED_EVERY: u64 = 256 * 1024;

/// An entry of a folder listing: (name, is a folder, ETag, size).
type Entry = (String, bool, String, u64);

/// Where a redirect from `from` to `location` goes, when the password may follow it: the same host
/// and port, and not from https to http.
fn redirect_target(from: &str, location: &str) -> Option<String> {
    let split = |u: &str| -> Option<(String, String, String)> {
        let (scheme, rest) = u.split_once("://")?;
        let (host, path) = rest.find('/').map(|i| (&rest[..i], &rest[i..])).unwrap_or((rest, "/"));
        let port = if host.contains(':') { String::new() } else if scheme.eq_ignore_ascii_case("https") { ":443".into() } else { ":80".into() };
        Some((scheme.to_ascii_lowercase(), format!("{}{port}", host.to_ascii_lowercase()), path.to_string()))
    };
    let (scheme, host, _) = split(from)?;
    if location.starts_with('/') && !location.starts_with("//") {
        let authority = from.split_once("://")?.1.split('/').next()?;
        return Some(format!("{scheme}://{authority}{location}"));
    }
    let (to_scheme, to_host, _) = split(location)?;
    // (http -> https keeps the default port's change: a host's :80 and :443 are the same server)
    let same_host = to_host == host || (scheme == "http" && to_scheme == "https" && to_host.trim_end_matches(":443") == host.trim_end_matches(":80"));
    (same_host && !(scheme == "https" && to_scheme == "http")).then(|| location.to_string())
}

/// Whether TLS checks servers' certificates against the system's store (and the user's own CAs,
/// so a NAS with its own certificate works) rather than the built-in list. Android needs the
/// platform verifier set up over JNI first; nt-ffi turns it on with [`use_system_certificates`].
static SYSTEM_CERTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(!cfg!(target_os = "android"));

/// Check servers' certificates against the system's store from now on (Android: once the
/// platform verifier is initialised).
pub fn use_system_certificates(on: bool) {
    SYSTEM_CERTS.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn root_certs() -> ureq::tls::RootCerts {
    if SYSTEM_CERTS.load(std::sync::atomic::Ordering::Relaxed) {
        ureq::tls::RootCerts::PlatformVerifier
    } else {
        ureq::tls::RootCerts::WebPki
    }
}

impl Dav {
    pub(crate) fn new(s: &Settings) -> Dav {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .allow_non_standard_methods(true)
            // no cap on a whole transfer (a 20 MB attachment over a phone takes minutes): only on
            // reaching the server, and on going quiet
            .timeout_resolve(Some(Duration::from_secs(15)))
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_per_read(Some(idle_timeout()))
            .timeout_per_write(Some(idle_timeout()))
            // (redirects are followed in `send`: ureq won't for WebDAV's methods)
            .max_redirects(0)
            .tls_config(ureq::tls::TlsConfig::builder().root_certs(root_certs()).build())
            .build()
            .into();
        let base = s.url.trim().trim_end_matches('/').to_string();
        Dav {
            base,
            auth: format!("Basic {}", base64(format!("{}:{}", s.user.trim(), s.password).as_bytes())),
            agent,
            lanes: std::sync::atomic::AtomicUsize::new(transfers()),
            throttled: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// The service asked us to slow down: fewer transfers at once from now on (6 → 2 → 1).
    fn slow_down(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        self.throttled.fetch_add(1, Relaxed);
        let n = self.lanes.load(Relaxed);
        self.lanes.store(if n > 2 { 2 } else { 1 }, Relaxed);
    }

    fn url(&self, path: &str) -> String {
        let mut u = self.base.clone();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            u.push('/');
            for b in part.bytes() {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    u.push(b as char);
                } else {
                    u.push_str(&format!("%{b:02X}"));
                }
            }
        }
        if path.ends_with('/') {
            u.push('/');
        }
        u
    }

    fn send(&self, method: &str, path: &str, headers: &[(&str, &str)], body: Vec<u8>) -> Result<Response> {
        self.request(method, path, headers, Payload::Bytes(&body), None, &unwatched)
    }

    /// One request, tried again (waiting longer each time, as the service's Retry-After asks) when
    /// the service is busy or the connection drops. With `sink`, the answer's body is written there
    /// as it comes instead of being kept.
    fn request(&self, method: &str, path: &str, headers: &[(&str, &str)], body: Payload, sink: Option<&Path>, moved: Moved) -> Result<Response> {
        let mut try_no = 0;
        loop {
            try_no += 1;
            let (r, wait) = match self.request_once(method, path, headers, body, sink, moved) {
                Ok(r) if matches!(r.status, 429 | 502 | 503 | 504) => {
                    let wait = r.retry_after;
                    (Ok(r), wait)
                }
                Err(Busy(e)) => (Err(e), None),
                Ok(r) => return Ok(r),
                Err(Final(e)) => return Err(e),
            };
            if try_no >= TRIES {
                return match r {
                    Ok(r) => Err(Error::Network(format!("The service is busy ({}); sync will try again later.", r.status))),
                    Err(e) => Err(e),
                };
            }
            self.slow_down();
            std::thread::sleep(wait.unwrap_or_else(|| backoff(try_no)));
        }
    }

    fn request_once(&self, method: &str, path: &str, headers: &[(&str, &str)], body: Payload, sink: Option<&Path>, moved: Moved) -> std::result::Result<Response, Failure> {
        let mut url = self.url(path);
        let mut method = method;
        let mut hops = 0;
        let bad = |e: String| Final(Error::Format(e));
        // a download cut short goes on from where it stopped
        let part = sink.map(|p| p.with_extension("part"));
        let have = part.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).filter(|n| *n > 0 && method == "GET");
        let range = have.map(|n| format!("bytes={n}-"));
        let mut r = loop {
            let mut req = ureq::http::Request::builder()
                .method(ureq::http::Method::from_bytes(method.as_bytes()).map_err(|e| bad(e.to_string()))?)
                .uri(&url)
                .header("Authorization", &self.auth);
            for (k, v) in headers {
                req = req.header(*k, *v);
            }
            if let Some(r) = &range {
                req = req.header("Range", r.as_str());
            }
            let sent = match body {
                Payload::Bytes(b) => self.agent.run(req.body(b).map_err(|e| bad(e.to_string()))?),
                Payload::File(p) => {
                    let f = std::fs::File::open(p).map_err(|e| Final(e.into()))?;
                    // how far ureq has read it: a second handle shares the file's position
                    let watch = f.try_clone().ok();
                    let req = req.body(f).map_err(|e| bad(e.to_string()))?;
                    let done = std::sync::atomic::AtomicBool::new(false);
                    std::thread::scope(|sc| {
                        if let Some(mut w) = watch {
                            let done = &done;
                            sc.spawn(move || {
                                let mut told = 0;
                                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                                    std::thread::sleep(Duration::from_millis(200));
                                    let at = std::io::Seek::stream_position(&mut w).unwrap_or(told);
                                    if at > told {
                                        moved(at - told);
                                        told = at;
                                    }
                                }
                            });
                        }
                        let r = self.agent.run(req);
                        done.store(true, std::sync::atomic::Ordering::Relaxed);
                        r
                    })
                }
            };
            let r = sent.map_err(failure)?;
            // a server that moved (to https, or to the folder with a slash): followed with the same
            // method and the password, on the same host only and never from https to http
            let to = r.headers().get("location").and_then(|v| v.to_str().ok()).and_then(|l| redirect_target(&url, l));
            match (r.status().as_u16(), to) {
                (301 | 302 | 303 | 307 | 308, Some(to)) if hops < 5 => {
                    if r.status().as_u16() == 303 {
                        method = "GET";
                    }
                    url = to;
                    hops += 1;
                }
                (301 | 302 | 303 | 307 | 308, None) => {
                    return Err(Final(Error::Network("The service sent sync to another server (use its address in Settings).".into())));
                }
                _ => break r,
            }
        };
        let status = r.status().as_u16();
        if status == 401 || status == 403 {
            return Err(Final(Error::Network("The service refused the user name or password (use an app password where the service has them).".into())));
        }
        let etag = r.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
        let retry_after = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok()).map(|s| Duration::from_secs(s.min(60)));
        if status == 416 {
            // what we had of it is no use: from the start, next try
            if let Some(part) = &part {
                let _ = std::fs::remove_file(part);
            }
            return Err(Busy(Error::Network(format!("Reading {path} failed (416)."))));
        }
        let body = match (sink, &part) {
            (Some(p), Some(part)) if (200..300).contains(&status) => {
                // streamed to a part file, renamed once whole; a dropped connection is tried again
                // from where it stopped (206: the rest; 200: the server sends it all again)
                let mut out = if status == 206 && have.is_some() {
                    std::fs::OpenOptions::new().append(true).open(part)
                } else {
                    std::fs::File::create(part)
                }
                .map_err(|e| Final(e.into()))?;
                let lost = |e: std::io::Error| Busy(Error::Network(format!("Couldn’t reach the service ({e}).")));
                let mut reader = r.body_mut().as_reader();
                let mut buf = vec![0u8; 64 * 1024];
                let mut untold = 0;
                loop {
                    let n = std::io::Read::read(&mut reader, &mut buf).map_err(lost)?;
                    if n == 0 {
                        break;
                    }
                    std::io::Write::write_all(&mut out, &buf[..n]).map_err(|e| Final(e.into()))?;
                    untold += n as u64;
                    if untold >= MOVED_EVERY {
                        moved(untold);
                        untold = 0;
                    }
                }
                moved(untold);
                drop(out);
                std::fs::rename(part, p).map_err(|e| Final(e.into()))?;
                Vec::new()
            }
            _ => r.body_mut().with_config().limit(1 << 31).read_to_vec().map_err(|e| Busy(Error::Network(format!("Couldn’t reach the service ({e}).")))).unwrap_or_default(),
        };
        Ok(Response { status, etag, body, retry_after })
    }

    /// The files and folders in `path` (None: it doesn't exist).
    fn list(&self, path: &str) -> Result<Option<Vec<Entry>>> {
        let q = br#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"><d:prop><d:getetag/><d:getlastmodified/><d:getcontentlength/><d:resourcetype/></d:prop></d:propfind>"#;
        let r = self.send("PROPFIND", &format!("{path}/"), &[("Depth", "1"), ("Content-Type", "application/xml")], q.to_vec())?;
        if r.status == 404 {
            return Ok(None);
        }
        if r.status != 207 && !(200..300).contains(&r.status) {
            return Err(Error::Network(format!("Listing {path} failed ({}).", r.status)));
        }
        let folder = url_path(&self.url(&format!("{path}/")));
        Ok(Some(parse_propfind(&String::from_utf8_lossy(&r.body), &folder)))
    }

    fn get(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let r = self.send("GET", path, &[], Vec::new())?;
        match r.status {
            404 => Ok(None),
            200..=299 => Ok(Some(r.body)),
            s => Err(Error::Network(format!("Reading {path} failed ({s})."))),
        }
    }

    /// Write `path`; its new ETag when the server says.
    fn put(&self, path: &str, bytes: Vec<u8>) -> Result<Option<String>> {
        self.put_payload(path, Payload::Bytes(&bytes), &unwatched)
    }

    /// Write `path` from a local file, read as it's sent.
    fn put_file(&self, path: &str, from: &Path, moved: Moved) -> Result<Option<String>> {
        self.put_payload(path, Payload::File(from), moved)
    }

    fn put_payload(&self, path: &str, body: Payload, moved: Moved) -> Result<Option<String>> {
        let r = self.request("PUT", path, &[("Content-Type", "application/octet-stream")], body, None, moved)?;
        if !(200..300).contains(&r.status) {
            return Err(Error::Network(format!("Saving {path} failed ({}).", r.status)));
        }
        Ok(r.etag)
    }

    /// Read `path` into the file `to`, as it comes (false: it isn't there).
    fn get_to(&self, path: &str, to: &Path, moved: Moved) -> Result<bool> {
        let r = self.request("GET", path, &[], Payload::Bytes(&[]), Some(to), moved)?;
        match r.status {
            404 => Ok(false),
            200..=299 => Ok(true),
            s => Err(Error::Network(format!("Reading {path} failed ({s})."))),
        }
    }


    /// Make folder `path` (and its parents) when missing.
    fn make_folders(&self, path: &str) -> Result<()> {
        let mut done = String::new();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            done.push('/');
            done.push_str(part);
            let r = self.send("MKCOL", &done, &[], Vec::new())?;
            // 201 made; 405 already there; some servers answer 301 / 409
            if !(200..300).contains(&r.status) && ![405, 301, 409].contains(&r.status) {
                return Err(Error::Network(format!("Making the folder {done} failed ({}).", r.status)));
            }
        }
        Ok(())
    }
}

fn url_path(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.find('/').map(|i| rest[i..].to_string()).unwrap_or_else(|| "/".into())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The text of the first `<x:tag>…</x:tag>` (any namespace prefix) in `block`.
fn tag_text<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let lower = block.to_ascii_lowercase();
    let open = find_tag(&lower, 0, tag, false)?;
    let end = lower[open..].find('>')? + open;
    if lower[..end].ends_with('/') {
        return Some("");
    }
    let close = find_tag(&lower, end, tag, true)?;
    Some(&block[end + 1..close])
}

/// The entries of a PROPFIND answer for folder `folder` (its path; the folder itself left out).
fn parse_propfind(xml: &str, folder: &str) -> Vec<Entry> {
    let clean = |p: &str| percent_decode(p).trim_end_matches('/').to_string();
    let lower = xml.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    loop {
        // each <x:response> … </x:response>
        let Some(open) = find_tag(&lower, from, "response", false) else { break };
        let Some(close) = find_tag(&lower, open, "response", true) else { break };
        let block = &xml[open..close];
        from = close + 1;
        let Some(href) = tag_text(block, "href").map(|h| h.trim().to_string()) else { continue };
        let path = if href.contains("://") { url_path(&href) } else { href };
        if clean(&path) == clean(folder) {
            continue;
        }
        let name = clean(&path).rsplit('/').next().unwrap_or("").to_string();
        let is_dir = find_tag(&block.to_ascii_lowercase(), 0, "collection", false).is_some();
        let etag = tag_text(block, "getetag")
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| format!("{}|{}", tag_text(block, "getlastmodified").unwrap_or(""), tag_text(block, "getcontentlength").unwrap_or("")));
        let size = tag_text(block, "getcontentlength").and_then(|t| t.trim().parse().ok()).unwrap_or(0);
        out.push((name, is_dir, etag.replace("&quot;", "\""), size));
    }
    out
}

/// Where `<x:tag` (or `</x:tag`) starts in `lower` at or after `from`.
fn find_tag(lower: &str, from: usize, tag: &str, closing: bool) -> Option<usize> {
    let mut i = from;
    while let Some(p) = lower[i..].find('<').map(|p| p + i) {
        let rest = &lower[p + 1..];
        let rest = if closing { rest.strip_prefix('/') } else { Some(rest).filter(|r| !r.starts_with('/')) };
        if let Some(r) = rest {
            let name: String = r.chars().take_while(|c| !c.is_whitespace() && *c != '>' && *c != '/').collect();
            if name.rsplit(':').next() == Some(tag) {
                return Some(p);
            }
        }
        i = p + 1;
    }
    None
}

/// Try the address and sign-in (the folder is made when missing); None when fine, else what's wrong.
pub fn test_connection(s: &Settings) -> Option<String> {
    if !s.ready() {
        return Some("Fill in the address and user name.".into());
    }
    let dav = Dav::new(s);
    dav.make_folders(&s.folder()).and_then(|_| dav.list(&s.folder())).err().map(|e| e.to_string())
}

// ------------------------------------------------------------------ notes as JSON

/// A short fingerprint (to notice changes).
fn fingerprint(b: &[u8]) -> String {
    let h = sha2::Sha256::digest(b);
    h[..12].iter().map(|b| format!("{b:02x}")).collect()
}

/// The content hash files/<…> are named by (SHA-256, hex).
pub fn content_hash(b: &[u8]) -> String {
    sha_hex(b)
}

pub(crate) fn sha_hex(b: &[u8]) -> String {
    sha2::Sha256::digest(b).iter().map(|b| format!("{b:02x}")).collect()
}

/// Files a note's JSON refers to: content hash → where the bytes are here.
pub(crate) type Files = HashMap<String, PathBuf>;

/// A file's content hash (reads it; the native library keeps them per path).
pub(crate) type Hasher<'a> = dyn FnMut(&Path) -> Option<String> + 'a;

pub(crate) fn hash_file(p: &Path) -> Option<String> {
    std::fs::read(p).ok().map(|b| sha_hex(&b))
}

fn file_ref(p: &Option<PathBuf>, files: &mut Files, hash: &mut Hasher) -> Value {
    match p.as_ref().and_then(|p| hash(p).map(|h| (p, h))) {
        Some((p, h)) => {
            files.insert(h.clone(), p.clone());
            json!({"sha": h, "name": p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()})
        }
        None => Value::Null,
    }
}

fn b64o(b: &Option<Vec<u8>>) -> Value {
    b.as_ref().map(|b| Value::String(base64(b))).unwrap_or(Value::Null)
}

fn crypto_json(c: &Crypto) -> Value {
    json!({"salt": b64o(&c.salt), "iterations": c.iterations, "wrapped": b64o(&c.wrapped_key), "verifier": b64o(&c.verifier), "iv": b64o(&c.iv), "tag": b64o(&c.tag), "values": b64o(&c.values)})
}

fn attachment_json(a: &Attachment, files: &mut Files, hash: &mut Hasher) -> Value {
    json!({
        "id": a.id, "uti": a.uti, "name": a.name, "width": a.width, "height": a.height, "url": a.url, "summary": a.summary,
        "created": a.created, "modified": a.modified, "view": format!("{:?}", a.view_size), "notDownloaded": a.not_downloaded,
        "table": a.table.as_ref().map(|t| base64(&table::encode_table(t))),
        "drawing": a.drawing.as_ref().map(|d| base64(&paper::encode_ink(d))),
        "pages": a.pages.iter().map(|p| json!([p.index, p.width, p.height, p.crop])).collect::<Vec<_>>(),
        "file": file_ref(&a.file, files, hash),
        "fallback": file_ref(&a.fallback, files, hash),
        "parts": a.parts.iter().map(|p| attachment_json(p, files, hash)).collect::<Vec<_>>(),
    })
}

/// A note as the server keeps it, and the files it needs.
pub(crate) fn note_json(n: &Note) -> (Value, Files) {
    note_json_with(n, &mut hash_file)
}

/// [`note_json`] with the files' hashes from `hash`.
pub(crate) fn note_json_with(n: &Note, hash: &mut Hasher) -> (Value, Files) {
    let mut files = Files::new();
    let mut j = json!({
        "id": n.id, "folder": n.folder, "created": n.created, "modified": n.modified, "pinned": n.pinned,
        "deleted": n.deleted, "title": n.title, "snippet": n.snippet, "locked": n.locked,
    });
    if let Some(s) = n.sealed.as_ref().filter(|_| n.locked && n.lock.is_none()) {
        let mut sealed_files = Vec::new();
        for (src, rel) in &s.files {
            // a folder (a media or fallback folder): every file in it, by its path inside
            for (f, inner) in walk(src) {
                if let Some(h) = hash(&f) {
                    files.insert(h.clone(), f.clone());
                    sealed_files.push(json!({"sha": h, "path": rel.join(inner).to_string_lossy()}));
                }
            }
        }
        j["sealed"] = json!({
            "crypto": crypto_json(&s.crypto), "hint": s.hint, "data": base64(&s.data), "iv": b64o(&s.data_iv), "tag": b64o(&s.data_tag),
            "attachments": s.attachments.iter().map(|a| json!({
                "id": a.id, "uti": a.uti, "parent": a.parent, "width": a.width, "height": a.height, "created": a.created, "modified": a.modified,
                "fallbackImage": a.fallback_image, "fallbackPdf": a.fallback_pdf, "fallbackImageIv": b64o(&a.fallback_image_iv),
                "fallbackImageTag": b64o(&a.fallback_image_tag), "fallbackPdfIv": b64o(&a.fallback_pdf_iv), "fallbackPdfTag": b64o(&a.fallback_pdf_tag),
                "crypto": crypto_json(&a.crypto),
                "media": a.media.as_ref().map(|m| json!({"id": m.id, "generation": m.generation, "assetIv": b64o(&m.asset_iv), "assetTag": b64o(&m.asset_tag), "crypto": crypto_json(&m.crypto)})),
            })).collect::<Vec<_>>(),
            "inline": s.inline.iter().map(|t| json!({"id": t.id, "uti": t.uti, "crypto": crypto_json(&t.crypto)})).collect::<Vec<_>>(),
            "files": sealed_files,
        });
    } else {
        j["body"] = Value::String(base64(&body::encode_note(&n.body)));
        j["outline"] = b64o(&body::encode_outline_state(&n.body));
        // inline tokens' text (hashtags, mentions, math results) lives beside the body
        j["inline"] = Value::Array(n.body.tokens().filter(|t| t.is_inline()).map(|t| json!({"id": t.id, "alt": t.alt, "content": t.content})).collect());
        j["attachments"] = Value::Array(n.attachments.iter().map(|a| attachment_json(a, &mut files, hash)).collect());
    }
    (json!({"v": 1, "note": j}), files)
}

/// Every file under `p` (or `p` itself): (path, path inside `p`).
pub(crate) fn walk(p: &Path) -> Vec<(PathBuf, PathBuf)> {
    if p.is_file() {
        return vec![(p.to_path_buf(), PathBuf::new())];
    }
    let mut out = Vec::new();
    let mut todo = vec![PathBuf::new()];
    while let Some(rel) = todo.pop() {
        for e in std::fs::read_dir(p.join(&rel)).into_iter().flatten().flatten() {
            let r = rel.join(e.file_name());
            if e.path().is_dir() {
                todo.push(r);
            } else if !e.file_name().to_string_lossy().starts_with("._") {
                out.push((e.path(), r));
            }
        }
    }
    out
}

fn ub(v: &Value) -> Option<Vec<u8>> {
    v.as_str().and_then(unbase64)
}

fn crypto_of(v: &Value) -> Crypto {
    Crypto { salt: ub(&v["salt"]), iterations: v["iterations"].as_i64().unwrap_or(0), wrapped_key: ub(&v["wrapped"]), verifier: ub(&v["verifier"]), iv: ub(&v["iv"]), tag: ub(&v["tag"]), values: ub(&v["values"]) }
}

/// Where a note's file is found: (content hash, its name) -> a path holding it, named so.
pub(crate) type Fetch<'a> = dyn Fn(&str, &str) -> Result<Option<PathBuf>> + 'a;

/// A file a note's JSON refers to.
fn fetch(v: &Value, get: &Fetch) -> Result<Option<PathBuf>> {
    let Some(sha) = v["sha"].as_str() else { return Ok(None) };
    let name = v["name"].as_str().filter(|n| !n.is_empty() && !n.contains('/') && *n != "." && *n != "..").unwrap_or("file");
    get(sha, name)
}

/// The server's files/<sha>, fetched into `cache` (named as it was).
fn from_server<'a>(dav: &'a Dav, root: &'a str, cache: &'a Path) -> impl Fn(&str, &str) -> Result<Option<PathBuf>> + 'a {
    move |sha: &str, name: &str| {
        let path = cache.join(sha).join(name);
        if !path.exists() && !from_local(cache.parent().map(|l| l.join("files")).as_deref(), sha, &path) && !fetch_file(dav, root, sha, &path, &unwatched)? {
            return Ok(None);
        }
        Ok(Some(path))
    }
}

fn attachment_of(v: &Value, get: &Fetch) -> Result<Attachment> {
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    let uti = s("uti");
    let mut a = Attachment {
        id: s("id"),
        kind: AttachmentKind::from_uti(&uti),
        uti,
        name: s("name"),
        width: v["width"].as_f64().unwrap_or(0.0),
        height: v["height"].as_f64().unwrap_or(0.0),
        url: v["url"].as_str().map(str::to_string),
        summary: s("summary"),
        created: v["created"].as_f64().unwrap_or(0.0),
        modified: v["modified"].as_f64().unwrap_or(0.0),
        view_size: match v["view"].as_str() {
            Some("Small") => ViewSize::Small,
            Some("Medium") => ViewSize::Medium,
            _ => ViewSize::Large,
        },
        not_downloaded: v["notDownloaded"].as_bool().unwrap_or(false),
        table: ub(&v["table"]).and_then(|b| table::decode_table(&b).ok()),
        drawing: ub(&v["drawing"]).and_then(|b| paper::decode_ink(&b)),
        pages: v["pages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let c: Vec<f32> = p[3].as_array()?.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
                Some(PdfPage { index: p[0].as_u64()? as u32, width: p[1].as_f64()? as f32, height: p[2].as_f64()? as f32, crop: c.try_into().ok()? })
            })
            .collect(),
        ..Default::default()
    };
    a.file = fetch(&v["file"], get)?;
    a.fallback = fetch(&v["fallback"], get)?;
    for p in v["parts"].as_array().into_iter().flatten() {
        a.parts.push(attachment_of(p, get)?);
    }
    Ok(a)
}

/// A note from the server's JSON, its files fetched into `cache`.
/// A note from its JSON, its files found with `get`.
pub(crate) fn note_of(j: &Value, get: &Fetch) -> Result<Note> {
    let n = &j["note"];
    let s = |k: &str| n[k].as_str().unwrap_or_default().to_string();
    let mut note = Note {
        id: s("id"),
        folder: s("folder"),
        title: s("title"),
        snippet: s("snippet"),
        created: n["created"].as_f64().unwrap_or(0.0),
        modified: n["modified"].as_f64().unwrap_or(0.0),
        pinned: n["pinned"].as_bool().unwrap_or(false),
        deleted: n["deleted"].as_bool().unwrap_or(false),
        locked: n["locked"].as_bool().unwrap_or(false),
        ..Default::default()
    };
    if let Some(sv) = n.get("sealed").filter(|v| v.is_object()) {
        let mut sealed = Sealed {
            crypto: crypto_of(&sv["crypto"]),
            hint: sv["hint"].as_str().map(str::to_string),
            data: ub(&sv["data"]).unwrap_or_default(),
            data_iv: ub(&sv["iv"]),
            data_tag: ub(&sv["tag"]),
            ..Default::default()
        };
        for a in sv["attachments"].as_array().into_iter().flatten() {
            let st = |k: &str| a[k].as_str().map(str::to_string);
            sealed.attachments.push(SealedAttachment {
                id: st("id").unwrap_or_default(),
                uti: st("uti").unwrap_or_default(),
                parent: st("parent"),
                width: a["width"].as_f64().unwrap_or(0.0),
                height: a["height"].as_f64().unwrap_or(0.0),
                created: a["created"].as_f64(),
                modified: a["modified"].as_f64(),
                fallback_image: st("fallbackImage"),
                fallback_pdf: st("fallbackPdf"),
                fallback_image_iv: ub(&a["fallbackImageIv"]),
                fallback_image_tag: ub(&a["fallbackImageTag"]),
                fallback_pdf_iv: ub(&a["fallbackPdfIv"]),
                fallback_pdf_tag: ub(&a["fallbackPdfTag"]),
                crypto: crypto_of(&a["crypto"]),
                media: a.get("media").filter(|m| m.is_object()).map(|m| SealedMedia {
                    id: m["id"].as_str().unwrap_or_default().into(),
                    generation: m["generation"].as_str().map(str::to_string),
                    asset_iv: ub(&m["assetIv"]),
                    asset_tag: ub(&m["assetTag"]),
                    crypto: crypto_of(&m["crypto"]),
                }),
            });
        }
        for t in sv["inline"].as_array().into_iter().flatten() {
            sealed.inline.push(SealedInline { id: t["id"].as_str().unwrap_or_default().into(), uti: t["uti"].as_str().unwrap_or_default().into(), crypto: crypto_of(&t["crypto"]) });
        }
        for f in sv["files"].as_array().into_iter().flatten() {
            let rel = PathBuf::from(f["path"].as_str().unwrap_or_default());
            if rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
                continue;
            }
            if let Some(p) = fetch(&json!({"sha": f["sha"], "name": "file"}), get)? {
                sealed.files.push((p, rel));
            }
        }
        note.sealed = Some(Box::new(sealed));
        return Ok(note);
    }
    let mut b = ub(&n["body"]).and_then(|b| body::decode_note(&b).ok()).unwrap_or_default();
    if let Some(os) = ub(&n["outline"]) {
        body::apply_outline_state(&mut b, &body::decode_outline_state(&os));
    }
    let inline: HashMap<&str, (&str, &str)> = n["inline"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| Some((t["id"].as_str()?, (t["alt"].as_str().unwrap_or_default(), t["content"].as_str().unwrap_or_default()))))
        .collect();
    for t in b.tokens_mut() {
        if let Some((alt, content)) = inline.get(t.id.as_str()) {
            t.alt = alt.to_string();
            t.content = content.to_string();
        }
    }
    note.body = b;
    for a in n["attachments"].as_array().into_iter().flatten() {
        note.attachments.push(attachment_of(a, get)?);
    }
    Ok(note)
}

/// The library's folders (and Smart Folders) as the server keeps them.
pub(crate) fn meta_json(lib: &Library) -> Value {
    let folders: Vec<Value> = lib
        .folders
        .iter()
        .filter(|f| f.kind != FolderKind::Trash && !f.default)
        .map(|f| json!({"id": f.id, "name": f.name, "parent": f.parent, "smart": f.smart, "order": f.sort_order, "account": f.account}))
        .collect();
    let accounts: Vec<Value> = lib.accounts.iter().map(|a| json!({"id": a.id, "name": a.name})).collect();
    json!({"v": 1, "accounts": accounts, "folders": folders})
}

// ------------------------------------------------------------------ the sync

/// What was last in step with the server.
#[derive(Default)]
struct State {
    /// id → (this library's change key, the note's fingerprint, the server's ETag)
    notes: HashMap<String, (String, String, String)>,
    meta_hash: String,
    meta_tag: String,
    /// files known to be on the server
    files: HashSet<String>,
    stamp: String,
    /// the service this state is for (a new account starts afresh)
    account: String,
    /// the library folder (where sync-base/ is)
    dir: PathBuf,
}

impl State {
    fn load(library: &Path, account: &str) -> State {
        let Some(j) = std::fs::read(library.join("sync-state.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
            return State { account: account.into(), dir: library.to_path_buf(), ..Default::default() };
        };
        if j["account"].as_str() != Some(account) {
            return State { account: account.into(), dir: library.to_path_buf(), ..Default::default() };
        }
        let mut s = State { account: account.into(), dir: library.to_path_buf(), ..Default::default() };
        for (id, v) in j["notes"].as_object().into_iter().flatten() {
            let p = |i: usize| v[i].as_str().unwrap_or_default().to_string();
            s.notes.insert(id.clone(), (p(0), p(1), p(2)));
        }
        s.meta_hash = j["metaHash"].as_str().unwrap_or_default().into();
        s.meta_tag = j["metaTag"].as_str().unwrap_or_default().into();
        s.files = j["files"].as_array().into_iter().flatten().filter_map(|f| f.as_str().map(str::to_string)).collect();
        s.stamp = j["stamp"].as_str().unwrap_or_default().into();
        s
    }

    fn save(&self, library: &Path) -> Result<()> {
        let notes: serde_json::Map<String, Value> = self.notes.iter().map(|(k, v)| (k.clone(), json!([v.0, v.1, v.2]))).collect();
        let j = json!({"account": self.account, "notes": notes, "metaHash": self.meta_hash, "metaTag": self.meta_tag, "files": self.files.iter().collect::<Vec<_>>(), "stamp": self.stamp});
        write_private(&library.join("sync-state.json"), j.to_string().as_bytes())
    }
}

/// A sync that doesn't merge: this library sent over the service's (`Push`), or the service's
/// taken over this library's (`Pull`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Force {
    #[default]
    None,
    Push,
    Pull,
}

/// What a sync did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub sent: usize,
    pub received: usize,
    pub removed: usize,
    /// notes edited on two devices: both kept
    pub copies: usize,
    /// notes edited on two devices whose edits were merged
    pub merged: usize,
    /// notes and attachments that couldn't be sent or read this time (tried again next sync)
    pub failed: usize,
}

impl Report {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.sent > 0 {
            parts.push(format!("{} sent", self.sent));
        }
        if self.received > 0 {
            parts.push(format!("{} received", self.received));
        }
        if self.removed > 0 {
            parts.push(format!("{} removed", self.removed));
        }
        if self.merged > 0 {
            parts.push(format!("{} edited on two devices (merged)", self.merged));
        }
        if self.copies > 0 {
            parts.push(format!("{} edited on two devices (kept both: “(other copy)”)", self.copies));
        }
        if self.failed > 0 {
            parts.push(format!("{} couldn’t be synced this time (sync will try again)", self.failed));
        }
        if parts.is_empty() { "Up to date".into() } else { parts.join(", ") }
    }
}

/// What a sync is doing now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// comparing this library with the service's
    #[default]
    Checking,
    /// reading the notes and attachments changed elsewhere
    Downloading,
    /// bringing them into this library (merging edits made on two devices)
    Merging,
    /// sending this library's changes: attachments first, then the notes
    Uploading,
    /// folders and the service's marker, last
    Finishing,
}

/// How far a sync is, for a progress bar ([`sync_now_with`]'s callback, or [`progress`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub phase: Phase,
    /// files done of this phase's total
    pub done: usize,
    pub total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// what's being sent or read now: a note's title or an attachment's name
    pub item: String,
}

impl Progress {
    /// "Uploading 312 of 1,418 · 210 MB of 812 MB"
    pub fn message(&self) -> String {
        let what = match self.phase {
            Phase::Checking => return "Checking for changes…".into(),
            Phase::Merging => return "Bringing in changes…".into(),
            Phase::Finishing => return "Finishing…".into(),
            Phase::Downloading => "Downloading",
            Phase::Uploading => "Uploading",
        };
        let mut m = format!("{what} {} of {}", thousands(self.done as u64), thousands(self.total as u64));
        if self.bytes_total > 0 {
            m.push_str(&format!(" · {} of {}", megabytes(self.bytes_done, self.bytes_total), megabytes(self.bytes_total, self.bytes_total)));
        }
        m
    }

    /// 0…1, for the bar.
    pub fn fraction(&self) -> f32 {
        if self.bytes_total > 0 {
            (self.bytes_done as f64 / self.bytes_total as f64).min(1.0) as f32
        } else if self.total > 0 {
            (self.done as f64 / self.total as f64).min(1.0) as f32
        } else {
            0.0
        }
    }
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// In the units the total reads best in ("210 MB", "1.2 MB", "340 KB").
fn megabytes(b: u64, scale: u64) -> String {
    if scale >= 10_000_000 {
        format!("{} MB", thousands(b / 1_000_000))
    } else if scale >= 1_000_000 {
        format!("{:.1} MB", b as f64 / 1e6)
    } else {
        format!("{} KB", b.div_ceil(1000))
    }
}

static PROGRESS: std::sync::Mutex<Option<Progress>> = std::sync::Mutex::new(None);

/// The sync under way in this process, if one is (for a window that polls rather than listens).
pub fn progress() -> Option<Progress> {
    PROGRESS.lock().ok().and_then(|p| p.clone())
}

/// Keeps [`Progress`] and tells the listener of each step.
struct Reporter<'a> {
    on: &'a (dyn Fn(&Progress) + Sync),
    now: std::sync::Mutex<Progress>,
}

impl Reporter<'_> {
    fn set(&self, f: impl FnOnce(&mut Progress)) {
        let p = {
            let Ok(mut now) = self.now.lock() else { return };
            f(&mut now);
            now.clone()
        };
        if let Ok(mut g) = PROGRESS.lock() {
            *g = Some(p.clone());
        }
        (self.on)(&p);
    }

    fn phase(&self, phase: Phase, total: usize, bytes_total: u64) {
        self.set(|p| *p = Progress { phase, total, bytes_total, ..Default::default() });
    }

    fn started(&self, item: &str) {
        self.set(|p| p.item = item.to_string());
    }

    /// Bytes moved within a file still on its way.
    fn moved(&self, bytes: u64) {
        self.set(|p| p.bytes_done = (p.bytes_done + bytes).min(p.bytes_total.max(p.bytes_done)));
    }

    fn finished(&self, bytes: u64) {
        self.set(|p| {
            p.done += 1;
            p.bytes_done += bytes;
        });
    }
}

/// Bring the library and the server in step; the outcome is also kept for [`Status::load`].
pub fn sync_now(store: &mut Store, s: &Settings, force: Force) -> Result<Report> {
    sync_now_with(store, s, force, &|_| {})
}

/// [`sync_now`], telling `on` how far it is as it goes (from the sync's own threads: `on` must be
/// quick, e.g. store the [`Progress`] and ask for a repaint).
pub fn sync_now_with(store: &mut Store, s: &Settings, force: Force, on: &(dyn Fn(&Progress) + Sync)) -> Result<Report> {
    let rep = Reporter { on, now: Default::default() };
    let r = sync(store, s, force, &rep);
    if let Ok(mut g) = PROGRESS.lock() {
        *g = None;
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let status = match &r {
        Ok(rep) => Status { last_sync: Some(now), summary: rep.summary(), failed: rep.failed > 0 },
        Err(e) => Status { last_sync: Status::load(store.dir()).last_sync, summary: format!("Sync failed: {e}"), failed: true },
    };
    status.save(store.dir());
    r
}

fn new_stamp() -> String {
    crate::pb::new_uuid()
}

/// Run `jobs` on up to [`Dav`]'s lanes of workers at once (fewer as the service slows us down).
/// A job that fails (after its request's own tries) doesn't stop the others: the failed ones'
/// indexes are returned, for the next sync to try again. Only when they keep failing (the service
/// gone, the password refused) does it stop: then the error is returned.
fn parallel<T: Sync>(dav: &Dav, jobs: &[T], run: impl Fn(&T) -> Result<()> + Sync) -> Result<Vec<usize>> {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
    /// failures in a row (none done in between) that stop the sync
    const GIVE_UP: usize = 8;
    if jobs.is_empty() {
        return Ok(Vec::new());
    }
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let streak = AtomicUsize::new(0);
    let failed: std::sync::Mutex<Vec<usize>> = Default::default();
    let last: std::sync::Mutex<Option<Error>> = Default::default();
    let workers = dav.lanes.load(Relaxed).clamp(1, jobs.len());
    std::thread::scope(|sc| {
        for w in 0..workers {
            let (next, stop, streak, failed, last, run) = (&next, &stop, &streak, &failed, &last, &run);
            sc.spawn(move || loop {
                // (worker 0 always stays: there's always one lane)
                if stop.load(Relaxed) || w >= dav.lanes.load(Relaxed) {
                    return;
                }
                let i = next.fetch_add(1, Relaxed);
                let Some(job) = jobs.get(i) else { return };
                match run(job) {
                    Ok(()) => streak.store(0, Relaxed),
                    Err(e) => {
                        if let Ok(mut f) = failed.lock() {
                            f.push(i);
                        }
                        if let Ok(mut l) = last.lock() {
                            *l = Some(e);
                        }
                        if streak.fetch_add(1, Relaxed) + 1 >= GIVE_UP.min(jobs.len()) {
                            stop.store(true, Relaxed);
                        }
                    }
                }
            });
        }
    });
    let failed = failed.into_inner().unwrap_or_default();
    match last.into_inner().ok().flatten() {
        Some(e) if stop.load(Relaxed) => Err(e),
        _ => Ok(failed),
    }
}

/// The files a note's JSON refers to: (content hash, the name it's fetched as).
fn file_refs(v: &Value, out: &mut Vec<(String, String)>) {
    match v {
        Value::Object(o) => {
            if let Some(sha) = o.get("sha").and_then(Value::as_str) {
                let name = o.get("name").and_then(Value::as_str).filter(|n| !n.is_empty() && !n.contains('/') && *n != "." && *n != "..").unwrap_or("file");
                out.push((sha.to_string(), name.to_string()));
            }
            o.values().for_each(|x| file_refs(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| file_refs(x, out)),
        _ => {}
    }
}

/// A file's SHA-256 (hex), read as it goes.
fn sha_of_file(p: &Path) -> Option<String> {
    let mut h = sha2::Sha256::new();
    std::io::copy(&mut std::fs::File::open(p).ok()?, &mut h).ok()?;
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The server's files/<sha> into `path`, checked against its name (a damaged or half-written file
/// is never taken in).
fn fetch_file(dav: &Dav, root: &str, sha: &str, path: &Path, moved: Moved) -> Result<bool> {
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    // (the same name every time, so a download cut short goes on from there next sync)
    let tmp = dir.join(format!(".{sha}.dl"));
    if !dav.get_to(&format!("{root}/files/{sha}"), &tmp, moved)? {
        return Ok(false);
    }
    if sha_of_file(&tmp).as_deref() != Some(sha) {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::Network(format!("An attachment on the service doesn’t match its name ({sha}); sync will try again later.")));
    }
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

/// A file this library already has (a native library's files/<sha>), put where the sync reads
/// it from instead of downloading it again.
fn from_local(local: Option<&Path>, sha: &str, path: &Path) -> bool {
    let Some(src) = local.map(|l| l.join(sha)).filter(|p| p.is_file()) else { return false };
    let Some(dir) = path.parent() else { return false };
    std::fs::create_dir_all(dir).is_ok() && (std::fs::hard_link(&src, path).is_ok() || std::fs::copy(&src, path).is_ok())
}

/// A change of this library's to send.
enum Job {
    /// a note's JSON (its files go first); an empty key: the one it has once written here
    Note { id: String, key: String, j: Value, files: Files, hash: String, title: String },
    /// a note erased here
    Tombstone { id: String },
}

/// What the sync's workers share: the state, the report, and when the state was last saved.
struct Shared<'a> {
    state: &'a mut State,
    rep: &'a mut Report,
    saved: std::time::Instant,
}

impl Shared<'_> {
    /// The state written out now and then, so a sync cut short starts again where it was.
    fn checkpoint(&mut self) {
        if self.saved.elapsed() > Duration::from_secs(1) {
            let _ = self.state.save(&self.state.dir);
            self.saved = std::time::Instant::now();
        }
    }
}

fn sync(store: &mut Store, s: &Settings, force: Force, progress: &Reporter) -> Result<Report> {
    if !s.ready() {
        return Err(Error::Network("Sync isn’t set up (address and user name).".into()));
    }
    progress.phase(Phase::Checking, 0, 0);
    let dav = Dav::new(s);
    let root = s.folder();
    let lib_dir = store.dir().to_path_buf();
    let account = format!("{}|{}|{}", s.url.trim(), s.user.trim(), root);
    let mut state = State::load(&lib_dir, &account);
    let keys = store.change_keys()?;
    let mut rep = Report::default();

    // all quiet: nothing changed here since, and the service's marker is the same
    let quiet_here = !state.notes.is_empty() && keys.iter().all(|(id, k)| state.notes.get(id).is_some_and(|v| &v.0 == k)) && state.notes.keys().all(|id| keys.contains_key(id));
    if force == Force::None && quiet_here && !state.stamp.is_empty() {
        if let Some(st) = dav.get(&format!("{root}/stamp.json"))? {
            if String::from_utf8_lossy(&st) == state.stamp {
                return Ok(rep);
            }
        }
    }
    // (one at a time, and only these two)
    dav.make_folders(&format!("{root}/notes"))?;
    dav.make_folders(&format!("{root}/files"))?;
    // (kept until a sync finishes: what one cut short had read is used again)
    let cache = lib_dir.join(".sync-cache");

    // ---- folders and Smart Folders first (a note never arrives elsewhere before its folder) ----
    let lib = store.library_meta()?;
    let mine = meta_json(&lib);
    let mine_hash = fingerprint(mine.to_string().as_bytes());
    let listing = dav.list(&root)?.unwrap_or_default();
    let meta_remote = listing.iter().find(|e| e.0 == "meta.json").map(|e| e.2.clone());
    match force {
        // ours changed, theirs not: ours goes up / theirs changed, ours not: theirs comes down
        Force::Push => {
            state.meta_hash.clear();
            state.meta_tag = meta_remote.clone().unwrap_or_default();
        }
        Force::Pull => {
            state.meta_hash = mine_hash.clone();
            state.meta_tag.clear();
        }
        Force::None => {}
    }
    let mine_changed = mine_hash != state.meta_hash;
    if let Some(tag) = meta_remote.as_ref().filter(|t| **t != state.meta_tag) {
        // (an empty or half-written one, e.g. a server's stale listing, counts as not there yet)
        if let Some(theirs) = dav.get(&format!("{root}/meta.json"))?.and_then(|b| serde_json::from_slice::<Value>(&b).ok()).filter(Value::is_object) {
            apply_folders(store, &lib, &theirs, mine_changed)?;
            state.meta_tag = tag.clone();
            state.meta_hash = fingerprint(meta_json(&store.library_meta()?).to_string().as_bytes());
            rep.received += 1;
        }
    }
    let send_meta = |store: &Store, state: &mut State, rep: &mut Report, remote_there: bool| -> Result<()> {
        let now = meta_json(&store.library_meta()?);
        let h = fingerprint(now.to_string().as_bytes());
        if h == state.meta_hash && remote_there {
            return Ok(());
        }
        let tag = dav.put(&format!("{root}/meta.json"), now.to_string().into_bytes())?;
        state.meta_hash = h;
        state.meta_tag = match tag {
            Some(t) => t,
            None => dav.list(&root)?.unwrap_or_default().into_iter().find(|e| e.0 == "meta.json").map(|e| e.2).unwrap_or_default(),
        };
        rep.sent += 1;
        Ok(())
    };
    send_meta(store, &mut state, &mut rep, meta_remote.is_some())?;
    state.save(&lib_dir)?;

    // ---- notes ----
    let listed = dav.list(&format!("{root}/notes"))?.unwrap_or_default();
    let remote: HashMap<String, String> = listed.iter().filter(|e| !e.1 && e.0.ends_with(".json")).map(|e| (e.0.trim_end_matches(".json").to_string(), e.2.clone())).collect();
    let sizes: HashMap<String, u64> = listed.iter().map(|e| (e.0.trim_end_matches(".json").to_string(), e.3)).collect();
    match force {
        // every note here counts as changed here only / every note there as changed there only
        Force::Push => {
            for id in keys.keys() {
                state.notes.insert(id.clone(), (String::new(), String::new(), remote.get(id).cloned().unwrap_or_default()));
            }
        }
        Force::Pull => {
            for id in remote.keys() {
                let k = keys.get(id).cloned().unwrap_or_default();
                let h = state.notes.get(id).map(|v| v.1.clone()).unwrap_or_default();
                state.notes.insert(id.clone(), (k, h, String::new()));
            }
        }
        Force::None => {}
    }
    let ids: Vec<String> = {
        let mut all: HashSet<String> = remote.keys().cloned().collect();
        all.extend(keys.keys().cloned());
        all.extend(state.notes.keys().cloned());
        let mut v: Vec<String> = all.into_iter().collect();
        v.sort();
        v
    };

    // ---- what changed elsewhere, read first (several at once): the notes, then their files ----
    let wanted: Vec<&String> = ids.iter().filter(|id| remote.get(*id).is_some_and(|t| state.notes.get(*id).is_none_or(|k| &k.2 != t))).collect();
    let fetched: std::sync::Mutex<HashMap<String, Vec<u8>>> = Default::default();
    // attachments that couldn't be read: their notes wait for the next sync
    let mut missing: HashSet<String> = HashSet::new();
    if !wanted.is_empty() {
        progress.phase(Phase::Downloading, wanted.len(), wanted.iter().map(|id| sizes.get(*id).copied().unwrap_or(0)).sum());
        let notes_cache = cache.join("notes");
        std::fs::create_dir_all(&notes_cache)?;
        rep.failed += parallel(&dav, &wanted, |id| {
            progress.started(id);
            let tag = remote.get(*id).cloned().unwrap_or_default();
            let (jp, tp) = (notes_cache.join(format!("{id}.json")), notes_cache.join(format!("{id}.tag")));
            // read already by a sync cut short (and not changed since)
            let b = match std::fs::read(&tp).ok().filter(|t| *t == tag.as_bytes()).and_then(|_| std::fs::read(&jp).ok()) {
                Some(b) => Some(b),
                None => {
                    let b = dav.get(&format!("{root}/notes/{id}.json"))?;
                    if let Some(b) = &b {
                        let _ = std::fs::write(&jp, b).and_then(|_| std::fs::write(&tp, &tag));
                    }
                    b
                }
            };
            if let Some(b) = b {
                let n = b.len() as u64;
                fetched.lock().map_err(|_| Error::Format("sync".into()))?.insert(id.to_string(), b);
                progress.finished(n);
            }
            Ok(())
        })?
        .len();
        // their attachments, into the cache the notes are read from
        let mut need: Vec<(String, String)> = Vec::new();
        for b in fetched.lock().map_err(|_| Error::Format("sync".into()))?.values() {
            if let Ok(j) = serde_json::from_slice::<Value>(b) {
                file_refs(&j["note"], &mut need);
            }
        }
        need.sort();
        need.dedup();
        // (what this library has already, or a sync cut short read, isn't read again)
        let local = store.native_root().map(|r| r.join("files"));
        need.retain(|(sha, name)| {
            let p = cache.join(sha).join(name);
            !p.exists() && !from_local(local.as_deref(), sha, &p)
        });
        if !need.is_empty() {
            let file_sizes: HashMap<String, u64> = dav.list(&format!("{root}/files"))?.unwrap_or_default().into_iter().map(|e| (e.0, e.3)).collect();
            progress.phase(Phase::Downloading, need.len(), need.iter().map(|(sha, _)| file_sizes.get(sha).copied().unwrap_or(0)).sum());
            let failed = parallel(&dav, &need, |(sha, name)| {
                progress.started(name);
                let told = std::sync::atomic::AtomicU64::new(0);
                fetch_file(&dav, &root, sha, &cache.join(sha).join(name), &|n| {
                    told.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                    progress.moved(n);
                })?;
                // (what a sync cut short had read already counts too)
                progress.finished(file_sizes.get(sha).copied().unwrap_or(0).saturating_sub(told.into_inner()));
                Ok(())
            })?;
            missing.extend(failed.into_iter().map(|i| need[i].0.clone()));
        }
    }
    let fetched = fetched.into_inner().unwrap_or_default();

    // ---- each note compared, merged; what to send is gathered and sent after ----
    progress.phase(Phase::Merging, 0, 0);
    let mut incoming: Vec<Note> = Vec::new();
    let mut erase: Vec<String> = Vec::new();
    let mut jobs: Vec<Job> = Vec::new();
    let job_of = |id: &str, key: &str, n: &Note, j: Value, files: Files, hash: String| Job::Note { id: id.to_string(), key: key.to_string(), j, files, hash, title: n.title.clone() };
    for id in ids {
        let known = state.notes.get(&id).cloned();
        let theirs = remote.get(&id);
        let here = keys.get(&id);
        let theirs_changed = theirs.is_some_and(|t| known.as_ref().is_none_or(|k| &k.2 != t));
        // changed here: the change key moved, and the note's content (its JSON) too
        let mut mine: Option<(Note, Value, Files, String)> = None;
        let load_mine = |store: &Store| -> Result<Option<(Note, Value, Files, String)>> {
            let n = store.note(&id)?;
            if n.locked && n.sealed.is_none() && n.lock.is_none() {
                // locked with a key that never leaves its Mac (login password): not synced
                return Ok(None);
            }
            let (j, f) = note_json(&n);
            let h = fingerprint(j.to_string().as_bytes());
            Ok(Some((n, j, f, h)))
        };
        let key_moved = here.is_some_and(|k| known.as_ref().is_none_or(|v| &v.0 != k));
        let mut mine_changed = false;
        if key_moved {
            mine = load_mine(store)?;
            let Some(h) = mine.as_ref().map(|m| &m.3) else { continue };
            mine_changed = known.as_ref().is_none_or(|v| &v.1 != h);
            if !mine_changed {
                // touched but the same: just remember the new key
                let v = state.notes.get_mut(&id).unwrap();
                v.0 = here.unwrap().clone();
            }
        }
        if theirs_changed {
            let Some(b) = fetched.get(&id) else { continue };
            // (a note file that doesn't read, e.g. still being written: left for the next round)
            let Ok(j) = serde_json::from_slice::<Value>(b) else { continue };
            if !missing.is_empty() {
                let mut refs = Vec::new();
                file_refs(&j["note"], &mut refs);
                if refs.iter().any(|(sha, _)| missing.contains(sha)) {
                    rep.failed += 1;
                    continue;
                }
            }
            let theirs_hash = fingerprint(j.to_string().as_bytes());
            let theirs_tag = theirs.unwrap().clone();
            if is_tombstone(&j) {
                // deleted for good on another device: here too, unless edited here meanwhile
                if here.is_some() && mine_changed && known.as_ref().is_some_and(|k| k.0 != "deleted") {
                    if mine.is_none() {
                        mine = load_mine(store)?;
                    }
                    if let Some((mn, mj, mf, mh)) = mine.take() {
                        jobs.push(job_of(&id, here.unwrap(), &mn, mj, mf, mh));
                    }
                    continue;
                }
                if here.is_some() {
                    erase.push(id.clone());
                    rep.removed += 1;
                }
                state.notes.insert(id.clone(), ("deleted".into(), theirs_hash, theirs_tag));
                continue;
            }
            let theirs_note = note_of(&j, &from_server(&dav, &root, &cache))?;
            if mine_changed && known.is_some() && mine.as_ref().is_some_and(|m| m.3 != theirs_hash) {
                // both changed: merged against the last version both had, when the edits don't clash
                let base = load_base(&lib_dir, &id);
                if let Some(merged) = base.as_ref().and_then(|b| merge_notes(b, &mine.as_ref().unwrap().1, &j)) {
                    let (mn, _, mf, _) = mine.take().unwrap();
                    let srv = from_server(&dav, &root, &cache);
                    let local = |sha: &str, name: &str| -> Result<Option<PathBuf>> {
                        match mf.get(sha) {
                            Some(p) => Ok(Some(p.clone())),
                            None => srv(sha, name),
                        }
                    };
                    let mut merged_note = note_of(&merged, &local)?;
                    let mh = fingerprint(merged.to_string().as_bytes());
                    jobs.push(job_of(&id, "", &mn, merged, mf, mh));
                    merged_note.id = id.clone();
                    incoming.push(merged_note);
                    rep.merged += 1;
                    continue;
                }
                // they clash: the newer edit wins, the other is kept as a copy
                let (mn, mj, mf, mh) = mine.take().unwrap();
                let mine_newer = mn.modified > theirs_note.modified;
                let loser = if mine_newer { theirs_note.clone() } else { mn.clone() };
                incoming.push(other_copy(&loser));
                rep.copies += 1;
                if mine_newer {
                    jobs.push(job_of(&id, here.unwrap(), &mn, mj, mf, mh));
                } else {
                    incoming.push(theirs_note);
                    state.notes.insert(id.clone(), (String::new(), theirs_hash, theirs_tag));
                    rep.received += 1;
                }
                continue;
            }
            if mine.as_ref().is_none_or(|m| m.3 != theirs_hash) {
                incoming.push(theirs_note);
                rep.received += 1;
            }
            // the change key it gets once written here is read after
            state.notes.insert(id.clone(), (String::new(), theirs_hash, theirs_tag));
            save_base(&lib_dir, &id, &j);
            continue;
        }
        if theirs.is_none() {
            if let Some(k) = &known {
                // gone from the server (deleted on another device)
                if here.is_some() && !mine_changed && !k.2.is_empty() {
                    erase.push(id.clone());
                    state.notes.remove(&id);
                    rep.removed += 1;
                    continue;
                }
                if here.is_none() {
                    state.notes.remove(&id);
                    continue;
                }
                // changed here meanwhile: the edit wins over the deletion (sent below)
            }
        }
        if here.is_none() {
            if theirs.is_some() && known.as_ref().is_some_and(|k| k.0 != "deleted") {
                // erased here: a tombstone on the server, so every device learns it
                jobs.push(Job::Tombstone { id: id.clone() });
            }
            continue;
        }
        if mine_changed || theirs.is_none() || known.is_none() {
            if mine.is_none() {
                mine = load_mine(store)?;
            }
            let Some((mn, j, f, h)) = mine.take() else { continue };
            jobs.push(job_of(&id, here.unwrap(), &mn, j, f, h));
        }
    }
    // what came in is written here before anything goes out (the state, saved as sending goes,
    // then never says a note is in step that isn't)
    if !erase.is_empty() {
        for id in &erase {
            store.erase_note(id)?;
        }
    }
    if !incoming.is_empty() {
        store.put_synced_notes(&incoming)?;
    }
    // remember the change keys of what was just written here (and of the copies, sent next)
    let keys = store.change_keys()?;
    for n in &incoming {
        if let (Some(v), Some(k)) = (state.notes.get_mut(&n.id), keys.get(&n.id)) {
            if v.0.is_empty() {
                v.0 = k.clone();
            }
        }
    }
    // the copies (new ids) go up too
    let planned: HashSet<String> = jobs.iter().map(|j| match j {
        Job::Note { id, .. } | Job::Tombstone { id } => id.clone(),
    }).collect();
    for n in incoming.iter().filter(|n| !state.notes.contains_key(&n.id) && !planned.contains(&n.id)) {
        let n = store.note(&n.id)?;
        let (j, f) = note_json(&n);
        let h = fingerprint(j.to_string().as_bytes());
        jobs.push(job_of(&n.id, &keys[&n.id], &n, j, f, h));
    }
    state.save(&lib_dir)?;

    // ---- sending: every attachment first, then the notes that use them ----
    let mut files: Vec<(String, PathBuf, u64)> = Vec::new();
    let mut seen: HashSet<&String> = HashSet::new();
    for j in &jobs {
        if let Job::Note { files: f, .. } = j {
            for (sha, path) in f {
                if !state.files.contains(sha) && seen.insert(sha) {
                    if let Ok(m) = std::fs::metadata(path) {
                        files.push((sha.clone(), path.clone(), m.len()));
                    }
                }
            }
        }
    }
    let note_bytes = |j: &Job| match j {
        Job::Note { j, .. } => j.to_string().len() as u64,
        Job::Tombstone { .. } => 0,
    };
    if !files.is_empty() || !jobs.is_empty() {
        progress.phase(Phase::Uploading, files.len() + jobs.len(), files.iter().map(|f| f.2).sum::<u64>() + jobs.iter().map(note_bytes).sum::<u64>());
    }
    let sent = {
        let shared = std::sync::Mutex::new(Shared { state: &mut state, rep: &mut rep, saved: std::time::Instant::now() });
        let lock = || shared.lock().map_err(|_| Error::Format("sync".into()));
        (|| -> Result<()> {
            let failed = parallel(&dav, &files, |(sha, path, size)| {
                progress.started(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().as_str());
                let told = std::sync::atomic::AtomicU64::new(0);
                dav.put_file(&format!("{root}/files/{sha}"), path, &|n| {
                    // (a try that failed and starts again isn't counted twice)
                    let before = told.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                    progress.moved(n.min(size.saturating_sub(before)));
                })?;
                let mut sh = lock()?;
                sh.state.files.insert(sha.clone());
                sh.checkpoint();
                drop(sh);
                progress.finished(size.saturating_sub(told.into_inner()));
                Ok(())
            })?;
            // a note whose attachment couldn't be sent waits for the next sync
            let unsent: HashSet<&String> = failed.iter().map(|i| &files[*i].0).collect();
            lock()?.rep.failed += failed.len();
            let failed = parallel(&dav, &jobs, |job| {
                match job {
                    Job::Note { files: f, .. } if f.keys().any(|sha| unsent.contains(sha)) => {
                        lock()?.rep.failed += 1;
                        progress.finished(0);
                    }
                    Job::Note { id, key, j, hash, title, .. } => {
                        progress.started(title);
                        let body = j.to_string();
                        let tag = dav.put(&format!("{root}/notes/{id}.json"), body.clone().into_bytes())?;
                        // (a merged note's key: the one it got when written here just now)
                        let key = if key.is_empty() { keys.get(id).cloned().unwrap_or_default() } else { key.clone() };
                        save_base(&lib_dir, id, j);
                        let mut sh = lock()?;
                        sh.state.notes.insert(id.clone(), (key, hash.clone(), tag.unwrap_or_default()));
                        sh.rep.sent += 1;
                        sh.checkpoint();
                        drop(sh);
                        progress.finished(body.len() as u64);
                    }
                    Job::Tombstone { id } => {
                        progress.started(id);
                        let tag = dav.put(&format!("{root}/notes/{id}.json"), tombstone_json(id).to_string().into_bytes())?;
                        let _ = std::fs::remove_file(base_path(&lib_dir, id));
                        let mut sh = lock()?;
                        sh.state.notes.insert(id.clone(), ("deleted".into(), String::new(), tag.unwrap_or_default()));
                        sh.rep.removed += 1;
                        sh.checkpoint();
                        drop(sh);
                        progress.finished(0);
                    }
                }
                Ok(())
            })?;
            lock()?.rep.failed += failed.len();
            Ok(())
        })()
    };
    if let Err(e) = sent {
        // what was sent stays sent: the next sync goes on from there (and the service's marker
        // isn't changed, so no other device takes this half for a finished sync)
        state.save(&lib_dir)?;
        return Err(e);
    }

    // ---- last: folders, then the marker ----
    progress.phase(Phase::Finishing, 0, 0);
    send_meta(store, &mut state, &mut rep, true)?;

    // notes saved without an ETag in the answer: read them from a listing, so the next round
    // doesn't take them for others' changes
    if state.notes.values().any(|v| v.2.is_empty()) {
        let tags: HashMap<String, String> =
            dav.list(&format!("{root}/notes"))?.unwrap_or_default().into_iter().map(|e| (e.0.trim_end_matches(".json").to_string(), e.2)).collect();
        for (id, v) in state.notes.iter_mut() {
            if v.2.is_empty() {
                if let Some(t) = tags.get(id) {
                    v.2 = t.clone();
                }
            }
        }
    }
    if rep.failed > 0 {
        // not all went: no new marker (no other device takes this for a finished sync), and what
        // was read is kept for the next try
        state.save(&lib_dir)?;
        return Ok(rep);
    }
    let _ = std::fs::remove_dir_all(&cache);
    // something changed on the service: a new marker, so the others' quick check notices
    let stamp_now = dav.get(&format!("{root}/stamp.json"))?;
    if rep.sent > 0 || rep.removed > 0 || stamp_now.is_none() {
        let fresh = new_stamp();
        dav.put(&format!("{root}/stamp.json"), fresh.clone().into_bytes())?;
        state.stamp = fresh;
    } else if let Some(st) = stamp_now {
        state.stamp = String::from_utf8_lossy(&st).into_owned();
    }
    state.save(&lib_dir)?;
    Ok(rep)
}

// ------------------------------------------------------------------ merging, tombstones

/// The last version of each note both sides agreed on (sync-base/<id>.json): what a three-way
/// merge compares the two edits to.
fn base_path(lib: &Path, id: &str) -> PathBuf {
    lib.join("sync-base").join(format!("{id}.json"))
}

fn save_base(lib: &Path, id: &str, j: &Value) {
    if lib.as_os_str().is_empty() {
        return;
    }
    let _ = crate::native::write_atomic(&base_path(lib, id), j.to_string().as_bytes());
}

fn load_base(lib: &Path, id: &str) -> Option<Value> {
    std::fs::read(base_path(lib, id)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// A note deleted for good, as the server keeps it (so other devices learn it).
fn tombstone_json(id: &str) -> Value {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    json!({"v": 1, "deleted": true, "id": id, "at": now})
}

fn is_tombstone(j: &Value) -> bool {
    j["deleted"].as_bool() == Some(true) && j.get("note").is_none()
}

/// Three-way merge of lists keyed by id: what one side changed (against `base`) is taken; both
/// changing the same item differently, or one deleting what the other changed, can't be merged.
/// Theirs' order, with mine's new items after their predecessor.
pub(crate) fn merge3<T: Clone + PartialEq>(base: &[T], mine: &[T], theirs: &[T], id: impl Fn(&T) -> String) -> Option<Vec<T>> {
    let index = |v: &[T]| -> HashMap<String, T> { v.iter().map(|x| (id(x), x.clone())).collect() };
    let (b, m, t) = (index(base), index(mine), index(theirs));
    if b.len() != base.len() || m.len() != mine.len() || t.len() != theirs.len() || b.contains_key("") || m.contains_key("") || t.contains_key("") {
        return None; // ids missing or repeated: no safe merge
    }
    let pick = |k: &str| -> Option<Option<T>> {
        Some(match (b.get(k), m.get(k), t.get(k)) {
            (Some(bv), Some(mv), Some(tv)) => Some(if mv == bv {
                tv.clone()
            } else if tv == bv || tv == mv {
                mv.clone()
            } else {
                return None;
            }),
            (Some(bv), None, Some(tv)) => {
                if tv == bv {
                    None
                } else {
                    return None;
                }
            }
            (Some(bv), Some(mv), None) => {
                if mv == bv {
                    None
                } else {
                    return None;
                }
            }
            (Some(_), None, None) => None,
            (None, Some(mv), Some(tv)) => Some(if mv == tv { mv.clone() } else { return None }),
            (None, Some(mv), None) => Some(mv.clone()),
            (None, None, Some(tv)) => Some(tv.clone()),
            (None, None, None) => None,
        })
    };
    let mut out: Vec<(String, T)> = Vec::new();
    for x in theirs {
        let k = id(x);
        if let Some(v) = pick(&k)? {
            out.push((k, v));
        }
    }
    let mut prev: Option<String> = None;
    for x in mine {
        let k = id(x);
        if !t.contains_key(&k) && !b.contains_key(&k) {
            let at = prev.as_ref().and_then(|p| out.iter().position(|(q, _)| q == p)).map(|i| i + 1).unwrap_or(0);
            out.insert(at, (k.clone(), x.clone()));
        }
        prev = Some(k);
    }
    Some(out.into_iter().map(|(_, v)| v).collect())
}

/// Three-way merge of a note's JSON (paragraphs by their ids, attachments and inline tokens by
/// theirs; folder, pin and Recently Deleted from the side that changed them). None when the two
/// edits clash, or for locked notes.
pub(crate) fn merge_notes(base: &Value, mine: &Value, theirs: &Value) -> Option<Value> {
    let (bn, mn, tn) = (&base["note"], &mine["note"], &theirs["note"]);
    if [bn, mn, tn].iter().any(|n| n.get("sealed").is_some_and(|s| !s.is_null()) || n["locked"].as_bool() == Some(true) || n.get("body").is_none()) {
        return None;
    }
    let body = |n: &Value| -> Option<Body> {
        let mut b = body::decode_note(&ub(&n["body"])?).ok()?;
        if let Some(os) = ub(&n["outline"]) {
            body::apply_outline_state(&mut b, &body::decode_outline_state(&os));
        }
        Some(b)
    };
    let (bb, mb, tb) = (body(bn)?, body(mn)?, body(tn)?);
    // a side that rewrote the whole note (no paragraph kept) can't be merged paragraph by paragraph
    let kept = |x: &Body| x.paragraphs.iter().any(|p| bb.paragraphs.iter().any(|q| q.id == p.id));
    if !bb.paragraphs.is_empty() && (!kept(&mb) || !kept(&tb)) {
        return None;
    }
    let paragraphs = merge3(&bb.paragraphs, &mb.paragraphs, &tb.paragraphs, |p| p.id.clone())?;
    let merged = Body { paragraphs };
    let list = |n: &Value, k: &str| n[k].as_array().cloned().unwrap_or_default();
    let key = |v: &Value| v["id"].as_str().unwrap_or_default().to_string();
    let attachments = merge3(&list(bn, "attachments"), &list(mn, "attachments"), &list(tn, "attachments"), key)?;
    let inline = merge3(&list(bn, "inline"), &list(mn, "inline"), &list(tn, "inline"), key)?;
    let mut out = tn.clone();
    for k in ["folder", "pinned", "deleted"] {
        if mn[k] != bn[k] {
            out[k] = mn[k].clone();
        }
    }
    let num = |n: &Value, k: &str| n[k].as_f64().unwrap_or(0.0);
    out["modified"] = json!(num(mn, "modified").max(num(tn, "modified")));
    out["body"] = Value::String(base64(&body::encode_note(&merged)));
    out["outline"] = b64o(&body::encode_outline_state(&merged));
    let (title, snippet) = body::title_and_snippet(&merged);
    out["title"] = json!(title);
    out["snippet"] = json!(snippet);
    out["attachments"] = Value::Array(attachments);
    out["inline"] = Value::Array(inline);
    Some(json!({"v": 1, "note": out}))
}

/// The losing side of an edit made on two devices, kept as a new note "… (other copy)".
fn other_copy(n: &Note) -> Note {
    let mut c = n.clone();
    c.id = crate::pb::new_uuid();
    if c.sealed.is_some() {
        // a locked note's rows keep their own ids: the copy is its title only
        c.sealed = None;
        c.locked = false;
        c.body = Body::from_lines(&format!("{} (other copy)", n.title), &["(locked on the other device)"]);
        return c;
    }
    // new ids for its attachments too (they're the copy's own)
    let mut ids: HashMap<String, String> = HashMap::new();
    for a in c.attachments.iter_mut() {
        let new = crate::pb::new_uuid();
        ids.insert(a.id.clone(), new.clone());
        a.id = new;
        for p in a.parts.iter_mut() {
            p.id = crate::pb::new_uuid();
        }
    }
    for t in c.body.tokens_mut() {
        if let Some(new) = ids.get(&t.id) {
            t.id = new.clone();
        }
    }
    if let Some(first) = c.body.paragraphs.first_mut() {
        first.spans.push(Span::plain(" (other copy)"));
    } else {
        c.body = Body::from_lines("(other copy)", &[]);
    }
    c
}

/// Folders as the server has them: when only theirs changed they replace ours; when both did,
/// ours where both have one, plus what's only there.
pub(crate) fn apply_folders(store: &mut Store, lib: &Library, theirs: &Value, mine_changed: bool) -> Result<()> {
    let list: Vec<&Value> = theirs["folders"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    // the accounts folders belong to (made here when missing; this library's own keeps its name)
    let account_names: HashMap<&str, &str> =
        theirs["accounts"].as_array().into_iter().flatten().filter_map(|a| Some((a["id"].as_str()?, a["name"].as_str().unwrap_or("Notes")))).collect();
    let account_of = |f: &Value| -> Option<(String, String)> {
        let id = f["account"].as_str().filter(|a| !a.is_empty())?;
        Some((id.to_string(), account_names.get(id).copied().unwrap_or(id).to_string()))
    };
    let ids: HashSet<&str> = list.iter().filter_map(|f| f["id"].as_str()).collect();
    // parents before children
    let mut pending: Vec<&Value> = list.clone();
    let mut known: HashSet<String> = lib.folders.iter().map(|f| f.id.clone()).collect();
    for _ in 0..16 {
        let mut next = Vec::new();
        for f in pending {
            let (Some(id), name) = (f["id"].as_str(), f["name"].as_str().unwrap_or_default()) else { continue };
            let parent = f["parent"].as_str().filter(|p| ids.contains(p) || known.contains(*p));
            if parent.is_some_and(|p| !known.contains(p)) {
                next.push(f);
                continue;
            }
            let smart = f["smart"].as_str();
            let account = account_of(f);
            match lib.folder(id) {
                None => {
                    store.create_folder_in_account(id, name, parent, smart, account.as_ref().map(|(i, n)| (i.as_str(), n.as_str())))?;
                }
                Some(have) if !mine_changed => {
                    if let Some((aid, aname)) = account.as_ref().filter(|(a, _)| *a != have.account) {
                        let pk = store.ensure_account_named(aid, aname)?;
                        store.set_folder_account(id, pk)?;
                        if parent.is_some() {
                            store.move_folder(id, parent)?;
                        }
                    }
                    if have.name != name {
                        store.rename_folder(id, name)?;
                    }
                    if have.parent.as_deref() != parent {
                        store.move_folder(id, parent)?;
                    }
                    if let (Some(q), true) = (smart, have.smart.as_deref() != smart) {
                        store.set_smart_query(id, q)?;
                    }
                }
                Some(_) => {}
            }
            if let Some(o) = f["order"].as_i64() {
                if lib.folder(id).is_none_or(|h| h.sort_order != o && !mine_changed) {
                    store.set_folder_order(id, o)?;
                }
            }
            known.insert(id.to_string());
        }
        if next.is_empty() {
            break;
        }
        pending = next;
    }
    if !mine_changed {
        // deleted there: here too (their notes go to Recently Deleted, as Notes does)
        for f in lib.folders.iter().filter(|f| !f.default && f.kind != FolderKind::Trash && !ids.contains(f.id.as_str())) {
            if store.library_meta()?.folder(&f.id).is_some() {
                store.delete_folder(&f.id)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod redirect_tests {
    use super::redirect_target;

    #[test]
    fn the_password_follows_only_to_the_same_server() {
        let from = "http://nas.local/dav/Notes";
        assert_eq!(redirect_target(from, "/dav/Notes/").as_deref(), Some("http://nas.local/dav/Notes/"));
        assert_eq!(redirect_target(from, "https://nas.local/dav/Notes").as_deref(), Some("https://nas.local/dav/Notes"));
        assert_eq!(redirect_target("https://nas.local:8443/d", "/e").as_deref(), Some("https://nas.local:8443/e"));
        assert_eq!(redirect_target("https://nas.local/d", "http://nas.local/d"), None);
        assert_eq!(redirect_target(from, "http://elsewhere.example/dav"), None);
        assert_eq!(redirect_target(from, "//elsewhere.example/dav"), None);
    }
}
