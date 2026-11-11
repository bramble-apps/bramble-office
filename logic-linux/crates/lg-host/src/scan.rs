//! Finding plug-ins: the standard folders for each format, every plug-in file looked at in a
//! separate process (a plug-in that crashes or hangs while loading takes only that process
//! down), and the results kept in Studio's settings (~/.config/bramble-studio/plug-ins.txt)
//! with the Plug-in Manager's choices (plug-in-manager.txt): plug-ins hidden from the menus,
//! ignored ones (not scanned, not loaded), crash counts.
//!
//! The scanner process is Studio itself run as `bramble-studio --scan-plugin FORMAT PATH`
//! (or the `lg-plugscan` binary); it prints one `PLUGIN` line per plug-in, or `ERROR` and a
//! reason, and exits.

use std::collections::{BTreeSet, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use crate::{Format, Kind, PluginInfo};

/// How a plug-in file fared in the scan (the Plug-in Manager's Compatibility column).
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Loaded, made, processed: usable.
    Ok,
    /// Loaded but refused (not a plug-in of the format, init failed, no plug-ins in it).
    Failed(String),
    /// Took the scanner process down, this many times.
    Crashed(u32),
    /// Didn't answer within the time limit.
    TimedOut,
    /// Ignored in the Plug-in Manager: not scanned.
    Ignored,
}

impl Status {
    pub fn ok(&self) -> bool {
        *self == Status::Ok
    }
    /// As the Plug-in Manager shows it.
    pub fn label(&self) -> String {
        match self {
            Status::Ok => "✓ validated".into(),
            Status::Failed(e) => format!("not compatible: {e}"),
            Status::Crashed(1) => "crashed once while scanning".into(),
            Status::Crashed(n) => format!("crashed {n} times while scanning"),
            Status::TimedOut => "didn't answer (timed out)".into(),
            Status::Ignored => "ignored (not scanned)".into(),
        }
    }
}

/// One plug-in file or bundle and what the scan found in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub format: Format,
    pub path: PathBuf,
    /// The file's modification time when scanned (seconds), to rescan only what changed.
    pub mtime: u64,
    pub status: Status,
    pub info: Option<PluginInfo>,
}

/// The folders a format's plug-ins are installed in (the user's, then the system's), plus
/// LG_PLUGIN_PATH's (colon-separated, every format: for checks).
pub fn folders(f: Format) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut v: Vec<PathBuf> = Vec::new();
    let env_list = |name: &str| std::env::var_os(name).map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    match f {
        Format::Clap => {
            v.extend(env_list("CLAP_PATH"));
            v.extend(home.iter().map(|h| h.join(".clap")));
            v.extend(["/usr/lib/clap", "/usr/local/lib/clap"].map(PathBuf::from));
        }
        Format::Vst3 => {
            v.extend(home.iter().map(|h| h.join(".vst3")));
            v.extend(["/usr/lib/vst3", "/usr/local/lib/vst3"].map(PathBuf::from));
        }
        Format::Lv2 => {
            let lp = env_list("LV2_PATH");
            if lp.is_empty() {
                v.extend(home.iter().map(|h| h.join(".lv2")));
                v.extend(["/usr/lib/lv2", "/usr/local/lib/lv2", "/usr/lib/x86_64-linux-gnu/lv2"].map(PathBuf::from));
            } else {
                v.extend(lp);
            }
        }
    }
    v.extend(env_list("LG_PLUGIN_PATH"));
    let mut seen = HashSet::new();
    v.retain(|p| seen.insert(p.clone()));
    v
}

/// The plug-in files and bundles in the folders: CLAP `.clap` files, VST3 `.vst3` bundles,
/// LV2 `.lv2` bundles (with a manifest.ttl). Sorted.
pub fn find_files() -> Vec<(Format, PathBuf)> {
    let mut out = BTreeSet::new();
    for f in Format::ALL {
        for dir in folders(f) {
            walk(&dir, f, 0, &mut out);
        }
    }
    out.into_iter().collect()
}

fn walk(dir: &Path, f: Format, depth: u32, out: &mut BTreeSet<(Format, PathBuf)>) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
        match (f, ext.as_str()) {
            (Format::Clap, "clap") if p.is_file() => {
                out.insert((f, p));
            }
            (Format::Vst3, "vst3") => {
                out.insert((f, p));
            }
            (Format::Lv2, "lv2") if p.join("manifest.ttl").is_file() => {
                out.insert((f, p));
            }
            _ if p.is_dir() && !matches!(ext.as_str(), "clap" | "vst3" | "lv2") => walk(&p, f, depth + 1, out),
            _ => {}
        }
    }
}

fn mtime(p: &Path) -> u64 {
    // a bundle's time is its newest file's (one level down is enough for the formats' layouts)
    let t = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let mut m = t(p);
    if p.is_dir() {
        for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
            m = m.max(t(&e.path()));
        }
    }
    m
}

fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio"))
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

/// One `PLUGIN` line (the scanner's output and the cache's).
pub fn info_line(i: &PluginInfo) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        i.format.label(),
        clean(&i.id),
        clean(&i.name),
        clean(&i.vendor),
        clean(&i.version),
        if i.kind == Kind::Instrument { "instrument" } else { "effect" },
        clean(&i.category),
        i.inputs,
        i.outputs,
        i.params,
        i.latency,
        if i.has_editor { 1 } else { 0 }
    )
}

pub fn parse_info(line: &str, path: &Path) -> Option<PluginInfo> {
    let w: Vec<&str> = line.split('\t').collect();
    if w.len() < 12 {
        return None;
    }
    Some(PluginInfo {
        format: Format::parse(w[0])?,
        id: w[1].into(),
        name: w[2].into(),
        vendor: w[3].into(),
        version: w[4].into(),
        kind: if w[5] == "instrument" { Kind::Instrument } else { Kind::Effect },
        category: w[6].into(),
        inputs: w[7].parse().ok()?,
        outputs: w[8].parse().ok()?,
        params: w[9].parse().ok()?,
        latency: w[10].parse().ok()?,
        has_editor: w[11] == "1",
        path: path.to_path_buf(),
    })
}

fn status_text(s: &Status) -> String {
    match s {
        Status::Ok => "ok".into(),
        Status::Failed(e) => format!("failed {}", clean(e)),
        Status::Crashed(n) => format!("crashed {n}"),
        Status::TimedOut => "timeout".into(),
        Status::Ignored => "ignored".into(),
    }
}

fn parse_status(s: &str) -> Status {
    let (w, rest) = s.split_once(' ').unwrap_or((s, ""));
    match w {
        "ok" => Status::Ok,
        "failed" => Status::Failed(rest.into()),
        "crashed" => Status::Crashed(rest.parse().unwrap_or(1)),
        "timeout" => Status::TimedOut,
        _ => Status::Ignored,
    }
}

/// The cache file: per entry a line `FORMAT \t path \t mtime \t status`, then its plug-in
/// line (`\t`-prefixed) when it has one.
pub fn cache_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LG_PLUGIN_CACHE") {
        return Some(PathBuf::from(p));
    }
    Some(config_dir()?.join("plug-ins.txt"))
}

pub fn load_cache() -> Vec<Entry> {
    let Some(p) = cache_path() else { return Vec::new() };
    let Ok(t) = std::fs::read_to_string(p) else { return Vec::new() };
    let mut out: Vec<Entry> = Vec::new();
    let mut file: Option<(Format, PathBuf, u64, Status)> = None;
    for l in t.lines() {
        if let Some(rest) = l.strip_prefix("PLUGIN\t") {
            if let Some((f, path, m, s)) = &file {
                if let Some(info) = parse_info(rest, path) {
                    out.push(Entry { format: *f, path: path.clone(), mtime: *m, status: s.clone(), info: Some(info) });
                }
            }
            continue;
        }
        let w: Vec<&str> = l.splitn(4, '\t').collect();
        if w.len() == 4 {
            if let Some(f) = Format::parse(w[0]) {
                let st = parse_status(w[3]);
                if !st.ok() {
                    out.push(Entry { format: f, path: PathBuf::from(w[1]), mtime: w[2].parse().unwrap_or(0), status: st.clone(), info: None });
                }
                file = Some((f, PathBuf::from(w[1]), w[2].parse().unwrap_or(0), st));
            }
        }
    }
    out
}

pub fn save_cache(list: &[Entry]) {
    let Some(p) = cache_path() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut s = String::from("# Bramble Studio's plug-in scan (rewritten by each scan)\n");
    let mut last: Option<&Path> = None;
    for e in list {
        if last != Some(e.path.as_path()) {
            s += &format!("{}\t{}\t{}\t{}\n", e.format.label(), e.path.display(), e.mtime, status_text(&e.status));
            last = Some(&e.path);
        }
        if let Some(i) = &e.info {
            s += &format!("PLUGIN\t{}\n", info_line(i));
        }
    }
    let _ = std::fs::write(p, s);
}

/// A plug-in's key for the manager's lists: "FORMAT|id".
pub fn key(i: &PluginInfo) -> String {
    format!("{}|{}", i.format.label(), i.id)
}

fn manager_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LG_PLUGIN_CACHE") {
        return Some(PathBuf::from(p).with_extension("manager.txt"));
    }
    Some(config_dir()?.join("plug-in-manager.txt"))
}

/// The Plug-in Manager's choices: plug-ins hidden from the menus (still loaded by projects),
/// and ignored ones (keys, or a file's path: not scanned, not loaded).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Choices {
    pub hidden: BTreeSet<String>,
    pub ignored: BTreeSet<String>,
}

pub fn choices() -> Choices {
    let mut c = Choices::default();
    let Some(t) = manager_path().and_then(|p| std::fs::read_to_string(p).ok()) else { return c };
    for l in t.lines() {
        if let Some(k) = l.strip_prefix("hide\t") {
            c.hidden.insert(k.into());
        } else if let Some(k) = l.strip_prefix("ignore\t") {
            c.ignored.insert(k.into());
        }
    }
    c
}

pub fn save_choices(c: &Choices) {
    let Some(p) = manager_path() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut s = String::new();
    for k in &c.hidden {
        s += &format!("hide\t{k}\n");
    }
    for k in &c.ignored {
        s += &format!("ignore\t{k}\n");
    }
    let _ = std::fs::write(p, s);
}

/// The plug-ins that aren't to be loaded (ignored in the manager).
pub fn disabled() -> BTreeSet<String> {
    choices().ignored
}

/// Looks at one file in this process (the scanner child's work).
pub fn describe(f: Format, path: &Path) -> Result<Vec<PluginInfo>, String> {
    let list = match f {
        Format::Clap => crate::clap::describe(path)?,
        Format::Vst3 => crate::vst3::describe(path)?,
        Format::Lv2 => crate::lv2::describe(path)?,
    };
    if list.is_empty() {
        return Err("no instruments or effects in it".into());
    }
    // each one made and run for a block: a plug-in that can't process isn't listed
    for i in &list {
        let mut inst = crate::open(i, 48000.0)?;
        let mut l = vec![0f32; 256];
        let mut r = vec![0f32; 256];
        lg_dsp::Effect::process(&mut inst, &mut l, &mut r);
        if l.iter().chain(r.iter()).any(|x| !x.is_finite()) {
            return Err(format!("{} gives invalid samples", i.name));
        }
    }
    Ok(list)
}

/// The scanner process's main: `--scan-plugin FORMAT PATH`. Prints `PLUGIN …` lines or
/// `ERROR reason` and exits.
pub fn child_main(args: &[String]) -> ! {
    let res = match (args.first().and_then(|f| Format::parse(f)), args.get(1)) {
        (Some(f), Some(p)) => describe(f, Path::new(p)),
        _ => Err("usage: --scan-plugin CLAP|VST3|LV2 PATH".into()),
    };
    match res {
        Ok(list) => {
            for i in list {
                println!("PLUGIN\t{}", info_line(&i));
            }
            std::process::exit(0)
        }
        Err(e) => {
            println!("ERROR\t{}", clean(&e));
            std::process::exit(2)
        }
    }
}

/// The program to scan with: LG_PLUGSCAN, else Studio's own binary (with `--scan-plugin`).
pub fn scanner() -> Option<PathBuf> {
    std::env::var_os("LG_PLUGSCAN").map(PathBuf::from).or_else(|| std::env::current_exe().ok())
}

/// Scans one file in a child process (`timeout` for it to answer).
pub fn scan_file(exe: &Path, f: Format, path: &Path, timeout: Duration, crashes_before: u32) -> Vec<Entry> {
    let m = mtime(path);
    let fail = |s: Status| vec![Entry { format: f, path: path.to_path_buf(), mtime: m, status: s, info: None }];
    let child = Command::new(exe).arg("--scan-plugin").arg(f.label()).arg(path).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let Ok(mut child) = child else { return fail(Status::Failed("the scanner wouldn't start".into())) };
    let mut out = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break None,
        }
    };
    let text = reader.join().unwrap_or_default();
    let Some(status) = status else { return fail(Status::TimedOut) };
    let plugins: Vec<PluginInfo> = text.lines().filter_map(|l| l.strip_prefix("PLUGIN\t")).filter_map(|l| parse_info(l, path)).collect();
    if status.success() && !plugins.is_empty() {
        return plugins.into_iter().map(|i| Entry { format: f, path: path.to_path_buf(), mtime: m, status: Status::Ok, info: Some(i) }).collect();
    }
    if let Some(e) = text.lines().find_map(|l| l.strip_prefix("ERROR\t")) {
        return fail(Status::Failed(e.into()));
    }
    // killed by a signal or exited without a word: it crashed
    fail(Status::Crashed(crashes_before + 1))
}

/// Progress of a running scan (for the Plug-in Manager).
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub current: String,
    pub finished: bool,
}

/// Scans every plug-in file: unchanged files keep their cached result unless in `force`
/// (Reset & Rescan); ignored ones aren't looked at. Saves the cache and updates the list of
/// loadable plug-ins.
pub fn scan_all(force: &HashSet<PathBuf>, progress: Option<&std::sync::Mutex<Progress>>) -> Vec<Entry> {
    let files = find_files();
    let cache = load_cache();
    let ign = choices().ignored;
    let exe = scanner();
    let mut out: Vec<Entry> = Vec::new();
    if let Some(p) = progress {
        let mut g = p.lock().unwrap_or_else(|e| e.into_inner());
        *g = Progress { done: 0, total: files.len(), current: String::new(), finished: false };
    }
    for (k, (f, path)) in files.iter().enumerate() {
        if let Some(p) = progress {
            let mut g = p.lock().unwrap_or_else(|e| e.into_inner());
            g.done = k;
            g.current = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        }
        let old: Vec<&Entry> = cache.iter().filter(|e| e.path == *path && e.format == *f).collect();
        if ign.contains(&path.display().to_string()) {
            out.push(Entry { format: *f, path: path.clone(), mtime: mtime(path), status: Status::Ignored, info: None });
            continue;
        }
        let unchanged = !old.is_empty() && old.iter().all(|e| e.mtime == mtime(path));
        if unchanged && !force.contains(path) && !matches!(old[0].status, Status::Ignored) {
            out.extend(old.into_iter().cloned());
            continue;
        }
        let crashes = old.iter().find_map(|e| if let Status::Crashed(n) = e.status { Some(n) } else { None }).unwrap_or(0);
        match &exe {
            Some(exe) => out.extend(scan_file(exe, *f, path, Duration::from_secs(30), crashes)),
            None => out.push(Entry { format: *f, path: path.clone(), mtime: mtime(path), status: Status::Failed("no scanner".into()), info: None }),
        }
    }
    save_cache(&out);
    crate::set_catalog(out.iter().filter(|e| e.status.ok()).filter_map(|e| e.info.clone()).filter(|i| !ign.contains(&key(i))).collect());
    if let Some(p) = progress {
        let mut g = p.lock().unwrap_or_else(|e| e.into_inner());
        g.done = files.len();
        g.finished = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_lines_round_trip() {
        let i = PluginInfo { format: Format::Vst3, id: "ABCD".into(), name: "Gain\tX".into(), vendor: "V".into(), version: "1.0".into(), kind: Kind::Instrument, category: "Instrument|Synth".into(), path: "/x.vst3".into(), inputs: 0, outputs: 2, params: 3, latency: 64, has_editor: true };
        let back = parse_info(&info_line(&i), Path::new("/x.vst3")).unwrap();
        assert_eq!(back.name, "Gain X");
        assert_eq!(PluginInfo { name: "Gain X".into(), ..i }, back);
        assert_eq!(parse_status(&status_text(&Status::Crashed(3))), Status::Crashed(3));
        assert_eq!(parse_status(&status_text(&Status::Failed("no".into()))), Status::Failed("no".into()));
    }
}
