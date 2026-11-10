//! Update notices for every Bramble Office app: a background check of the latest GitHub release
//! (at most once a day, cached in ~/.cache/bramble/update.json), a small red dot on the app menu
//! and its "Update Available…" item when a newer version is out, and a sheet with the version,
//! a friendly list of what's new and an Update button. How Update works follows how the app was
//! installed (Flatpak, Snap, AppImage, unpacked AppImage from install.sh, a downloaded binary, or
//! a development build). Nothing is installed until the person presses the button.
//!
//! Each app calls `init` once at start, `title_dot` after drawing its app menu's title,
//! `menu_item` in that menu (after About), `menu_footer` at its bottom and `sheet` every frame.
//!
//! Test hooks: BRAMBLE_UPDATE_FAKE=<version> pretends that version is out (no network);
//! BRAMBLE_UPDATE_OPEN=1 opens the sheet at start; BRAMBLE_NO_UPDATE_CHECK=1 turns checks off;
//! BRAMBLE_UPDATE_INSTALL=appimage|binary|flatpak|snap|other shows that kind of installation's sheet.

use std::cmp::Ordering;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AO};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LATEST_URL: &str = "https://api.github.com/repos/bramble-apps/bramble-office/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/bramble-apps/bramble-office/releases/latest";
/// A generic name, nothing about the person or their computer.
const UA: &str = "bramble-office-updater";
const DAY: u64 = 24 * 60 * 60;
/// The badge's red (macOS's notification red, a touch softer).
pub const BADGE_RED: egui::Color32 = egui::Color32::from_rgb(0xF0, 0x44, 0x3A);

/// What the app is called in the release files.
#[derive(Clone, Debug)]
pub struct AppInfo {
    /// The name people see ("Keydeck", "Bramble Notes").
    pub name: &'static str,
    /// env!("CARGO_PKG_VERSION").
    pub version: &'static str,
    /// The binary's name in the suite archives ("keydeck", "bramble-notes").
    pub bin: &'static str,
    /// The app's AppImage among the release files ("Keydeck-x86_64.AppImage").
    pub appimage: &'static str,
    /// The macOS bundle in the suite zip ("Keydeck.app").
    pub mac_bundle: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// "v0.5.0"
    pub tag: String,
    /// The release notes (GitHub markdown).
    pub notes: String,
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn version(&self) -> &str {
        self.tag.trim_start_matches(['v', 'V'])
    }
}

/// GitHub's release JSON (the API's, or the cached copy) -> Release.
pub fn parse_release(json: &[u8]) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_slice(json).ok()?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let tag = s("tag_name");
    if tag.is_empty() || v.get("draft").and_then(|d| d.as_bool()) == Some(true) || v.get("prerelease").and_then(|d| d.as_bool()) == Some(true) {
        return None;
    }
    let assets = v
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some(Asset {
                        name: x.get("name")?.as_str()?.to_string(),
                        url: x.get("browser_download_url")?.as_str()?.to_string(),
                        size: x.get("size").and_then(|n| n.as_u64()).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let page = s("html_url");
    Some(Release { tag, notes: s("body"), page: if page.is_empty() { RELEASES_PAGE.into() } else { page }, assets })
}

fn release_json(r: &Release) -> serde_json::Value {
    serde_json::json!({
        "tag_name": r.tag, "body": r.notes, "html_url": r.page,
        "assets": r.assets.iter().map(|a| serde_json::json!({"name": a.name, "browser_download_url": a.url, "size": a.size})).collect::<Vec<_>>(),
    })
}

/// "v1.2.3-beta.1" -> ([1, 2, 3], Some("beta.1")); None when it isn't a version.
fn parse_version(s: &str) -> Option<(Vec<u64>, Option<String>)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let s = s.split('+').next()?;
    let (main, pre) = match s.split_once('-') {
        Some((m, p)) => (m, Some(p.to_string())),
        None => (s, None),
    };
    let nums: Option<Vec<u64>> = main.split('.').map(|p| p.parse().ok()).collect();
    let nums = nums.filter(|n| !n.is_empty())?;
    Some((nums, pre))
}

/// Orders two versions ("0.10.0" > "0.9.3", "1.0.0" > "1.0.0-rc.1"); None if either isn't one.
pub fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    let (an, ap) = parse_version(a)?;
    let (bn, bp) = parse_version(b)?;
    let n = an.len().max(bn.len());
    for i in 0..n {
        let (x, y) = (an.get(i).copied().unwrap_or(0), bn.get(i).copied().unwrap_or(0));
        if x != y {
            return Some(x.cmp(&y));
        }
    }
    Some(match (ap, bp) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => x.cmp(&y),
    })
}

/// Whether `latest` is newer than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    compare_versions(latest, current) == Some(Ordering::Greater)
}

/// One line of the friendly change list.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Heading(String),
    Bullet(String),
    Text(String),
}

/// Shown when a release says nothing a person would want to read.
pub const NO_NOTES: &str = "Fixes and improvements to make things run more smoothly.";

/// Inline markdown -> plain words: links keep their text, images, bare addresses, HTML and
/// GitHub's "by @someone in https://…" credits go, emphasis and code marks are dropped.
fn plain_inline(s: &str) -> String {
    let mut out = String::new();
    let ch: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < ch.len() {
        let c = ch[i];
        // ![alt](url): left out
        if c == '!'
            && ch.get(i + 1) == Some(&'[')
            && let Some(end) = link_end(&ch, i + 1)
        {
            i = end;
            continue;
        }
        // [text](url) -> text
        if c == '['
            && let Some(end) = link_end(&ch, i)
        {
            let close = ch[i..].iter().position(|&x| x == ']').map(|p| p + i).unwrap_or(i);
            out.extend(&ch[i + 1..close]);
            i = end;
            continue;
        }
        // <tags> and <!-- comments -->
        if c == '<'
            && let Some(p) = ch[i..].iter().position(|&x| x == '>')
        {
            i += p + 1;
            continue;
        }
        if c == '*' || c == '`' || (c == '_' && (i + 1 < ch.len() && ch[i + 1] == '_')) {
            i += if c == '_' { 2 } else { 1 };
            continue;
        }
        out.push(c);
        i += 1;
    }
    // GitHub's generated credits and bare addresses
    let mut words: Vec<&str> = Vec::new();
    let toks: Vec<&str> = out.split_whitespace().collect();
    let mut k = 0;
    while k < toks.len() {
        let t = toks[k];
        if t == "by" && toks.get(k + 1).is_some_and(|n| n.starts_with('@')) {
            k += 2;
            if toks.get(k) == Some(&"in") {
                k += 2;
            }
            continue;
        }
        if t == "in" && toks.get(k + 1).is_some_and(|n| n.starts_with("http")) {
            k += 2;
            continue;
        }
        if t.starts_with("http://") || t.starts_with("https://") {
            k += 1;
            continue;
        }
        words.push(t);
        k += 1;
    }
    let mut s = words.join(" ");
    while s.ends_with([':', ' ', '-', '—']) && !s.is_empty() {
        s.pop();
    }
    s
}

/// The index just past "[text](url)" starting at `i` ('['), if that's what is there.
fn link_end(ch: &[char], i: usize) -> Option<usize> {
    let close = ch[i..].iter().position(|&x| x == ']')? + i;
    if ch.get(close + 1) != Some(&'(') {
        return None;
    }
    Some(ch[close..].iter().position(|&x| x == ')')? + close + 1)
}

/// Release notes (markdown) -> a short friendly list: headings, bullets and paragraphs as plain
/// text, GitHub's boilerplate ("Full Changelog", contributor credits, links) left out.
pub fn changelog(md: &str) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut para = String::new();
    let mut comment = false;
    let flush = |para: &mut String, out: &mut Vec<Line>| {
        let t = plain_inline(para);
        if !t.is_empty() {
            out.push(Line::Text(t));
        }
        para.clear();
    };
    for raw in md.replace("\r\n", "\n").lines() {
        let line = raw.trim();
        if comment {
            if line.contains("-->") {
                comment = false;
            }
            continue;
        }
        if line.starts_with("<!--") && !line.contains("-->") {
            comment = true;
            continue;
        }
        let lower = line.to_lowercase();
        if lower.contains("full changelog") || line.chars().all(|c| matches!(c, '-' | '=' | '*' | '_')) && !line.is_empty() {
            flush(&mut para, &mut out);
            continue;
        }
        if line.is_empty() {
            flush(&mut para, &mut out);
            continue;
        }
        if let Some(h) = line.strip_prefix('#') {
            flush(&mut para, &mut out);
            let h = plain_inline(h.trim_start_matches('#').trim());
            let friendly = match h.to_lowercase().replace('’', "'").as_str() {
                "what's changed" | "changes" | "changelog" | "release notes" => "What's new".to_string(),
                "new contributors" => "Welcome to new contributors".to_string(),
                "bug fixes" | "fixes" => "Fixes".to_string(),
                _ => h,
            };
            if !friendly.is_empty() {
                out.push(Line::Heading(friendly));
            }
            continue;
        }
        let bullet = ["- ", "* ", "+ "].iter().find_map(|b| line.strip_prefix(b)).or_else(|| {
            let d = line.find(|c: char| !c.is_ascii_digit())?;
            (d > 0).then(|| line[d..].strip_prefix(". ").or_else(|| line[d..].strip_prefix(") "))).flatten()
        });
        if let Some(b) = bullet {
            flush(&mut para, &mut out);
            let b = b.trim_start_matches("[x] ").trim_start_matches("[ ] ");
            let t = plain_inline(b);
            if !t.is_empty() {
                let mut c = t.chars();
                let t = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
                out.push(Line::Bullet(t));
            }
            continue;
        }
        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(line.trim_start_matches('>').trim());
    }
    flush(&mut para, &mut out);
    // a heading with nothing under it says nothing
    let mut kept: Vec<Line> = Vec::new();
    for (i, l) in out.iter().enumerate() {
        if matches!(l, Line::Heading(_)) && !matches!(out.get(i + 1), Some(Line::Bullet(_) | Line::Text(_))) {
            continue;
        }
        kept.push(l.clone());
    }
    if kept.is_empty() {
        kept.push(Line::Text(NO_NOTES.into()));
    }
    kept
}

/// How the app was installed, which decides what Update does.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// `flatpak update <id>`
    Flatpak(String),
    /// `snap refresh <name>`
    Snap(String),
    /// A local build (cargo's target folder, or the suite launcher in ~/.local/lib/suite-launch):
    /// updated by pulling and rebuilding.
    Dev,
    /// The AppImage at this path (APPIMAGE).
    AppImage(PathBuf),
    /// An AppImage unpacked by install.sh: this folder holds AppRun and usr/bin.
    Unpacked(PathBuf),
    /// A binary from the suite download (the .exe, the Linux binary, or the .app on macOS).
    Binary(PathBuf),
    /// Somewhere this app can't replace itself (a system folder, Android): the download page.
    Other,
}

/// The installation from the environment's facts (kept apart from `detect_install` for tests).
pub fn detect_from(flatpak: Option<&str>, snap: Option<&str>, appimage: Option<&str>, exe: Option<&Path>, home: Option<&Path>, launcher: bool) -> Install {
    if let Some(id) = flatpak.filter(|s| !s.is_empty()) {
        return Install::Flatpak(id.to_string());
    }
    if let Some(n) = snap.filter(|s| !s.is_empty()) {
        return Install::Snap(n.to_string());
    }
    if let Some(p) = appimage.filter(|s| !s.is_empty()) {
        return Install::AppImage(PathBuf::from(p));
    }
    let Some(exe) = exe else { return Install::Other };
    let s = exe.to_string_lossy().replace('\\', "/");
    if s.contains("/target/release/") || s.contains("/target/debug/") || s.contains("/target/x86_64") || s.contains("/target/aarch64") {
        return Install::Dev;
    }
    if let Some(h) = home
        && launcher
        && exe.parent() == Some(&h.join(".local/bin"))
    {
        return Install::Dev;
    }
    // install.sh: <dir>/usr/bin/<bin> next to <dir>/AppRun
    if let Some(dir) = exe.parent().and_then(|p| p.parent()).and_then(|p| p.parent())
        && exe.parent().and_then(|p| p.file_name()) == Some("bin".as_ref())
        && dir.join("AppRun").exists()
    {
        return Install::Unpacked(dir.to_path_buf());
    }
    if cfg!(target_os = "android") || s.starts_with("/usr/") || s.starts_with("/opt/") || s.starts_with("/snap/") || s.starts_with("/app/") {
        return Install::Other;
    }
    Install::Binary(exe.to_path_buf())
}

/// How this copy of the app was installed.
pub fn detect_install() -> Install {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let launcher = home.as_ref().is_some_and(|h| h.join(".local/lib/suite-launch/launch").exists());
    let exe = std::env::current_exe().ok().map(|p| p.canonicalize().unwrap_or(p));
    let env = |k: &str| std::env::var(k).ok();
    detect_from(env("FLATPAK_ID").as_deref(), env("SNAP_NAME").as_deref(), env("APPIMAGE").as_deref(), exe.as_deref(), home.as_deref(), launcher)
}

/// The suite download for this computer (None where there is none).
fn suite_archive() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("Bramble-Office-linux-x86_64.tar.gz")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("Bramble-Office-windows-x86_64.zip")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("Bramble-Office-macos-apple-silicon.zip")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("Bramble-Office-macos-intel.zip")
    } else {
        None
    }
}

// ---------------------------------------------------------------------------------------------
// state

#[derive(Clone, Debug, PartialEq)]
enum Step {
    /// What's new, and the Update button.
    Info,
    /// "Download … and replace …?" (downloads only)
    Confirm,
    Working(String),
    /// Done; the text, and whether Restart can start the new version.
    Done(String, bool),
    Failed(String),
}

struct State {
    info: AppInfo,
    release: Option<Release>,
    install: Install,
    open: bool,
    step: Step,
    /// What Restart starts.
    restart: Option<std::process::Command>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static CTX: OnceLock<egui::Context> = OnceLock::new();
static GOT: AtomicU64 = AtomicU64::new(0);
static TOTAL: AtomicU64 = AtomicU64::new(0);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    g.as_mut().map(f)
}

fn repaint() {
    if let Some(c) = CTX.get() {
        c.request_repaint();
    }
}

fn set_step(s: Step) {
    with(|st| st.step = s);
    repaint();
}

/// Once at start: remembers the app and checks for a newer release in the background.
pub fn init(info: AppInfo) {
    let mut install = detect_install();
    // (screenshots: BRAMBLE_UPDATE_INSTALL=appimage|binary|flatpak|snap|other shows that kind's sheet)
    if let Ok(k) = std::env::var("BRAMBLE_UPDATE_INSTALL") {
        let here = std::env::current_exe().unwrap_or_default();
        install = match k.as_str() {
            "appimage" => Install::AppImage(here.with_extension("AppImage")),
            "binary" => Install::Binary(here),
            "flatpak" => Install::Flatpak("io.github.bramble_apps.Example".into()),
            "snap" => Install::Snap("bramble-example".into()),
            "other" => Install::Other,
            _ => install,
        };
    }
    cleanup_old(&install);
    let open = std::env::var_os("BRAMBLE_UPDATE_OPEN").is_some();
    {
        let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_some() {
            return;
        }
        *g = Some(State { info: info.clone(), release: None, install, open: false, step: Step::Info, restart: None });
    }
    if let Ok(v) = std::env::var("BRAMBLE_UPDATE_FAKE") {
        let r = Release {
            tag: format!("v{}", v.trim_start_matches('v')),
            notes: "## What's Changed\n* **Update notices**: a small red dot shows when a new version is out by @someone in https://github.com/x/y/pull/1\n* Faster opening of large documents\n* Fixed: pasting into tables kept the old formatting\n\n**Full Changelog**: https://github.com/bramble-apps/bramble-office/compare/v0.4.2...v0.5.0".into(),
            page: RELEASES_PAGE.into(),
            // (names only: nothing is fetched unless Download and Install is pressed, which
            // then fails on the made-up address)
            assets: [info.appimage, suite_archive().unwrap_or("none")]
                .iter()
                .map(|n| Asset { name: n.to_string(), url: format!("https://invalid.invalid/{n}"), size: 48 << 20 })
                .collect(),
        };
        with(|s| {
            s.open = open;
            s.release = is_newer(r.version(), info.version).then_some(r);
        });
        return;
    }
    if std::env::var_os("BRAMBLE_NO_UPDATE_CHECK").is_some() || cfg!(target_os = "android") {
        return;
    }
    std::thread::Builder::new()
        .name("update-check".into())
        .spawn(move || {
            if let Some(r) = check() {
                with(|s| {
                    if is_newer(r.version(), s.info.version) {
                        s.release = Some(r);
                        s.open |= open;
                    }
                });
                repaint();
            }
        })
        .ok();
}

fn cache_file() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(if cfg!(target_os = "macos") { "Library/Caches" } else { ".cache" })))
        .unwrap_or_else(std::env::temp_dir);
    base.join("bramble").join("update.json")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The latest release: the cached one when it was checked less than a day ago, else asked of
/// GitHub (the attempt is remembered either way, so an offline computer asks once a day too).
fn check() -> Option<Release> {
    let path = cache_file();
    let cached: Option<serde_json::Value> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let checked = cached.as_ref().and_then(|c| c.get("checked")?.as_u64()).unwrap_or(0);
    let old = cached.as_ref().and_then(|c| c.get("release")).and_then(|r| parse_release(r.to_string().as_bytes()));
    let t = now();
    if t >= checked && t - checked < DAY {
        return old;
    }
    let fresh = fetch_latest();
    let keep = fresh.clone().or(old);
    let doc = serde_json::json!({"checked": t, "release": keep.as_ref().map(release_json)});
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, doc.to_string());
    keep
}

fn fetch_latest() -> Option<Release> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).user_agent(UA).build().into();
    let mut resp = agent.get(LATEST_URL).header("Accept", "application/vnd.github+json").call().ok()?;
    let body = resp.body_mut().with_config().limit(4 << 20).read_to_vec().ok()?;
    parse_release(&body)
}

/// A newer version is out.
pub fn available() -> bool {
    with(|s| s.release.is_some()).unwrap_or(false)
}

/// Opens the update sheet.
pub fn open_sheet() {
    with(|s| {
        s.open = true;
        if matches!(s.step, Step::Failed(_)) {
            s.step = Step::Info;
        }
    });
}

// ---------------------------------------------------------------------------------------------
// drawing

fn remember_ctx(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
}

/// The badge: a small red dot centred at `c`, ringed in the background colour so it reads on
/// any title.
pub fn paint_dot(painter: &egui::Painter, c: egui::Pos2, bg: egui::Color32) {
    painter.circle_filled(c, 4.0, bg);
    painter.circle_filled(c, 3.0, BADGE_RED);
}

/// After drawing the app menu's title (or the one button holding every menu): the dot at its
/// top right when an update is out.
pub fn title_dot(ui: &egui::Ui, title: egui::Rect) {
    remember_ctx(ui.ctx());
    if available() {
        paint_dot(ui.painter(), egui::pos2(title.max.x - 3.5, title.min.y + 5.0), ui.visuals().panel_fill);
    }
}

/// In the app menu, after About: "Update Available…" with its dot, when there is one. `lead` is
/// the width of the menu's icon column (0 when it has none), so the label lines up.
pub fn menu_item(ui: &mut egui::Ui, lead: f32) {
    remember_ctx(ui.ctx());
    if !available() {
        return;
    }
    let text: egui::Atoms = {
        use egui::IntoAtoms;
        // (room after the label for the dot)
        let dot = egui::Atom::custom(egui::IdSalt::new("update-dot"), egui::vec2(10.0, 8.0));
        if lead > 0.0 { (egui::Atom::custom(egui::IdSalt::new("update-lead"), egui::vec2(lead, 16.0)), "Update Available…", dot).into_atoms() } else { ("Update Available…", dot).into_atoms() }
    };
    let r = ui.add(egui::Button::new(text));
    let y = r.rect.center().y;
    let x = r.rect.max.x - ui.spacing().button_padding.x - 5.0;
    paint_dot(ui.painter(), egui::pos2(x, y), ui.visuals().window_fill);
    if r.clicked() {
        open_sheet();
        ui.close();
    }
}

/// A small heart centred at `c`, `r` wide each side of the middle.
pub fn paint_heart(p: &egui::Painter, c: egui::Pos2, r: f32, col: egui::Color32) {
    use egui::pos2;
    let k = r * 0.52;
    p.circle_filled(pos2(c.x - k, c.y - k * 0.45), k, col);
    p.circle_filled(pos2(c.x + k, c.y - k * 0.45), k, col);
    p.add(egui::Shape::convex_polygon(vec![pos2(c.x - 2.0 * k + 0.1, c.y - k * 0.2), pos2(c.x + 2.0 * k - 0.1, c.y - k * 0.2), pos2(c.x, c.y + 1.45 * k)], col, egui::Stroke::NONE));
}

/// The thank-you line with its Support development link, small and quiet.
fn thanks(ui: &mut egui::Ui, in_menu: bool) {
    let weak = ui.visuals().weak_text_color();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new("Thank you for using Bramble Office").size(11.5).color(weak));
        // (♥ drawn: not every app's font has the glyph)
        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 11.0), egui::Sense::hover());
        paint_heart(ui.painter(), r.center() + egui::vec2(0.0, 0.5), 4.2, weak);
        let link = ui.add(egui::Label::new(egui::RichText::new("Support development").size(11.5).color(ui.visuals().hyperlink_color)).sense(egui::Sense::click()));
        if link.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if link.on_hover_text(crate::DONATE_URL).clicked() {
            open_url(crate::DONATE_URL);
            if in_menu {
                ui.close();
            }
        }
    });
}

/// At the bottom of the app menu: a separator and the thank-you line.
pub fn menu_footer(ui: &mut egui::Ui) {
    remember_ctx(ui.ctx());
    ui.separator();
    ui.add_space(1.0);
    egui::Frame::new().inner_margin(egui::Margin::symmetric(ui.spacing().button_padding.x as i8, 0)).show(ui, |ui| thanks(ui, true));
    ui.add_space(2.0);
}

fn open_url(url: &str) {
    let url = url.to_string();
    std::thread::spawn(move || {
        use std::process::Stdio;
        let _ = crate::open_command(&url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    });
}

fn mb(n: u64) -> String {
    format!("{:.0} MB", (n as f64 / 1_048_576.0).max(1.0))
}

/// The sheet's button row, right-aligned (default button rightmost, as on macOS).
fn buttons(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 26.0), egui::Layout::right_to_left(egui::Align::Center), add);
}

fn primary(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add(egui::Button::new(egui::RichText::new(label).color(egui::Color32::WHITE)).fill(crate::theme::ACCENT()).min_size(egui::vec2(90.0, 24.0))).clicked()
}

fn secondary(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add(egui::Button::new(label).min_size(egui::vec2(80.0, 24.0))).clicked()
}

/// What Update will do, in a sentence.
fn plan(install: &Install, name: &str) -> String {
    match install {
        Install::Flatpak(_) => "Update installs it through Flatpak.".into(),
        Install::Snap(_) => "Update asks the Snap Store for it (snaps also update by themselves).".into(),
        Install::Dev => format!("This is a development build of {name}, so it doesn't update itself: pull the latest code and rebuild."),
        Install::AppImage(p) => format!("Update downloads the new AppImage and puts it in place of {}.", p.display()),
        Install::Unpacked(_) => format!("Update downloads the new {name} and puts it where this one is installed."),
        Install::Binary(_) => format!("Update downloads the new {name} and puts it in place of this copy."),
        Install::Other => "Download it from the Bramble Office releases page.".into(),
    }
}

/// Every frame: the update sheet, when it's open.
pub fn sheet(ctx: &egui::Context) {
    remember_ctx(ctx);
    let snapshot = with(|s| (s.open, s.release.clone(), s.info.clone(), s.install.clone(), s.step.clone()));
    let fresh_id = egui::Id::unique("kn-update-sheet-frames");
    let Some((true, Some(rel), info, install, step)) = snapshot else {
        ctx.data_mut(|d| d.remove::<u32>(fresh_id));
        return;
    };
    // (the first frames size the window: draw a few more so it settles centred)
    let n = ctx.data_mut(|d| {
        let n = d.get_temp_mut_or(fresh_id, 0u32);
        *n += 1;
        *n
    });
    if n < 4 {
        ctx.request_repaint();
    }
    let mut open = true;
    let mut next: Option<Step> = None;
    let mut close = false;
    let mut restart = false;
    // (narrower on a small window)
    let width = (ctx.content_rect().width() - 60.0).clamp(260.0, 420.0);
    let notes_h = (ctx.content_rect().height() - 330.0).clamp(80.0, 200.0);
    egui::Window::new("Software Update")
        // (a sheet: centred on the window, where it stays)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.label(egui::RichText::new(format!("{} {} is available", info.name, rel.version())).size(16.0).strong());
            ui.label(egui::RichText::new(format!("You have version {}.", info.version)).color(ui.visuals().weak_text_color()));
            ui.add_space(8.0);
            ui.label(egui::RichText::new("What's new").strong());
            ui.add_space(2.0);
            egui::Frame::new().fill(ui.visuals().extreme_bg_color).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().max_height(notes_h).auto_shrink([false, true]).show(ui, |ui| {
                    let mut lines = changelog(&rel.notes);
                    // (the sheet already says "What's new")
                    if lines.first() == Some(&Line::Heading("What's new".into())) {
                        lines.remove(0);
                    }
                    for l in lines {
                        match l {
                            Line::Heading(h) => {
                                ui.add_space(3.0);
                                ui.label(egui::RichText::new(h).strong());
                            }
                            Line::Bullet(b) => {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    ui.label("•");
                                    ui.label(b);
                                });
                            }
                            Line::Text(t) => {
                                ui.label(t);
                            }
                        }
                    }
                });
            });
            let page = ui.add(egui::Label::new(egui::RichText::new("Full release notes").size(12.0).color(ui.visuals().hyperlink_color)).sense(egui::Sense::click()));
            if page.clicked() {
                open_url(&rel.page);
            }
            ui.add_space(8.0);
            match &step {
                Step::Info => {
                    ui.label(egui::RichText::new(plan(&install, info.name)).color(ui.visuals().weak_text_color()));
                    ui.add_space(6.0);
                    buttons(ui, |ui| match &install {
                        Install::Dev => {
                            if primary(ui, "OK") {
                                close = true;
                            }
                        }
                        Install::Other => {
                            if primary(ui, "Open Download Page") {
                                open_url(&rel.page);
                                close = true;
                            }
                            if secondary(ui, "Not Now") {
                                close = true;
                            }
                        }
                        Install::Flatpak(_) | Install::Snap(_) => {
                            if primary(ui, "Update") {
                                start();
                            }
                            if secondary(ui, "Not Now") {
                                close = true;
                            }
                        }
                        _ => {
                            if primary(ui, "Update…") {
                                next = Some(Step::Confirm);
                            }
                            if secondary(ui, "Not Now") {
                                close = true;
                            }
                        }
                    });
                }
                Step::Confirm => {
                    let (what, size) = download_choice(&info, &install, &rel).map(|a| (a.name.clone(), a.size)).unwrap_or_default();
                    let target = match &install {
                        Install::AppImage(p) | Install::Binary(p) | Install::Unpacked(p) => p.display().to_string(),
                        _ => String::new(),
                    };
                    let text = if what.is_empty() {
                        format!("This release has no download of {} for this computer yet.", info.name)
                    } else {
                        format!("Download {what} ({}) from GitHub and replace {target} with it? Your documents and settings stay as they are.", mb(size))
                    };
                    ui.label(text);
                    ui.add_space(6.0);
                    buttons(ui, |ui| {
                        if what.is_empty() {
                            if primary(ui, "Open Download Page") {
                                open_url(&rel.page);
                                close = true;
                            }
                        } else if primary(ui, "Download and Install") {
                            start();
                        }
                        if secondary(ui, "Cancel") {
                            next = Some(Step::Info);
                        }
                    });
                }
                Step::Working(t) => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label(t);
                    });
                    let (got, total) = (GOT.load(AO::Relaxed), TOTAL.load(AO::Relaxed));
                    if total > 0 {
                        ui.add(egui::ProgressBar::new(got as f32 / total as f32).desired_height(6.0).text(egui::RichText::new(format!("{} of {}", mb(got), mb(total))).size(11.0)));
                    }
                }
                Step::Done(t, can) => {
                    ui.label(t);
                    ui.add_space(6.0);
                    buttons(ui, |ui| {
                        if *can {
                            if primary(ui, "Restart Now") {
                                restart = true;
                            }
                            if secondary(ui, "Later") {
                                close = true;
                            }
                        } else if primary(ui, "OK") {
                            close = true;
                        }
                    });
                }
                Step::Failed(t) => {
                    ui.label(t);
                    let link = ui.add(egui::Label::new(egui::RichText::new("Get it from the releases page instead").color(ui.visuals().hyperlink_color)).sense(egui::Sense::click()));
                    if link.clicked() {
                        open_url(&rel.page);
                    }
                    ui.add_space(6.0);
                    buttons(ui, |ui| {
                        if primary(ui, "Close") {
                            close = true;
                        }
                        if secondary(ui, "Try Again") {
                            next = Some(Step::Info);
                        }
                    });
                }
            }
            ui.add_space(6.0);
            ui.separator();
            thanks(ui, false);
        });
    if matches!(step, Step::Working(_)) {
        // the progress bar moves on its own
        ctx.request_repaint_after(Duration::from_millis(200));
    }
    if restart {
        let cmd = with(|s| s.restart.take()).flatten();
        if let Some(mut c) = cmd {
            use std::process::Stdio;
            if c.stdin(Stdio::null()).spawn().is_ok() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        close = true;
    }
    with(|s| {
        if let Some(n) = next {
            s.step = n;
        }
        // working on: the window stays until the work is done
        if (!open || close) && !matches!(s.step, Step::Working(_)) {
            s.open = false;
            if !matches!(s.step, Step::Done(..)) {
                s.step = Step::Info;
            }
        }
    });
}

/// The release file this installation updates from.
fn download_choice<'a>(info: &AppInfo, install: &Install, rel: &'a Release) -> Option<&'a Asset> {
    let want = match install {
        Install::AppImage(_) | Install::Unpacked(_) => info.appimage,
        Install::Binary(_) => suite_archive()?,
        _ => return None,
    };
    rel.assets.iter().find(|a| a.name == want)
}

/// The Update / Download and Install button: does the work in a background thread.
fn start() {
    let Some((info, install, rel)) = with(|s| (s.info.clone(), s.install.clone(), s.release.clone())) else { return };
    let Some(rel) = rel else { return };
    GOT.store(0, AO::Relaxed);
    TOTAL.store(0, AO::Relaxed);
    set_step(Step::Working(match install {
        Install::Flatpak(_) | Install::Snap(_) => format!("Updating {}…", info.name),
        _ => format!("Downloading {} {}…", info.name, rel.version()),
    }));
    std::thread::spawn(move || {
        let r = run_update(&info, &install, &rel);
        match r {
            Ok((text, cmd)) => {
                let can = cmd.is_some();
                with(|s| s.restart = cmd);
                set_step(Step::Done(text, can));
            }
            Err(e) => set_step(Step::Failed(e)),
        }
    });
}

type Restart = Option<std::process::Command>;

fn run_update(info: &AppInfo, install: &Install, rel: &Release) -> Result<(String, Restart), String> {
    use std::process::{Command, Stdio};
    let restart_msg = format!("{} {} is installed. Restart {} to start using it — save your work first.", info.name, rel.version(), info.name);
    let quiet = |c: &mut Command| -> Result<std::process::Output, String> {
        c.stdin(Stdio::null()).output().map_err(|e| format!("Couldn't start the update ({e})."))
    };
    match install {
        Install::Flatpak(id) => {
            // inside the sandbox flatpak runs on the host through flatpak-spawn
            let out = quiet(Command::new("flatpak-spawn").args(["--host", "flatpak", "update", "-y", "--noninteractive", id]))
                .or_else(|_| quiet(Command::new("flatpak").args(["update", "-y", "--noninteractive", id])))?;
            if !out.status.success() {
                return Err(format!("Flatpak couldn't update {}. You can run “flatpak update {id}” in a terminal, or use your software centre.", info.name));
            }
            let mut c = Command::new("flatpak-spawn");
            c.args(["--host", "flatpak", "run", id]);
            Ok((restart_msg, Some(c)))
        }
        Install::Snap(name) => {
            let out = quiet(Command::new("snap").args(["refresh", name]))?;
            if !out.status.success() {
                return Err(format!("The Snap Store couldn't be asked from inside the app. Snaps update by themselves; to update now run “snap refresh {name}” in a terminal."));
            }
            Ok((format!("{} {} is installed. Close {} and open it again to start using it.", info.name, rel.version(), info.name), None))
        }
        Install::Dev | Install::Other => Err("This copy can't update itself.".into()),
        Install::AppImage(path) => {
            let asset = download_choice(info, install, rel).ok_or_else(|| format!("This release has no AppImage of {} yet.", info.name))?;
            let tmp = sibling(path, "download");
            download(&asset.url, &tmp, asset.size)?;
            set_exec(&tmp);
            std::fs::rename(&tmp, path).map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("Couldn't replace {} ({e}).", path.display())
            })?;
            Ok((restart_msg, Some(Command::new(path))))
        }
        Install::Unpacked(dir) => {
            let asset = download_choice(info, install, rel).ok_or_else(|| format!("This release has no AppImage of {} yet.", info.name))?;
            let work = sibling(dir, "update");
            let _ = std::fs::remove_dir_all(&work);
            std::fs::create_dir_all(&work).map_err(|e| format!("Couldn't make a folder next to {} ({e}).", dir.display()))?;
            let image = work.join("new.AppImage");
            let r = (|| {
                download(&asset.url, &image, asset.size)?;
                set_exec(&image);
                set_step(Step::Working(format!("Installing {}…", info.name)));
                let ok = Command::new(&image).arg("--appimage-extract").current_dir(&work).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
                if !ok {
                    return Err("Couldn't unpack the new version.".to_string());
                }
                swap_dir(&work.join("squashfs-root"), dir)
            })();
            let _ = std::fs::remove_dir_all(&work);
            r?;
            Ok((restart_msg, Some(Command::new(dir.join("AppRun")))))
        }
        Install::Binary(exe) => {
            let asset = download_choice(info, install, rel).ok_or_else(|| "This release has no download for this computer yet.".to_string())?;
            let work = sibling(exe, "update");
            let _ = std::fs::remove_dir_all(&work);
            std::fs::create_dir_all(&work).map_err(|e| format!("Couldn't make a folder next to {} ({e}).", exe.display()))?;
            let archive = work.join(&asset.name);
            let r = (|| {
                download(&asset.url, &archive, asset.size)?;
                set_step(Step::Working(format!("Installing {}…", info.name)));
                install_from_archive(info, exe, &archive, &work)
            })();
            let _ = std::fs::remove_dir_all(&work);
            let started = r?;
            Ok((restart_msg, Some(Command::new(started))))
        }
    }
}

/// A scratch path next to `p` (same disk, so renames work).
fn sibling(p: &Path, what: &str) -> PathBuf {
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    p.with_file_name(format!(".{name}.{what}"))
}

fn set_exec(p: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
    }
    let _ = p;
}

/// Puts the folder `new` where `old` is (the old one out of the way first, gone after).
fn swap_dir(new: &Path, old: &Path) -> Result<(), String> {
    let aside = sibling(old, "old");
    let _ = std::fs::remove_dir_all(&aside);
    std::fs::rename(old, &aside).map_err(|e| format!("Couldn't move {} aside ({e}).", old.display()))?;
    if let Err(e) = std::fs::rename(new, old) {
        let _ = std::fs::rename(&aside, old);
        return Err(format!("Couldn't put the new version in place ({e})."));
    }
    let _ = std::fs::remove_dir_all(&aside);
    Ok(())
}

/// Leftovers of the last update (Windows can't delete a running .exe, so it's renamed aside).
fn cleanup_old(install: &Install) {
    if let Install::Binary(exe) = install {
        let _ = std::fs::remove_file(sibling(exe, "old"));
    }
}

fn download(url: &str, to: &Path, size: u64) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_per_read(Some(Duration::from_secs(60)))
        .user_agent(UA)
        .build()
        .into();
    let mut resp = agent.get(url).call().map_err(|_| "Couldn't reach GitHub. Check your internet connection and try again.".to_string())?;
    TOTAL.store(size, AO::Relaxed);
    let mut reader = resp.body_mut().with_config().limit(4 << 30).reader();
    let mut f = std::fs::File::create(to).map_err(|e| format!("Couldn't save the download ({e})."))?;
    let mut buf = vec![0u8; 64 << 10];
    let mut got = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|_| "The download stopped part way. Try again in a moment.".to_string())?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).map_err(|e| format!("Couldn't save the download ({e})."))?;
        got += n as u64;
        GOT.store(got, AO::Relaxed);
        repaint();
    }
    f.flush().map_err(|e| format!("Couldn't save the download ({e})."))?;
    if size > 0 && got != size {
        let _ = std::fs::remove_file(to);
        return Err("The download stopped part way. Try again in a moment.".into());
    }
    Ok(())
}

/// The suite archive -> this app's binary (or bundle) in place of the running one; returns what
/// Restart starts.
fn install_from_archive(info: &AppInfo, exe: &Path, archive: &Path, work: &Path) -> Result<PathBuf, String> {
    let missing = || format!("This release doesn't include {} for this computer yet.", info.name);
    if cfg!(target_os = "macos") {
        // exe = …/Name.app/Contents/MacOS/bin: the whole bundle is replaced
        let bundle = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).ok_or_else(|| "Couldn't find the app's bundle.".to_string())?.to_path_buf();
        let prefix = format!("bramble-office/{}/", info.mac_bundle);
        let out = work.join("new");
        unzip(archive, &prefix, &out)?;
        if !out.exists() {
            return Err(missing());
        }
        swap_dir(&out, &bundle)?;
        return Ok(exe.to_path_buf());
    }
    let new = work.join("new-bin");
    if cfg!(target_os = "windows") {
        let name = format!("bramble-office/{}.exe", info.bin);
        let mut z = zip::ZipArchive::new(std::fs::File::open(archive).map_err(|e| e.to_string())?).map_err(|_| "The download isn't a valid archive.".to_string())?;
        let mut e = z.by_name(&name).map_err(|_| missing())?;
        let mut f = std::fs::File::create(&new).map_err(|e| e.to_string())?;
        std::io::copy(&mut e, &mut f).map_err(|_| "Couldn't unpack the new version.".to_string())?;
    } else {
        let member = format!("bramble-office/{}", info.bin);
        let ok = std::process::Command::new("tar").arg("-xzf").arg(archive).arg("-C").arg(work).arg(&member).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
        if !ok {
            return Err(missing());
        }
        std::fs::rename(work.join(&member), &new).map_err(|_| missing())?;
    }
    set_exec(&new);
    // (a running .exe can be renamed but not overwritten; on Linux a rename replaces it)
    let aside = sibling(exe, "old");
    if cfg!(target_os = "windows") {
        let _ = std::fs::remove_file(&aside);
        std::fs::rename(exe, &aside).map_err(|e| format!("Couldn't move {} aside ({e}).", exe.display()))?;
    }
    if let Err(e) = std::fs::rename(&new, exe) {
        if cfg!(target_os = "windows") {
            let _ = std::fs::rename(&aside, exe);
        }
        return Err(format!("Couldn't replace {} ({e}).", exe.display()));
    }
    Ok(exe.to_path_buf())
}

/// Unpacks the zip entries under `prefix` into `out` (keeping Unix permissions).
fn unzip(archive: &Path, prefix: &str, out: &Path) -> Result<(), String> {
    let bad = || "Couldn't unpack the new version.".to_string();
    let mut z = zip::ZipArchive::new(std::fs::File::open(archive).map_err(|e| e.to_string())?).map_err(|_| "The download isn't a valid archive.".to_string())?;
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|_| bad())?;
        let Some(rel) = e.enclosed_name() else { continue };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let Some(rest) = rel.strip_prefix(prefix) else { continue };
        let dest = out.join(rest);
        if e.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|_| bad())?;
            continue;
        }
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d).map_err(|_| bad())?;
        }
        #[cfg(unix)]
        if e.is_symlink() {
            let mut target = String::new();
            e.read_to_string(&mut target).map_err(|_| bad())?;
            let _ = std::os::unix::fs::symlink(target, &dest);
            continue;
        }
        let mut f = std::fs::File::create(&dest).map_err(|_| bad())?;
        std::io::copy(&mut e, &mut f).map_err(|_| bad())?;
        #[cfg(unix)]
        if let Some(m) = e.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(m & 0o7777));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("v0.5.0", "0.4.2"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.99"));
        assert!(is_newer("v1.0.0", "1.0.0-rc.1"));
        assert!(is_newer("0.4.2.1", "0.4.2"));
        assert!(!is_newer("v0.4.2", "0.4.2"));
        assert!(!is_newer("0.4.2", "0.4.10"));
        assert!(!is_newer("0.4", "0.4.0"));
        assert!(!is_newer("1.0.0-rc.1", "1.0.0"));
        assert!(!is_newer("nightly", "0.4.2"));
        assert!(!is_newer("", "0.4.2"));
        assert_eq!(compare_versions("V2.0.0+build5", "2.0.0"), Some(Ordering::Equal));
    }

    #[test]
    fn github_boilerplate_only() {
        let l = changelog("**Full Changelog**: https://github.com/bramble-apps/bramble-office/compare/v0.4.1...v0.4.2");
        assert_eq!(l, vec![Line::Text(NO_NOTES.into())]);
        assert_eq!(changelog(""), vec![Line::Text(NO_NOTES.into())]);
    }

    #[test]
    fn generated_notes() {
        let md = "<!-- Release notes generated using configuration in .github/release.yml -->\r\n\r\n## What's Changed\r\n### New\r\n* **Keydeck:** smoother `transitions` by @alice in https://github.com/bramble-apps/bramble-office/pull/12\r\n* fixed [a crash](https://github.com/x/y/issues/3) when pasting\r\n\r\n## New Contributors\r\n* @alice made their first contribution in https://github.com/bramble-apps/bramble-office/pull/12\r\n\r\n**Full Changelog**: https://github.com/bramble-apps/bramble-office/compare/v0.4.2...v0.5.0";
        let l = changelog(md);
        assert_eq!(
            l,
            vec![
                Line::Heading("New".into()),
                Line::Bullet("Keydeck: smoother transitions".into()),
                Line::Bullet("Fixed a crash when pasting".into()),
                Line::Heading("Welcome to new contributors".into()),
                Line::Bullet("@alice made their first contribution".into()),
            ]
        );
    }

    #[test]
    fn hand_written_notes() {
        let md = "# Bramble Office 0.5\n\nThis release brings\nupdate notices.\n\n## Fixes\n- Values: tables keep their *styles*\n1. Letters opens __faster__\n\n---\n![shot](https://x/y.png)\n> Thanks for testing!";
        let l = changelog(md);
        assert_eq!(
            l,
            vec![
                Line::Heading("Bramble Office 0.5".into()),
                Line::Text("This release brings update notices.".into()),
                Line::Heading("Fixes".into()),
                Line::Bullet("Values: tables keep their styles".into()),
                Line::Bullet("Letters opens faster".into()),
                Line::Text("Thanks for testing!".into()),
            ]
        );
    }

    #[test]
    fn release_json_round_trip() {
        let j = br#"{"tag_name":"v0.5.0","html_url":"https://github.com/r","body":"hi","draft":false,"prerelease":false,
            "assets":[{"name":"Keydeck-x86_64.AppImage","browser_download_url":"https://d/k","size":42}]}"#;
        let r = parse_release(j).unwrap();
        assert_eq!(r.version(), "0.5.0");
        assert_eq!(r.assets[0], Asset { name: "Keydeck-x86_64.AppImage".into(), url: "https://d/k".into(), size: 42 });
        assert_eq!(parse_release(release_json(&r).to_string().as_bytes()), Some(r));
        assert!(parse_release(br#"{"tag_name":"v1","prerelease":true}"#).is_none());
        assert!(parse_release(b"not json").is_none());
    }

    /// The Linux suite archive's binary replaces the running copy (a stand-in file here).
    #[cfg(target_os = "linux")]
    #[test]
    fn binary_from_suite_archive() {
        let dir = std::env::temp_dir().join(format!("kn-update-bin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("pack/bramble-office")).unwrap();
        std::fs::write(dir.join("pack/bramble-office/keydeck"), "new").unwrap();
        std::fs::write(dir.join("pack/bramble-office/values"), "other").unwrap();
        let archive = dir.join("Bramble-Office-linux-x86_64.tar.gz");
        assert!(std::process::Command::new("tar").arg("-czf").arg(&archive).arg("-C").arg(dir.join("pack")).arg("bramble-office").status().unwrap().success());
        let exe = dir.join("bin/keydeck");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(&exe, "old").unwrap();
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let info = AppInfo { name: "Keydeck", version: "0.4.2", bin: "keydeck", appimage: "Keydeck-x86_64.AppImage", mac_bundle: "Keydeck.app" };
        assert_eq!(install_from_archive(&info, &exe, &archive, &work).unwrap(), exe);
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777, 0o755);
        // an app the archive doesn't have yet: a clear message, the old copy untouched
        let calc = dir.join("bin/calculator");
        std::fs::write(&calc, "old").unwrap();
        let info = AppInfo { name: "Calculator", bin: "calculator", ..info };
        assert!(install_from_archive(&info, &calc, &archive, &work).unwrap_err().contains("doesn't include Calculator"));
        assert_eq!(std::fs::read_to_string(&calc).unwrap(), "old");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn swapping_folders() {
        let dir = std::env::temp_dir().join(format!("kn-update-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("app")).unwrap();
        std::fs::create_dir_all(dir.join("new")).unwrap();
        std::fs::write(dir.join("app/AppRun"), "old").unwrap();
        std::fs::write(dir.join("new/AppRun"), "new").unwrap();
        swap_dir(&dir.join("new"), &dir.join("app")).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("app/AppRun")).unwrap(), "new");
        assert!(!sibling(&dir.join("app"), "old").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_kinds() {
        let home = Path::new("/home/u");
        let d = |f, s, a, e: &str, l| detect_from(f, s, a, Some(Path::new(e)), Some(home), l);
        assert_eq!(d(Some("io.github.bramble_apps.Keydeck"), None, None, "/app/bin/keydeck", false), Install::Flatpak("io.github.bramble_apps.Keydeck".into()));
        assert_eq!(d(None, Some("bramble-keydeck"), None, "/snap/bramble-keydeck/1/bin/keydeck", false), Install::Snap("bramble-keydeck".into()));
        assert_eq!(d(None, None, Some("/home/u/Apps/Keydeck.AppImage"), "/tmp/.mount_x/usr/bin/keydeck", false), Install::AppImage("/home/u/Apps/Keydeck.AppImage".into()));
        assert_eq!(d(None, None, None, "/home/u/src/bramble-office/keynote-linux/target/release/keydeck", false), Install::Dev);
        assert_eq!(d(None, None, None, "/home/u/.local/bin/keydeck", true), Install::Dev);
        assert_eq!(d(None, None, None, "/home/u/.local/bin/keydeck", false), Install::Binary("/home/u/.local/bin/keydeck".into()));
        assert_eq!(d(None, None, None, "/usr/bin/keydeck", false), Install::Other);
        assert_eq!(d(None, None, None, "C:\\Users\\u\\AppData\\Local\\Programs\\Bramble Office\\keydeck.exe", false), Install::Binary("C:\\Users\\u\\AppData\\Local\\Programs\\Bramble Office\\keydeck.exe".into()));
        // install.sh: an unpacked AppImage
        let dir = std::env::temp_dir().join(format!("kn-update-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("usr/bin")).unwrap();
        std::fs::write(dir.join("AppRun"), "").unwrap();
        let exe = dir.join("usr/bin/keydeck");
        assert_eq!(detect_from(None, None, None, Some(&exe), Some(home), false), Install::Unpacked(dir.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
