//! Bramble Notes' own library format: the files are the library, as on the sync server
//! (docs/FORMAT.md):
//!
//! ```text
//! <library>/
//!   library.json        {"format": "bramble-notes", "version": 1, "folders": [...], "tags": [...]}
//!   notes/<id>.json     one per note ({"v": 1, "note": {...}}, as the sync server keeps them); a note
//!                       deleted for good is a tombstone {"v": 1, "deleted": true, "id", "at"}
//!   files/<sha256>      attachment files by content, never changed
//!   index/              a cache: a NoteStore-format database (NoteStore.sqlite + Accounts/) the
//!                       library is read from, and manifest.json (what of the files it holds).
//!                       Rebuilt from the files when missing; never synced.
//! ```
//!
//! [`Store::open_library`] opens one: every write to the index is mirrored to the changed notes'
//! files (written to a temp file, then renamed), and on open notes changed on either side since are
//! brought in step (a note file changed elsewhere, e.g. by sync, wins). A library in the old layout
//! (NoteStore.sqlite in the folder) is converted once, after a full backup beside it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::model::*;
use crate::store::Store;
use crate::{sync, Error, Result};

pub const VERSION: u64 = 1;
pub const FORMAT: &str = "bramble-notes";

pub fn is_native(dir: &Path) -> bool {
    dir.join("library.json").is_file()
}

pub fn index_dir(root: &Path) -> PathBuf {
    root.join("index")
}

fn note_path(root: &Path, id: &str) -> PathBuf {
    root.join("notes").join(format!("{id}.json"))
}

/// A file's (size, modification time in ns): how a changed file is noticed.
fn stat(p: &Path) -> Option<(u64, i64)> {
    let m = std::fs::metadata(p).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos() as i64;
    Some((m.len(), t))
}

/// Write `bytes` to `path` whole or not at all (a temp file renamed over it).
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = path.with_file_name(format!(".{}.tmp-{}", path.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut f, bytes)?;
        f.sync_data()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Put a file into files/<sha> (a hard link when it can be: the content never changes).
fn store_file(root: &Path, sha: &str, src: &Path) -> Result<()> {
    let dest = root.join("files").join(sha);
    if dest.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())?;
    let tmp = dest.with_file_name(format!(".{sha}.tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    if std::fs::hard_link(src, &tmp).is_err() {
        std::fs::copy(src, &tmp)?;
    }
    std::fs::rename(&tmp, &dest)?;
    Ok(())
}

/// What of the files the index holds (index/manifest.json).
#[derive(Default)]
struct Manifest {
    /// note id -> (its file's size, modification time; the index's change key when written)
    notes: HashMap<String, (u64, i64, String)>,
    library: Option<(u64, i64)>,
    /// attachment file (in the index) -> (size, modification time, content hash)
    hashes: HashMap<String, (u64, i64, String)>,
}

impl Manifest {
    fn load(root: &Path) -> Manifest {
        let Some(j) = std::fs::read(index_dir(root).join("manifest.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
            return Manifest::default();
        };
        let mut m = Manifest::default();
        for (id, v) in j["notes"].as_object().into_iter().flatten() {
            m.notes.insert(id.clone(), (v[0].as_u64().unwrap_or(0), v[1].as_i64().unwrap_or(0), v[2].as_str().unwrap_or_default().into()));
        }
        m.library = j["library"].as_array().map(|v| (v[0].as_u64().unwrap_or(0), v[1].as_i64().unwrap_or(0)));
        for (p, v) in j["hashes"].as_object().into_iter().flatten() {
            m.hashes.insert(p.clone(), (v[0].as_u64().unwrap_or(0), v[1].as_i64().unwrap_or(0), v[2].as_str().unwrap_or_default().into()));
        }
        m
    }

    fn save(&self, root: &Path) -> Result<()> {
        let notes: serde_json::Map<String, Value> = self.notes.iter().map(|(k, v)| (k.clone(), json!([v.0, v.1, v.2]))).collect();
        let hashes: serde_json::Map<String, Value> = self.hashes.iter().map(|(k, v)| (k.clone(), json!([v.0, v.1, v.2]))).collect();
        let j = json!({"notes": notes, "library": self.library.map(|l| json!([l.0, l.1])), "hashes": hashes});
        write_atomic(&index_dir(root).join("manifest.json"), j.to_string().as_bytes())
    }
}

/// The native side of a [`Store`]: where its files are, and what was last written there.
pub(crate) struct Native {
    pub(crate) root: PathBuf,
    manifest: Manifest,
    /// note pk -> id, so a note whose rows are gone is still known
    pub(crate) known: HashMap<i64, String>,
}

impl Native {
    pub(crate) fn load(root: &Path) -> Native {
        Native { root: root.to_path_buf(), manifest: Manifest::load(root), known: HashMap::new() }
    }

    pub(crate) fn save(&self) -> Result<()> {
        self.manifest.save(&self.root)
    }

    /// A file's content hash, remembered by its path, size and time (index files never change).
    fn hash(&mut self, p: &Path) -> Option<String> {
        let st = stat(p)?;
        let key = p.to_string_lossy().into_owned();
        if let Some((s, t, h)) = self.manifest.hashes.get(&key) {
            if (*s, *t) == st {
                return Some(h.clone());
            }
        }
        let h = sync::hash_file(p)?;
        self.manifest.hashes.insert(key, (st.0, st.1, h.clone()));
        Some(h)
    }

    /// Write a note's file (and its attachments' files) from the index.
    pub(crate) fn export(&mut self, store: &Store, id: &str, key: &str) -> Result<()> {
        let n = store.note(id)?;
        if n.locked && n.sealed.is_none() && n.lock.is_none() {
            // a lock whose key isn't here (a Mac's login password): nothing to write
            return Ok(());
        }
        let (j, files) = sync::note_json_with(&n, &mut |p: &Path| self.hash(p));
        for (sha, src) in &files {
            store_file(&self.root, sha, src)?;
        }
        let path = note_path(&self.root, id);
        write_atomic(&path, j.to_string().as_bytes())?;
        let (s, t) = stat(&path).unwrap_or_default();
        self.manifest.notes.insert(id.to_string(), (s, t, key.to_string()));
        Ok(())
    }

    /// A note deleted for good: its file says so (other devices learn it from there).
    pub(crate) fn tombstone(&mut self, id: &str) -> Result<()> {
        let path = note_path(&self.root, id);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        write_atomic(&path, json!({"v": 1, "deleted": true, "id": id, "at": now}).to_string().as_bytes())?;
        let (s, t) = stat(&path).unwrap_or_default();
        self.manifest.notes.insert(id.to_string(), (s, t, "deleted".into()));
        Ok(())
    }

    /// library.json: folders, Smart Folders and tags.
    pub(crate) fn write_library(&mut self, store: &Store) -> Result<()> {
        let path = self.root.join("library.json");
        write_atomic(&path, library_json(store)?.to_string().as_bytes())?;
        self.manifest.library = stat(&path);
        Ok(())
    }
}

fn library_json(store: &Store) -> Result<Value> {
    let lib = store.library_meta()?;
    let mut j = sync::meta_json(&lib);
    j["format"] = json!(FORMAT);
    j["version"] = json!(VERSION);
    j["tags"] = json!(lib.tags.iter().map(|t| t.name.clone()).collect::<Vec<_>>());
    Ok(j)
}

/// The note files found, by id: (path, size, time).
fn note_files(root: &Path) -> HashMap<String, (PathBuf, u64, i64)> {
    let mut out = HashMap::new();
    for e in std::fs::read_dir(root.join("notes")).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".json").filter(|_| !name.starts_with('.')) else { continue };
        if let Some((s, t)) = stat(&e.path()) {
            out.insert(id.to_string(), (e.path(), s, t));
        }
    }
    out
}

/// The files/<sha> of a library, as a note's JSON asks for them (named as they were).
fn from_files<'a>(root: &'a Path, scratch: &'a Path) -> impl Fn(&str, &str) -> Result<Option<PathBuf>> + 'a {
    move |sha: &str, name: &str| {
        if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(None);
        }
        let src = root.join("files").join(sha);
        if !src.is_file() {
            return Ok(None);
        }
        // the importer names attachments after their file: a link under the original name
        let dest = scratch.join(sha).join(name);
        if !dest.exists() {
            std::fs::create_dir_all(dest.parent().unwrap())?;
            if std::fs::hard_link(&src, &dest).is_err() {
                std::fs::copy(&src, &dest)?;
            }
        }
        Ok(Some(dest))
    }
}

/// What bringing the index in step with the files did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refresh {
    /// notes read in from their files (changed elsewhere)
    pub read: usize,
    /// notes written out (changed in the index since)
    pub written: usize,
    pub erased: usize,
    pub folders: bool,
}

/// Bring the index and the files in step: notes whose files changed since (sync, another app) are
/// read in, notes the index changed since are written out, tombstones erase.
pub(crate) fn reconcile(store: &mut Store) -> Result<Refresh> {
    let Some(root) = store.native_root() else { return Ok(Refresh::default()) };
    let mut rep = Refresh::default();
    let mut native = store.take_native().expect("native");
    let result = (|| -> Result<()> {
        // folders first (notes go into them)
        let lib_path = root.join("library.json");
        if stat(&lib_path) != native.manifest.library {
            if let Some(j) = std::fs::read(&lib_path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
                let lib = store.library_meta()?;
                sync::apply_folders(store, &lib, &j, false)?;
                rep.folders = true;
            }
            native.manifest.library = stat(&lib_path);
        }
        let files = note_files(&root);
        let keys = store.change_keys()?;
        let scratch = root.join("index").join(".incoming");
        let _ = std::fs::remove_dir_all(&scratch);
        let mut incoming = Vec::new();
        let mut erase = Vec::new();
        for (id, (path, s, t)) in &files {
            let known = native.manifest.notes.get(id);
            if known.is_some_and(|k| (k.0, k.1) == (*s, *t)) {
                continue;
            }
            let Some(j) = std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
            if j["deleted"].as_bool() == Some(true) {
                if keys.contains_key(id) {
                    erase.push(id.clone());
                }
                native.manifest.notes.insert(id.clone(), (*s, *t, "deleted".into()));
                continue;
            }
            incoming.push(sync::note_of(&j, &from_files(&root, &scratch))?);
            native.manifest.notes.insert(id.clone(), (*s, *t, String::new()));
        }
        for id in &erase {
            store.erase_note(id)?;
            rep.erased += 1;
        }
        if !incoming.is_empty() {
            rep.read = incoming.len();
            store.put_synced_notes(&incoming)?;
        }
        let _ = std::fs::remove_dir_all(&scratch);
        // the index's own changes (and its notes with no file yet) go out
        let keys = store.change_keys()?;
        let read: HashSet<String> = incoming.iter().map(|n| n.id.clone()).collect();
        for (id, key) in &keys {
            if read.contains(id) {
                if let Some(v) = native.manifest.notes.get_mut(id) {
                    v.2 = key.clone();
                }
                continue;
            }
            if native.manifest.notes.get(id).is_some_and(|v| &v.2 == key) && files.contains_key(id) {
                continue;
            }
            native.export(store, id, key)?;
            rep.written += 1;
        }
        // notes erased in the index while their files stayed: tombstones
        for id in native.manifest.notes.iter().filter(|(id, v)| v.2 != "deleted" && !keys.contains_key(*id)).map(|(id, _)| id.clone()).collect::<Vec<_>>() {
            native.tombstone(&id)?;
            rep.written += 1;
        }
        if rep.written > 0 || rep.folders || !is_native(&root) {
            native.write_library(store)?;
        }
        native.save()
    })();
    store.put_native(native);
    result.map(|_| rep)
}

// ------------------------------------------------------------------ the old layout, converted

/// What converting an old library did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Migration {
    pub backup: PathBuf,
    pub notes: usize,
    pub folders: usize,
    pub attachments: usize,
    pub locked: usize,
    pub tags: usize,
    pub seconds: f64,
}

/// Counts of a library, to check a conversion by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    notes: usize,
    folders: usize,
    attachments: usize,
    locked: usize,
    tags: usize,
}

fn counts_of(store: &Store) -> Result<Counts> {
    let lib = store.library_meta()?;
    let (notes, locked) = store.note_counts()?;
    Ok(Counts {
        notes,
        folders: lib.folders.iter().filter(|f| f.kind != FolderKind::Trash && !f.default).count(),
        attachments: store.attachment_count()?,
        locked,
        tags: lib.tags.len(),
    })
}

/// The same counts read from the files alone (every note file read, every file it names present).
fn counts_of_files(root: &Path) -> Result<Counts> {
    let lib: Value = serde_json::from_slice(&std::fs::read(root.join("library.json.new")).or_else(|_| std::fs::read(root.join("library.json")))?)
        .map_err(|e| Error::Format(format!("library.json: {e}")))?;
    let scratch = root.join("index").join(".verify");
    let _ = std::fs::remove_dir_all(&scratch);
    let mut c = Counts { folders: lib["folders"].as_array().map(Vec::len).unwrap_or(0), tags: lib["tags"].as_array().map(Vec::len).unwrap_or(0), ..Default::default() };
    let missing = std::cell::Cell::new(0usize);
    let check = |sha: &str, _: &str| -> Result<Option<PathBuf>> {
        if !root.join("files").join(sha).is_file() {
            missing.set(missing.get() + 1);
        }
        Ok(None)
    };
    for (id, (path, _, _)) in note_files(root) {
        let j: Value = serde_json::from_slice(&std::fs::read(&path)?).map_err(|e| Error::Format(format!("note {id}: {e}")))?;
        if j["deleted"].as_bool() == Some(true) {
            continue;
        }
        let n = sync::note_of(&j, &check)?;
        c.notes += 1;
        if n.locked {
            c.locked += 1;
            c.attachments += n.sealed.as_ref().map(|s| s.attachments.iter().filter(|a| a.parent.is_none()).count()).unwrap_or(0);
        } else {
            c.attachments += n.attachments.len();
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    if missing.get() > 0 {
        return Err(Error::Format(format!("{} attachment files missing from files/", missing.get())));
    }
    Ok(c)
}

fn copy_tree(src: &Path, dst: &Path, link: bool) -> Result<u64> {
    let mut bytes = 0;
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)?.flatten() {
            bytes += copy_tree(&e.path(), &dst.join(e.file_name()), link)?;
        }
    } else if src.is_file() {
        if !(link && std::fs::hard_link(src, dst).is_ok()) {
            bytes += std::fs::copy(src, dst)?;
        }
    }
    Ok(bytes)
}

/// A full backup of an old-layout library beside it: the database copied consistently, the rest
/// copied (attachment files hard-linked: they're never changed in place, only replaced).
fn backup(dir: &Path) -> Result<PathBuf> {
    let to = backup_target(dir)?;
    for e in std::fs::read_dir(dir)?.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with("NoteStore.sqlite") {
            continue;
        }
        copy_tree(&e.path(), &to.join(&n), n == "Accounts")?;
    }
    backup_db(&dir.join("NoteStore.sqlite"), &to.join("NoteStore.sqlite"))?;
    Ok(to)
}

/// A full copy of a library beside it (`<name>-backup-<date>`), before a big change: notes and
/// settings copied, attachment files hard-linked (they're never changed in place), the index's
/// database copied consistently (while it may be open). Never touched afterwards.
pub fn backup_library(dir: &Path) -> Result<PathBuf> {
    if !is_native(dir) {
        return backup(dir);
    }
    let to = backup_target(dir)?;
    copy_native(dir, &to, true)?;
    Ok(to)
}

/// A native library's files into `to` (made), its index's database copied consistently; `link`:
/// attachment files hard-linked where they can be.
fn copy_native(dir: &Path, to: &Path, link: bool) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(dir)?.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if matches!(n.as_str(), "index" | ".lock" | ".sync-cache") {
            continue;
        }
        copy_tree(&e.path(), &to.join(&n), link && n == "files")?;
    }
    let idx = index_dir(dir);
    std::fs::create_dir_all(to.join("index"))?;
    for e in std::fs::read_dir(&idx)?.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with("NoteStore.sqlite") || n.starts_with('.') {
            continue;
        }
        copy_tree(&e.path(), &to.join("index").join(&n), link && n == "Accounts")?;
    }
    backup_db(&idx.join("NoteStore.sqlite"), &to.join("index/NoteStore.sqlite"))
}

/// Where a library moved to `chosen` goes: `chosen` itself when it's empty (or not there yet) or
/// already a library, else a "Bramble Notes" folder inside it.
pub fn library_target(chosen: &Path) -> PathBuf {
    let empty = std::fs::read_dir(chosen).map(|mut d| d.next().is_none()).unwrap_or(true);
    if empty || is_native(chosen) { chosen.to_path_buf() } else { chosen.join("Bramble Notes") }
}

/// Settings > Library location > Change…: the library at `dir` copied into `to` (which must be
/// empty or not there yet), and the copy checked file by file. The original is left as it was.
/// Returns how many files were copied.
pub fn copy_library(dir: &Path, to: &Path) -> Result<usize> {
    let (a, b) = (std::fs::canonicalize(dir)?, to.parent().and_then(|p| std::fs::canonicalize(p).ok()).map(|p| p.join(to.file_name().unwrap_or_default())).unwrap_or_else(|| to.to_path_buf()));
    if b.starts_with(&a) || a.starts_with(&b) {
        return Err(Error::Format("the new folder can't be inside the library, or hold it".into()));
    }
    if std::fs::read_dir(to).is_ok_and(|mut d| d.next().is_some()) {
        return Err(Error::Format(format!("{} isn't empty", to.display())));
    }
    if !is_native(dir) {
        return Err(Error::Format("the library hasn't been converted to its own format yet".into()));
    }
    copy_native(dir, to, false)?;
    // (every file there, with its size: the index's database is compared by its notes instead)
    fn files(root: &Path, at: &Path, out: &mut Vec<(PathBuf, u64)>) {
        for e in std::fs::read_dir(at).into_iter().flatten().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap_or(&p).to_path_buf();
            let n = rel.to_string_lossy().replace('\\', "/");
            if n == ".lock" || n.starts_with(".sync-cache") || n.starts_with("index/NoteStore.sqlite") || n.starts_with("index/.") {
                continue;
            }
            if p.is_dir() {
                files(root, &p, out);
            } else {
                out.push((rel, e.metadata().map(|m| m.len()).unwrap_or(0)));
            }
        }
    }
    let (mut want, mut got) = (Vec::new(), Vec::new());
    files(dir, dir, &mut want);
    files(to, to, &mut got);
    want.sort();
    got.sort();
    if want != got {
        return Err(Error::Format(format!("the copy doesn't match ({} files there, {} here)", got.len(), want.len())));
    }
    let count = |p: &Path| -> Result<i64> { Ok(rusqlite::Connection::open_with_flags(p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?.query_row("SELECT COUNT(*) FROM ZICCLOUDSYNCINGOBJECT", [], |r| r.get(0))?) };
    if count(&index_dir(dir).join("NoteStore.sqlite"))? != count(&index_dir(to).join("NoteStore.sqlite"))? {
        return Err(Error::Format("the copy's index doesn't match".into()));
    }
    Ok(want.len() + 1)
}

/// `<library>-backup-<date and time>`, made.
fn backup_target(dir: &Path) -> Result<PathBuf> {
    let stamp = {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let days = secs.div_euclid(86400);
        let (y, m, d) = civil(days);
        format!("{y:04}-{m:02}-{d:02}-{:02}{:02}{:02}", secs.rem_euclid(86400) / 3600, secs.rem_euclid(3600) / 60, secs.rem_euclid(60))
    };
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "bramble-notes".into());
    let to = dir.with_file_name(format!("{name}-backup-{stamp}"));
    std::fs::create_dir_all(&to)?;
    Ok(to)
}

/// A database copied consistently (the SQLite backup API), and checked.
fn backup_db(from: &Path, to: &Path) -> Result<()> {
    {
        let src = rusqlite::Connection::open_with_flags(from, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let mut dst = rusqlite::Connection::open(to)?;
        let b = rusqlite::backup::Backup::new(&src, &mut dst)?;
        b.run_to_completion(256, std::time::Duration::from_millis(0), None)?;
    }
    let check: String = rusqlite::Connection::open(to)?.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(Error::Format(format!("the backup's database isn't sound ({check})")));
    }
    Ok(())
}

/// Days since 1970 -> (year, month, day).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

const OLD: [&str; 4] = ["NoteStore.sqlite", "NoteStore.sqlite-wal", "NoteStore.sqlite-shm", "Accounts"];

/// What opening the library is doing, for the window to show while it waits (read from any
/// thread: the library opens on a worker).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Opening {
    /// nothing slow under way
    #[default]
    Idle,
    /// another Notes is opening or converting this library: this one waits for it to finish
    Waiting,
    /// the old library's backup
    BackingUp,
    /// converting: `done` of `total` notes written as files
    Converting { done: usize, total: usize },
    /// the converted library counted against the old one
    Checking,
}

impl Opening {
    /// What the window shows meanwhile ("Updating your library… 120 of 1186"); None when idle.
    pub fn message(&self) -> Option<String> {
        match self {
            Opening::Idle => None,
            Opening::Waiting => Some("Waiting for another Notes window to finish opening the library…".into()),
            Opening::BackingUp => Some("Updating your library… backing it up".into()),
            Opening::Converting { done, total } => Some(format!("Updating your library… {done} of {total}")),
            Opening::Checking => Some("Updating your library… checking".into()),
        }
    }
}

static OPENING: std::sync::Mutex<Opening> = std::sync::Mutex::new(Opening::Idle);

/// What opening the library is doing now ([`Opening::message`] for the window).
pub fn opening() -> Opening {
    OPENING.lock().map(|o| *o).unwrap_or_default()
}

fn set_opening(o: Opening) {
    if let Ok(mut g) = OPENING.lock() {
        *g = o;
    }
}

/// Held while a library is opened (and converted), so two processes never do it at once: the
/// second waits ([`Opening::Waiting`]) until the first is done.
pub(crate) struct Lock(#[allow(dead_code)] std::fs::File);

impl Drop for Lock {
    fn drop(&mut self) {
        set_opening(Opening::Idle);
    }
}

pub(crate) fn lock(dir: &Path) -> Result<Lock> {
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".lock"))?;
    match f.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            set_opening(Opening::Waiting);
            f.lock()?;
        }
        Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
    }
    Ok(Lock(f))
}

/// A conversion cut short (the app quit or the machine went down) after its database moved into
/// index/ but before library.json was written: what's left of the old layout follows it, and the
/// library opens as a native one (library.json is written once it's open).
pub(crate) fn resume(dir: &Path) -> Result<()> {
    let idx = index_dir(dir);
    if is_native(dir) || dir.join("NoteStore.sqlite").exists() || !idx.join("NoteStore.sqlite").exists() {
        return Ok(());
    }
    for n in OLD {
        if dir.join(n).exists() && !idx.join(n).exists() {
            std::fs::rename(dir.join(n), idx.join(n))?;
        }
    }
    let _ = std::fs::remove_file(dir.join("library.json.new"));
    Ok(())
}

/// Convert a library in the old layout (NoteStore.sqlite in `dir`) to the native one: a backup
/// beside it first; the notes and attachments written as files; the database and its files moved
/// to index/ (they're the index from then on); the files' counts checked against the database's
/// before library.json is written. On any failure the old layout is put back as it was.
pub fn migrate(dir: &Path) -> Result<Migration> {
    let started = std::time::Instant::now();
    if let Some(pid) = open_elsewhere(&dir.join("NoteStore.sqlite")) {
        return Err(Error::Format(format!("the library is open in another program (process {pid}): quit it, then open Notes again")));
    }
    set_opening(Opening::BackingUp);
    let backup = backup(dir)?;
    let before = counts_of(&Store::open(dir)?)?;
    let result = (|| -> Result<Counts> {
        // the files, written from the database where it is
        {
            let st = Store::open(dir)?;
            let mut native = Native::load(dir);
            let keys = st.change_keys()?;
            for (i, (id, key)) in keys.iter().enumerate() {
                set_opening(Opening::Converting { done: i, total: keys.len() });
                native.export(&st, id, key)?;
            }
            set_opening(Opening::Checking);
            let path = dir.join("library.json.new");
            write_atomic(&path, library_json(&st)?.to_string().as_bytes())?;
            native.manifest.library = None;
            st.flush_wal()?;
            drop(st);
            // the database becomes the index
            let idx = index_dir(dir);
            std::fs::create_dir_all(&idx)?;
            for n in OLD {
                if dir.join(n).exists() {
                    std::fs::rename(dir.join(n), idx.join(n))?;
                }
            }
            // its attachment paths moved: hashes are found again by content
            native.manifest.hashes.clear();
            native.save()?;
        }
        let after = counts_of_files(dir)?;
        let index = counts_of(&Store::open(&index_dir(dir))?)?;
        if after != before || index != before {
            return Err(Error::Format(format!("the converted library doesn't match: before {before:?}, files {after:?}, index {index:?}")));
        }
        Ok(after)
    })();
    match result {
        Ok(c) => {
            std::fs::rename(dir.join("library.json.new"), dir.join("library.json"))?;
            let mut n = Native::load(dir);
            n.manifest.library = stat(&dir.join("library.json"));
            n.save()?;
            Ok(Migration { backup, notes: c.notes, folders: c.folders, attachments: c.attachments, locked: c.locked, tags: c.tags, seconds: started.elapsed().as_secs_f64() })
        }
        Err(e) => {
            // the old layout put back
            let idx = index_dir(dir);
            for n in OLD {
                if idx.join(n).exists() && !dir.join(n).exists() {
                    let _ = std::fs::rename(idx.join(n), dir.join(n));
                }
            }
            let _ = std::fs::remove_dir_all(&idx);
            let _ = std::fs::remove_dir_all(dir.join("notes"));
            let _ = std::fs::remove_dir_all(dir.join("files"));
            let _ = std::fs::remove_file(dir.join("library.json.new"));
            Err(e)
        }
    }
}

/// Another process with `path` open (Linux: /proc/*/fd), so it mustn't be moved now.
fn open_elsewhere(path: &Path) -> Option<u32> {
    let target = std::fs::canonicalize(path).ok()?;
    let me = std::process::id();
    for p in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = p.file_name().to_string_lossy().parse::<u32>() else { continue };
        if pid == me {
            continue;
        }
        for fd in std::fs::read_dir(p.path().join("fd")).into_iter().flatten().flatten() {
            if std::fs::read_link(fd.path()).is_ok_and(|l| l == target) {
                return Some(pid);
            }
        }
    }
    None
}

// ------------------------------------------------------------------ .bnotes

/// What importing a .bnotes did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BnotesImport {
    pub notes: usize,
    /// already in this library (same id): left as they are
    pub skipped: usize,
    pub folders: usize,
}

/// Export the whole library, losslessly, as a .bnotes file: library.json, notes/ and files/ zipped
/// (no index: it's rebuilt). Returns how many notes.
pub fn export_bnotes(store: &Store, out: &Path) -> Result<usize> {
    use std::io::Write;
    let tmp = out.with_extension("bnotes.tmp");
    let file = std::fs::File::create(&tmp)?;
    let mut z = zip::ZipWriter::new(file);
    let json = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(true);
    let zerr = |e: zip::result::ZipError| Error::Format(format!(".bnotes: {e}"));
    z.start_file("library.json", json).map_err(zerr)?;
    z.write_all(library_json(store)?.to_string().as_bytes())?;
    let mut files: HashMap<String, PathBuf> = HashMap::new();
    let mut n = 0;
    let mut ids: Vec<String> = store.change_keys()?.into_keys().collect();
    ids.sort();
    for id in ids {
        let note = store.note(&id)?;
        if note.locked && note.sealed.is_none() && note.lock.is_none() {
            continue;
        }
        let (j, f) = sync::note_json(&note);
        files.extend(f);
        z.start_file(format!("notes/{id}.json"), json).map_err(zerr)?;
        z.write_all(j.to_string().as_bytes())?;
        n += 1;
    }
    let mut shas: Vec<&String> = files.keys().collect();
    shas.sort();
    for sha in shas {
        z.start_file(format!("files/{sha}"), stored).map_err(zerr)?;
        std::io::copy(&mut std::fs::File::open(&files[sha])?, &mut z)?;
    }
    z.finish().map_err(zerr)?;
    std::fs::rename(&tmp, out)?;
    Ok(n)
}

/// Import a .bnotes file: its folders and notes added to this library (notes already here, by id,
/// are left as they are). Locked notes come still locked.
pub fn import_bnotes(store: &mut Store, path: &Path) -> Result<BnotesImport> {
    let zerr = |e: zip::result::ZipError| Error::Format(format!(".bnotes: {e}"));
    let mut z = zip::ZipArchive::new(std::fs::File::open(path)?).map_err(zerr)?;
    // unpacked inside the library (its index, never synced): the system's temp folder may not be
    // writable (Android's), and files/ hard-links from the same disk
    let base = store.native_root().map(|r| index_dir(&r)).unwrap_or_else(|| store.dir().to_path_buf());
    let tmp = base.join(format!(".bnotes-{}", crate::pb::new_uuid()));
    std::fs::create_dir_all(&tmp)?;
    let result = (|| -> Result<BnotesImport> {
        for i in 0..z.len() {
            let mut f = z.by_index(i).map_err(zerr)?;
            // only library.json, notes/<id>.json and files/<sha>: nothing else, nowhere else
            let Some(name) = f.enclosed_name() else { continue };
            let ok = name == Path::new("library.json")
                || (name.parent() == Some(Path::new("notes")) && name.extension().is_some_and(|e| e == "json"))
                || name.parent() == Some(Path::new("files"));
            if !ok || f.is_dir() {
                continue;
            }
            let dest = tmp.join(&name);
            std::fs::create_dir_all(dest.parent().unwrap())?;
            std::io::copy(&mut f, &mut std::fs::File::create(&dest)?)?;
        }
        let mut rep = BnotesImport::default();
        let lib: Value = serde_json::from_slice(&std::fs::read(tmp.join("library.json"))?).map_err(|e| Error::Format(format!(".bnotes library.json: {e}")))?;
        if lib["format"].as_str() != Some(FORMAT) {
            return Err(Error::Format("not a Bramble Notes library".into()));
        }
        let before = store.library_meta()?;
        sync::apply_folders(store, &before, &lib, true)?;
        rep.folders = store.library_meta()?.folders.len().saturating_sub(before.folders.len());
        let have: HashSet<String> = store.change_keys()?.into_keys().collect();
        let scratch = tmp.join(".named");
        let mut incoming = Vec::new();
        for (id, (p, _, _)) in note_files(&tmp) {
            let j: Value = serde_json::from_slice(&std::fs::read(&p)?).map_err(|e| Error::Format(format!("note {id}: {e}")))?;
            if j["deleted"].as_bool() == Some(true) {
                continue;
            }
            if have.contains(&id) {
                rep.skipped += 1;
                continue;
            }
            incoming.push(sync::note_of(&j, &from_files(&tmp, &scratch))?);
        }
        rep.notes = incoming.len();
        store.put_synced_notes(&incoming)?;
        Ok(rep)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}
