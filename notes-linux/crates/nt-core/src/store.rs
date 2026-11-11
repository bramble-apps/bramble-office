//! The app's own library: a NoteStore-format SQLite (same schema, Z_METADATA and Z_PRIMARYKEY as Notes
//! 4.13's, so it could go back to a Mac) in `~/.local/share/bramble-notes/`, with media under
//! `Accounts/LocalAccount/` exactly where Notes keeps them. One "On My Mac"-style account
//! ("LocalAccount") with its Notes and Recently Deleted folders.
//!
//! Writes follow the research's format section 9: bump Z_PRIMARYKEY.Z_MAX of the entity's root,
//! ICNote + ICNoteData (+ ICCloudState like Notes' own rows), title/snippet kept in sync with the
//! body, a valid topotext, ZMINIMUMSUPPORTEDNOTESVERSION = the max its contents need, Z_OPT bumped on
//! every change. Entity ids are looked up by name.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::body::{self, now_unix, APPLE_EPOCH};
use crate::model::*;
use crate::{import, lock, native, paper, paperkit, pb, table, Error};

type Result<T> = std::result::Result<T, Error>;

pub const ACCOUNT_ID: &str = "LocalAccount";
pub const DEFAULT_FOLDER: &str = "DefaultFolder-LocalAccount";
pub const TRASH_FOLDER: &str = "TrashFolder-LocalAccount";
const ACCOUNT_NAME: &str = "On My Mac";

const SCHEMA: &str = include_str!("../data/NoteStore-schema.sql");
const METADATA_PLIST: &[u8] = include_bytes!("../data/Z_METADATA.plist");
const MODEL_CACHE: &[u8] = include_bytes!("../data/Z_MODELCACHE.bin");

/// Notes 4.13's entities (Z_PRIMARYKEY of a fresh store): id, name, super.
const ENTITIES: &[(i64, &str, i64)] = &[
    (1, "ICAssetSignature", 0),
    (2, "ICCloudState", 0),
    (3, "ICCloudSyncingObject", 0),
    (4, "ICAccountData", 3),
    (5, "ICAttachment", 3),
    (6, "ICAttachmentPreviewImage", 3),
    (7, "ICDeviceMigrationState", 3),
    (8, "ICHashtag", 3),
    (9, "ICInlineAttachment", 3),
    (10, "ICLegacyTombstone", 3),
    (11, "ICMedia", 3),
    (12, "ICNote", 3),
    (13, "ICNoteContainer", 3),
    (14, "ICAccount", 13),
    (15, "ICFolder", 13),
    (16, "ICInvitation", 0),
    (17, "ICLocation", 0),
    (18, "ICAttachmentLocation", 17),
    (19, "ICNoteData", 0),
    (20, "ICNoteParticipant", 0),
    (21, "ICSearchIndexState", 0),
    (22, "ICServerChangeToken", 0),
    (16001, "CHANGE", 0),
    (16002, "TRANSACTION", 0),
    (16003, "TRANSACTIONSTRING", 0),
];

/// What [`Store::import_library`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportSummary {
    pub folders: usize,
    pub notes: usize,
    pub attachments: usize,
    /// notes whose id is already in this library (imported before)
    pub skipped_existing: usize,
    /// locked notes: their encrypted bodies can't be read, so they are not imported
    pub skipped_locked: usize,
    /// iCloud notes the Mac hadn't downloaded: no body to import
    pub skipped_not_downloaded: usize,
}

pub struct Store {
    conn: Connection,
    dir: PathBuf,
    ents: HashMap<String, (i64, i64)>,
    account_pk: i64,
    /// files of attachments removed in the open transaction, deleted once it commits
    unused: std::cell::RefCell<Vec<PathBuf>>,
    /// notes read lately: (id, change key, note), most recent last
    cache: std::cell::RefCell<Vec<(String, String, Note)>>,
    /// the reader's column map, loaded once
    schema: std::cell::OnceCell<import::Schema>,
    /// [`Store::search`]'s lowercase note text by note pk: (change key, identifier, text)
    search: std::cell::RefCell<HashMap<i64, (String, String, String)>>,
    /// the library folder ([`Store::open_library`]: the native library's; else `dir`)
    root: PathBuf,
    /// a native library's files, kept in step with every write ([`crate::native`])
    native: std::cell::RefCell<Option<native::Native>>,
    /// rows written since the last mirror: (ZICNOTEDATA, rowid)
    dirty: Option<std::sync::Arc<std::sync::Mutex<Vec<(bool, i64)>>>>,
    /// what converting an old library did, or why it failed (then this is the old library)
    migration: Option<std::result::Result<native::Migration, String>>,
}

/// Debug builds: a store call that takes longer than a frame (16 ms) says so on stderr, so the
/// UI's calls that must go to a worker thread show up.
struct Slow(&'static str, std::time::Instant);

impl Slow {
    fn new(name: &'static str) -> Slow {
        Slow(name, std::time::Instant::now())
    }
}

impl Drop for Slow {
    fn drop(&mut self) {
        let ms = self.1.elapsed().as_secs_f64() * 1e3;
        if cfg!(debug_assertions) && ms > 16.0 && std::env::var_os("NT_QUIET").is_none() {
            eprintln!("[nt-core] Store::{} took {ms:.1} ms", self.0);
        }
    }
}

/// How many opened notes [`Store::note`] keeps.
const NOTE_CACHE: usize = 16;

/// The app's library: the folder picked in Settings (Library location: Change…), else
/// [`standard_dir`].
pub fn default_dir() -> PathBuf {
    config_dir().and_then(|c| location_in(&c)).unwrap_or_else(standard_dir)
}

/// The app's settings folder (`$XDG_CONFIG_HOME/bramble-notes`, else `~/.config/bramble-notes`).
pub fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").filter(|x| !x.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-notes"))
}

/// The library folder named in `config`'s library-location.txt, if one is.
pub fn location_in(config: &Path) -> Option<PathBuf> {
    let t = std::fs::read_to_string(config.join("library-location.txt")).ok()?;
    let t = t.trim();
    (!t.is_empty()).then(|| PathBuf::from(t)).filter(|p| p.is_absolute())
}

/// Names `dir` as the library in `config` (None: back to [`standard_dir`]).
pub fn set_location_in(config: &Path, dir: Option<&Path>) -> Result<()> {
    let f = config.join("library-location.txt");
    match dir {
        Some(d) => {
            std::fs::create_dir_all(config)?;
            native::write_atomic(&f, d.to_string_lossy().as_bytes())?;
        }
        None => {
            if f.exists() {
                std::fs::remove_file(&f)?;
            }
        }
    }
    Ok(())
}

/// `$XDG_DATA_HOME/bramble-notes`, else `~/.local/share/bramble-notes`.
pub fn standard_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_DATA_HOME").filter(|x| !x.is_empty()) {
        return PathBuf::from(x).join("bramble-notes");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/bramble-notes")
}

fn to_core(unix: f64) -> f64 {
    unix - APPLE_EPOCH
}

/// The UTI Notes uses for a file, by extension.
pub fn uti_for(path: &Path) -> &'static str {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" => "public.png",
        "jpg" | "jpeg" => "public.jpeg",
        "heic" => "public.heic",
        "gif" => "com.compuserve.gif",
        "tif" | "tiff" => "public.tiff",
        "webp" => "org.webmproject.webp",
        "bmp" => "com.microsoft.bmp",
        "svg" => "public.svg-image",
        "pdf" => "com.adobe.pdf",
        "txt" | "text" => "public.plain-text",
        "md" | "markdown" => "net.daringfireball.markdown",
        "html" | "htm" => "public.html",
        "rtf" => "public.rtf",
        "csv" => "public.comma-separated-values-text",
        "json" => "public.json",
        "zip" => "public.zip-archive",
        "m4a" => "com.apple.m4a-audio",
        "mp3" => "public.mp3",
        "wav" => "com.microsoft.waveform-audio",
        "mp4" => "public.mpeg-4",
        "mov" => "com.apple.quicktime-movie",
        "docx" => "org.openxmlformats.wordprocessingml.document",
        "xlsx" => "org.openxmlformats.spreadsheetml.sheet",
        "pptx" => "org.openxmlformats.presentationml.presentation",
        "pages" => "com.apple.iwork.pages.sffpages",
        "numbers" => "com.apple.iwork.numbers.sffnumbers",
        "key" => "com.apple.iwork.keynote.sffkey",
        _ => "public.data",
    }
}

/// Pixel size of a PNG / JPEG / GIF (for ICAttachment.sizeWidth/Height), when it can be read cheaply.
fn image_size(b: &[u8]) -> Option<(f64, f64)> {
    if b.len() > 24 && b.starts_with(b"\x89PNG") {
        let w = u32::from_be_bytes(b[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(b[20..24].try_into().ok()?);
        return Some((w as f64, h as f64));
    }
    if b.len() > 10 && b.starts_with(b"GIF") {
        return Some((u16::from_le_bytes([b[6], b[7]]) as f64, u16::from_le_bytes([b[8], b[9]]) as f64));
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let m = b[i + 1];
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]);
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]);
                return Some((w as f64, h as f64));
            }
            i += 2 + len;
        }
    }
    None
}

/// Notes' attachments-browser section: 1 images, 2 drawings, 6 documents and everything else.
fn section_for(kind: AttachmentKind) -> i64 {
    match kind {
        AttachmentKind::Image | AttachmentKind::Scan => 1,
        AttachmentKind::Drawing => 2,
        _ => 6,
    }
}

/// ZMINIMUMSUPPORTEDNOTESVERSION for a note body (format section 6).
fn note_min_version(b: &Body) -> i64 {
    let mut v = 0;
    if b.tokens().any(Token::is_inline) {
        v = v.max(6);
    }
    if b.paragraphs.iter().flat_map(|p| &p.spans).any(|s| s.highlight.is_some()) {
        v = v.max(14);
    }
    v
}

fn inline_min_version(t: &Token) -> i64 {
    match t.kind() {
        TokenKind::Tag => 7,
        TokenKind::NoteLink { paragraph: Some(_), .. } => 13,
        TokenKind::Math => 15,
        _ => 6,
    }
}

fn attachment_min_version(kind: AttachmentKind) -> i64 {
    match kind {
        AttachmentKind::Table => 2,
        AttachmentKind::Drawing => 10,
        _ => 0,
    }
}

impl Store {
    /// Open (creating when needed) the library at [`default_dir`].
    /// The app's library ([`Store::open_library`]: converted to the native format the first time).
    pub fn open_default() -> Result<Store> {
        Store::open_library(&default_dir())
    }

    /// Open (creating when needed) the library in `dir` (NoteStore.sqlite + Accounts/).
    pub fn open(dir: &Path) -> Result<Store> {
        // a native library's folder (a worker opening the app's library): its index, kept in step
        if native::is_native(dir) {
            return Store::open_library(dir);
        }
        std::fs::create_dir_all(dir)?;
        let path = dir.join("NoteStore.sqlite");
        let fresh = !path.exists();
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // what a locked note replaces (its plain text) is overwritten, not left in free pages
        conn.pragma_update(None, "secure_delete", "ON")?;
        // WAL's usual: commits don't wait for the disk (an fsync took 60-400 ms each under load);
        // the database stays consistent, a power cut can lose only the last commits
        conn.pragma_update(None, "synchronous", std::env::var("NT_SYNCHRONOUS").unwrap_or_else(|_| "NORMAL".into()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        if fresh {
            Self::create_schema(&conn)?;
        }
        let mut ents = HashMap::new();
        {
            let mut st = conn.prepare("SELECT Z_ENT, Z_NAME, Z_SUPER FROM Z_PRIMARYKEY")?;
            for r in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))? {
                let (e, n, s) = r?;
                ents.insert(n, (e, s));
            }
        }
        if !ents.contains_key("ICNote") {
            return Err(Error::Format(format!("{} is not a Notes store", path.display())));
        }
        let mut s = Store { conn, dir: dir.to_path_buf(), ents, account_pk: 0, unused: Default::default(), cache: Default::default(), schema: Default::default(), search: Default::default(), root: dir.to_path_buf(), native: Default::default(), dirty: None, migration: None };
        s.ensure_account()?;
        s.unwrap_account_folders()?;
        // (Notes' schema indexes ZIDENTIFIER, Z_ENT and every relationship already)
        s.fill_list_thumbnails()?;
        Ok(s)
    }

    /// The library folder (for a native library, the one holding library.json; its index is inside).
    pub fn dir(&self) -> &Path {
        &self.root
    }

    /// Open the app's library in its own format ([`crate::native`]): notes as files, this database
    /// their index. A library in the old layout is converted first (after a backup beside it); if
    /// that fails, the old one opens as it was and [`Store::migration`] says why.
    pub fn open_library(dir: &Path) -> Result<Store> {
        std::fs::create_dir_all(dir)?;
        // (one process at a time opens or converts it; the lock goes when it's open)
        let _lock = native::lock(dir)?;
        native::resume(dir)?;
        let mut migration = None;
        if !native::is_native(dir) && dir.join("NoteStore.sqlite").exists() {
            match native::migrate(dir) {
                Ok(m) => migration = Some(Ok(m)),
                Err(e) => {
                    let mut s = Store::open(dir)?;
                    s.migration = Some(Err(e.to_string()));
                    return Ok(s);
                }
            }
        }
        let mut s = Store::open(&native::index_dir(dir))?;
        s.root = dir.to_path_buf();
        *s.native.borrow_mut() = Some(native::Native::load(dir));
        native::reconcile(&mut s)?;
        s.fill_known()?;
        // a library.json from before accounts were kept (or older than an unwrapping on open)
        let lib_json = std::fs::read(dir.join("library.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        if lib_json.is_none_or(|j| j.get("accounts").is_none()) {
            if let Some(mut nat) = s.take_native() {
                let r = nat.write_library(&s).and_then(|_| nat.save());
                s.put_native(nat);
                r?;
            }
        }
        // every row written from now on: its note's file follows
        let dirty = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let d = dirty.clone();
        s.conn.update_hook(Some(move |_: rusqlite::hooks::Action, _: &str, table: &str, rowid: i64| {
            if table == "ZICCLOUDSYNCINGOBJECT" || table == "ZICNOTEDATA" {
                if let Ok(mut v) = d.lock() {
                    v.push((table == "ZICNOTEDATA", rowid));
                }
            }
        }));
        s.dirty = Some(dirty);
        s.migration = migration;
        Ok(s)
    }

    /// What opening converted (an old library to the native format), or why converting failed.
    pub fn migration(&self) -> Option<&std::result::Result<native::Migration, String>> {
        self.migration.as_ref()
    }

    /// Whether this is a native library (notes as files).
    pub fn is_native(&self) -> bool {
        self.native.borrow().is_some()
    }

    /// Read in notes whose files changed outside (sync, another device's copy): call after
    /// changing the library's files.
    pub fn refresh(&mut self) -> Result<native::Refresh> {
        if let Some(d) = &self.dirty {
            d.lock().map(|mut v| v.clear()).ok();
        }
        let r = native::reconcile(self)?;
        if let Some(d) = &self.dirty {
            d.lock().map(|mut v| v.clear()).ok();
        }
        self.fill_known()?;
        Ok(r)
    }

    pub(crate) fn native_root(&self) -> Option<PathBuf> {
        self.native.borrow().as_ref().map(|n| n.root.clone())
    }

    pub(crate) fn take_native(&self) -> Option<native::Native> {
        self.native.borrow_mut().take()
    }

    pub(crate) fn put_native(&self, n: native::Native) {
        *self.native.borrow_mut() = Some(n);
    }

    fn fill_known(&self) -> Result<()> {
        let mut map = HashMap::new();
        let mut st = self.conn.prepare("SELECT Z_PK, ZIDENTIFIER FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1")?;
        for r in st.query_map([self.ent("ICNote")], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (pk, id) = r?;
            map.insert(pk, id);
        }
        if let Some(n) = self.native.borrow_mut().as_mut() {
            n.known = map;
        }
        Ok(())
    }

    /// (notes, locked notes) a native library writes out: not marked deleted, and not locked with a
    /// key that isn't here (a Mac's login password).
    pub(crate) fn note_counts(&self) -> Result<(usize, usize)> {
        let (n, l): (i64, i64) = self.conn.query_row(
            "SELECT COUNT(*), IFNULL(SUM(IFNULL(ZISPASSWORDPROTECTED, 0)), 0) FROM ZICCLOUDSYNCINGOBJECT
             WHERE Z_ENT = ?1 AND IFNULL(ZMARKEDFORDELETION, 0) = 0
               AND NOT (IFNULL(ZISPASSWORDPROTECTED, 0) = 1 AND IFNULL(ZCRYPTOITERATIONCOUNT, 0) = 0)",
            [self.ent("ICNote")],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((n as usize, l as usize))
    }

    /// How many attachments the notes have (not scan pages, not ones marked deleted).
    pub(crate) fn attachment_count(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM ZICCLOUDSYNCINGOBJECT a JOIN ZICCLOUDSYNCINGOBJECT n ON a.ZNOTE = n.Z_PK
             WHERE a.Z_ENT = ?1 AND n.Z_ENT = ?2 AND IFNULL(a.ZMARKEDFORDELETION, 0) = 0 AND IFNULL(n.ZMARKEDFORDELETION, 0) = 0
               AND NOT (IFNULL(n.ZISPASSWORDPROTECTED, 0) = 1 AND IFNULL(n.ZCRYPTOITERATIONCOUNT, 0) = 0)",
            params![self.ent("ICAttachment"), self.ent("ICNote")],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Everything in the write-ahead log written into the database file.
    pub(crate) fn flush_wal(&self) -> Result<()> {
        self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }

    /// The rows written since the last call: their notes' files rewritten (or tombstoned), and
    /// library.json when folders or tags changed.
    fn mirror(&self) -> Result<()> {
        let Some(d) = &self.dirty else { return Ok(()) };
        let rows: Vec<(bool, i64)> = std::mem::take(&mut *d.lock().map_err(|_| Error::Format("sync state".into()))?);
        if rows.is_empty() {
            return Ok(());
        }
        let Some(mut nat) = self.take_native() else { return Ok(()) };
        let result = (|| -> Result<()> {
            let (note_ent, att_ent, media_ent, inline_ent) = (self.ent("ICNote"), self.ent("ICAttachment"), self.ent("ICMedia"), self.ent("ICInlineAttachment"));
            let meta_ents = [self.ent("ICFolder"), self.ent("ICHashtag"), self.ent("ICAccount")];
            let mut notes: HashSet<i64> = HashSet::new();
            let mut meta = false;
            let note_of_att = |apk: i64| -> Result<Option<i64>> {
                let r: Option<(Option<i64>, Option<i64>)> =
                    self.conn.query_row("SELECT ZNOTE, ZPARENTATTACHMENT FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1", [apk], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
                Ok(match r {
                    Some((Some(n), _)) => Some(n),
                    Some((None, Some(p))) => self.conn.query_row("SELECT ZNOTE FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1", [p], |r| r.get(0)).optional()?.flatten(),
                    _ => None,
                })
            };
            for (data, rowid) in rows.into_iter().collect::<HashSet<_>>() {
                if data {
                    if let Some(n) = self.conn.query_row("SELECT ZNOTE FROM ZICNOTEDATA WHERE Z_PK = ?1", [rowid], |r| r.get::<_, Option<i64>>(0)).optional()?.flatten() {
                        notes.insert(n);
                    }
                    continue;
                }
                type R = (i64, Option<i64>, Option<i64>, Option<i64>);
                let r: Option<R> = self
                    .conn
                    .query_row("SELECT Z_ENT, ZNOTE, ZNOTE1, ZATTACHMENT1 FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1", [rowid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                    .optional()?;
                match r {
                    None => {
                        if nat.known.contains_key(&rowid) {
                            notes.insert(rowid);
                        }
                    }
                    Some((e, ..)) if e == note_ent => {
                        notes.insert(rowid);
                    }
                    Some((e, ..)) if e == att_ent => notes.extend(note_of_att(rowid)?),
                    Some((e, _, _, Some(a))) if e == media_ent => notes.extend(note_of_att(a)?),
                    Some((e, _, Some(n), _)) if e == inline_ent => {
                        notes.insert(n);
                    }
                    Some((e, ..)) if meta_ents.contains(&e) => meta = true,
                    _ => {}
                }
            }
            for pk in notes {
                let row: Option<(String, Option<i64>)> = self
                    .conn
                    .query_row("SELECT ZIDENTIFIER, ZMARKEDFORDELETION FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1 AND Z_ENT = ?2", params![pk, note_ent], |r| Ok((r.get(0)?, r.get(1)?)))
                    .optional()?;
                match row {
                    Some((id, gone)) if gone.unwrap_or(0) == 0 => {
                        let key = self.change_key(&id)?;
                        nat.export(self, &id, &key)?;
                        nat.known.insert(pk, id);
                    }
                    _ => {
                        if let Some(id) = nat.known.remove(&pk) {
                            nat.tombstone(&id)?;
                        }
                    }
                }
            }
            if meta {
                nat.write_library(self)?;
            }
            nat.save()
        })();
        self.put_native(nat);
        result
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join("NoteStore.sqlite")
    }

    fn create_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(SCHEMA)?;
        let tx = conn.unchecked_transaction()?;
        for &(e, n, s) in ENTITIES {
            tx.execute("INSERT INTO Z_PRIMARYKEY (Z_ENT, Z_NAME, Z_SUPER, Z_MAX) VALUES (?1, ?2, ?3, 0)", params![e, n, s])?;
        }
        tx.execute("INSERT INTO Z_METADATA (Z_VERSION, Z_UUID, Z_PLIST) VALUES (1, ?1, ?2)", params![pb::new_uuid(), METADATA_PLIST])?;
        tx.execute("INSERT INTO Z_MODELCACHE (Z_CONTENT) VALUES (?1)", params![MODEL_CACHE])?;
        tx.commit()?;
        Ok(())
    }

    fn ent(&self, name: &str) -> i64 {
        self.ents.get(name).map(|e| e.0).unwrap_or_else(|| panic!("NoteStore has no {name} entity"))
    }

    /// A new Z_PK for `entity`: Core Data numbers a whole inheritance tree from its root's Z_MAX.
    fn next_pk(&self, entity: &str) -> Result<i64> {
        let mut name = entity.to_string();
        while let Some((_, sup)) = self.ents.get(&name) {
            if *sup == 0 {
                break;
            }
            match self.ents.iter().find(|(_, (e, _))| e == sup) {
                Some((n, _)) => name = n.clone(),
                None => break,
            }
        }
        self.conn.execute("UPDATE Z_PRIMARYKEY SET Z_MAX = Z_MAX + 1 WHERE Z_NAME = ?1", [&name])?;
        Ok(self.conn.query_row("SELECT Z_MAX FROM Z_PRIMARYKEY WHERE Z_NAME = ?1", [&name], |r| r.get(0))?)
    }

    /// An ICCloudState row for a new object (Notes gives On My Mac rows one too).
    fn cloud_state(&self) -> Result<i64> {
        let pk = self.next_pk("ICCloudState")?;
        self.conn.execute(
            "INSERT INTO ZICCLOUDSTATE (Z_PK, Z_ENT, Z_OPT, ZCURRENTLOCALVERSION, ZINCLOUD, ZLATESTVERSIONSYNCEDTOCLOUD, ZLOCALVERSIONDATE)
             VALUES (?1, ?2, 1, 1, 0, 0, ?3)",
            params![pk, self.ent("ICCloudState"), to_core(now_unix())],
        )?;
        Ok(pk)
    }

    /// Insert the columns every ICCloudSyncingObject row has; returns its pk.
    fn insert_object(&self, entity: &str, identifier: &str, min_version: i64) -> Result<i64> {
        let pk = self.next_pk(entity)?;
        let cs = self.cloud_state()?;
        self.conn.execute(
            "INSERT INTO ZICCLOUDSYNCINGOBJECT (Z_PK, Z_ENT, Z_OPT, ZCRYPTOITERATIONCOUNT, ZISPASSWORDPROTECTED, ZISSHAREDIRTY,
               ZMARKEDFORDELETION, ZMINIMUMSUPPORTEDNOTESVERSION, ZNEEDSINITIALFETCHFROMCLOUD, ZCLOUDSTATE, ZIDENTIFIER)
             VALUES (?1, ?2, 1, 0, 0, 0, 0, ?3, 0, ?4, ?5)",
            params![pk, self.ent(entity), min_version, cs, identifier],
        )?;
        Ok(pk)
    }

    fn touch(&self, pk: i64) -> Result<()> {
        self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET Z_OPT = Z_OPT + 1 WHERE Z_PK = ?1", [pk])?;
        Ok(())
    }

    fn pk_of(&self, entity: &str, identifier: &str) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT Z_PK FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZIDENTIFIER = ?2",
                params![self.ent(entity), identifier],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("{entity} {identifier}")))
    }

    fn ensure_account(&mut self) -> Result<()> {
        let acct = self.ent("ICAccount");
        let pk: Option<i64> = self
            .conn
            .query_row("SELECT Z_PK FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZIDENTIFIER = ?2", params![acct, ACCOUNT_ID], |r| r.get(0))
            .optional()?;
        if let Some(pk) = pk {
            self.account_pk = pk;
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        let pk = self.insert_object("ICAccount", ACCOUNT_ID, 0)?;
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZACCOUNTTYPE = 3, ZNAME = ?2, ZACCOUNTNAMEFORACCOUNTLISTSORTING = ?3, ZOWNER = ?1,
               ZDATEHEADERSTYPE = 0, ZSORTORDER = 0, ZDIDCHOOSETOMIGRATE = 1, ZDIDFINISHMIGRATION = 0, ZDIDMIGRATEONMAC = 0,
               ZSERVERSIDEUPDATETASKFAILURECOUNT = 0, ZSTOREDATASEPARATELY = 1, ZNESTEDTITLEFORSORTING = ''
             WHERE Z_PK = ?1",
            params![pk, ACCOUNT_NAME, format!("3_{ACCOUNT_NAME}")],
        )?;
        self.account_pk = pk;
        self.insert_folder_row(DEFAULT_FOLDER, "Notes", None, 0, None, 1)?;
        self.insert_folder_row(TRASH_FOLDER, "Recently Deleted", None, 1, None, 3)?;
        tx.commit()?;
        Ok(())
    }

    fn insert_folder_row(&self, id: &str, name: &str, parent: Option<i64>, ty: i64, smart: Option<&str>, order: i64) -> Result<i64> {
        self.insert_folder_row_in(id, name, parent, ty, smart, order, self.account_pk)
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_folder_row_in(&self, id: &str, name: &str, parent: Option<i64>, ty: i64, smart: Option<&str>, order: i64, account: i64) -> Result<i64> {
        let pk = self.insert_object("ICFolder", id, if smart.is_some() { 7 } else { 0 })?;
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZTITLE2 = ?2, ZPARENT = ?3, ZFOLDERTYPE = ?4, ZSMARTFOLDERQUERYJSON = ?5, ZSORTORDER = ?6,
               ZACCOUNT8 = ?7, ZOWNER = ?7, ZDATEHEADERSTYPE = 0, ZCUSTOMNOTESORTTYPEVALUE = 0,
               ZACCOUNTNAMEFORACCOUNTLISTSORTING = ?8, ZDATEFORLASTTITLEMODIFICATION = ?9
             WHERE Z_PK = ?1",
            params![pk, name, parent, ty, smart, order, account, format!("3_{ACCOUNT_NAME}"), to_core(now_unix())],
        )?;
        Ok(pk)
    }

    /// A folder's account (pk): the one it belongs to, else this library's own.
    fn folder_account(&self, fpk: i64) -> Result<i64> {
        Ok(self.conn.query_row("SELECT ZOWNER FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1", [fpk], |r| r.get::<_, Option<i64>>(0))?.unwrap_or(self.account_pk))
    }

    /// The account `id` (made, local, when missing): its pk. This library's own is [`ACCOUNT_ID`].
    pub(crate) fn ensure_account_named(&self, id: &str, name: &str) -> Result<i64> {
        if id == ACCOUNT_ID {
            return Ok(self.account_pk);
        }
        if let Ok(pk) = self.pk_of("ICAccount", id) {
            return Ok(pk);
        }
        let pk = self.insert_object("ICAccount", id, 0)?;
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZACCOUNTTYPE = 3, ZNAME = ?2, ZACCOUNTNAMEFORACCOUNTLISTSORTING = ?3, ZOWNER = ?1,
               ZDATEHEADERSTYPE = 0, ZSORTORDER = 0, ZDIDCHOOSETOMIGRATE = 1, ZDIDFINISHMIGRATION = 0, ZDIDMIGRATEONMAC = 0,
               ZSERVERSIDEUPDATETASKFAILURECOUNT = 0, ZSTOREDATASEPARATELY = 1, ZNESTEDTITLEFORSORTING = ''
             WHERE Z_PK = ?1",
            params![pk, name, format!("3_{name}")],
        )?;
        Ok(pk)
    }

    /// Move a folder (and everything in it: subfolders, notes) to another account, as a top folder there.
    pub(crate) fn set_folder_account(&self, id: &str, account: i64) -> Result<()> {
        let root = self.pk_of("ICFolder", id)?;
        let mut todo = vec![root];
        while let Some(f) = todo.pop() {
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZOWNER = ?2, ZACCOUNT8 = ?2, Z_OPT = Z_OPT + 1 WHERE Z_PK = ?1", params![f, account])?;
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZACCOUNT7 = ?2, Z_OPT = Z_OPT + 1 WHERE Z_ENT = ?3 AND ZFOLDER = ?1", params![f, account, self.ent("ICNote")])?;
            let mut st = self.conn.prepare("SELECT Z_PK FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZPARENT = ?2")?;
            todo.extend(st.query_map(params![self.ent("ICFolder"), f], |r| r.get::<_, i64>(0))?.collect::<std::result::Result<Vec<_>, _>>()?);
        }
        self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZPARENT = NULL WHERE Z_PK = ?1", [root])?;
        Ok(())
    }

    /// A library imported before accounts were kept: each Mac account was a top folder named after it
    /// ("iCloud", "On My Mac") holding that account's folders. They become accounts again ("On My
    /// Mac" this library's own), every folder and note keeping its place.
    fn unwrap_account_folders(&mut self) -> Result<usize> {
        let (fent, nent) = (self.ent("ICFolder"), self.ent("ICNote"));
        let wrappers: Vec<(i64, String, String)> = {
            let mut st = self.conn.prepare(
                "SELECT f.Z_PK, f.ZIDENTIFIER, f.ZTITLE2 FROM ZICCLOUDSYNCINGOBJECT f
                 WHERE f.Z_ENT = ?1 AND f.ZPARENT IS NULL AND IFNULL(f.ZFOLDERTYPE, 0) = 0 AND f.ZSMARTFOLDERQUERYJSON IS NULL
                   AND IFNULL(f.ZMARKEDFORDELETION, 0) = 0 AND f.ZTITLE2 IN ('iCloud', 'On My Mac')
                   AND f.ZIDENTIFIER NOT IN (?3, ?4)
                   AND NOT EXISTS (SELECT 1 FROM ZICCLOUDSYNCINGOBJECT n WHERE n.Z_ENT = ?2 AND n.ZFOLDER = f.Z_PK)
                   AND EXISTS (SELECT 1 FROM ZICCLOUDSYNCINGOBJECT c WHERE c.Z_ENT = ?1 AND c.ZPARENT = f.Z_PK)",
            )?;
            st.query_map(params![fent, nent, DEFAULT_FOLDER, TRASH_FOLDER], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<std::result::Result<_, _>>()?
        };
        if wrappers.is_empty() {
            return Ok(0);
        }
        let tx = self.conn.unchecked_transaction()?;
        for (wpk, wid, name) in &wrappers {
            let account = if name == "On My Mac" { self.account_pk } else { self.ensure_account_named(&format!("Account-{wid}"), name)? };
            let children: Vec<(i64, String, String)> = {
                let mut st = self.conn.prepare("SELECT Z_PK, ZIDENTIFIER, ZTITLE2 FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZPARENT = ?2")?;
                st.query_map(params![fent, wpk], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<String>>(2)?.unwrap_or_default())))?.collect::<std::result::Result<_, _>>()?
            };
            for (cpk, cid, cname) in children {
                let has_sub: bool = self.conn.query_row("SELECT EXISTS (SELECT 1 FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZPARENT = ?2)", params![fent, cpk], |r| r.get(0))?;
                if account == self.account_pk && cname == "Notes" && !has_sub {
                    // the Mac's own "Notes" folder: this library's Notes folder
                    let dpk = self.pk_of("ICFolder", DEFAULT_FOLDER)?;
                    self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZFOLDER = ?2, Z_OPT = Z_OPT + 1 WHERE Z_ENT = ?3 AND ZFOLDER = ?1", params![cpk, dpk, nent])?;
                    self.delete_object_row(cpk)?;
                    continue;
                }
                self.set_folder_account(&cid, account)?;
            }
            self.delete_object_row(*wpk)?;
        }
        tx.commit()?;
        Ok(wrappers.len())
    }

    // ------------------------------------------------------------------ reading

    /// Everything in the library, as the importer reads a Mac store.
    /// Locked notes come back locked (no body); [`Store::unlock_note`] opens one.
    /// Everything, every note decoded (slow on a big library: the list wants [`Store::library_meta`]).
    pub fn library(&self) -> Result<Library> {
        let _slow = Slow::new("library");
        self.library_with(&import::Options::default())
    }

    fn library_with(&self, opts: &import::Options) -> Result<Library> {
        let schema = match self.schema.get() {
            Some(s) => s,
            None => {
                let _ = self.schema.set(import::Schema::load(&self.conn)?);
                self.schema.get().unwrap()
            }
        };
        import::read_conn_with(&self.conn, self.dir.clone(), opts, schema).map(|(l, _)| l)
    }

    /// What the notes list and sidebar need, fast: accounts, folders, tags, and each note's id,
    /// folder, title, snippet, dates, pinned/locked/deleted, `list_tags` and `list_thumbnail`
    /// ([`Note::light`]: no body or attachments; [`Note::tags`] and [`Note::thumbnail`] still work).
    /// Open a note with [`Store::note`].
    pub fn library_meta(&self) -> Result<Library> {
        let _slow = Slow::new("library_meta");
        self.library_with(&import::Options { meta: true, ..Default::default() })
    }

    /// Every attachment of every note not in Recently Deleted, light (names, kinds, files and
    /// previews; no tables, drawings or documents decoded), with its note's id: the attachments
    /// browser. Scans come with their pages in `parts`.
    pub fn attachments(&self) -> Result<Vec<(String, Attachment)>> {
        let _slow = Slow::new("attachments");
        let lib = self.library_with(&import::Options { meta: true, all_attachments: true, ..Default::default() })?;
        Ok(lib.notes.into_iter().filter(|n| !n.deleted && !n.locked).flat_map(|n| {
            let id = n.id.clone();
            n.attachments.into_iter().map(move |a| (id.clone(), a))
        }).collect())
    }

    /// Recently Deleted's clean-up: notes deleted more than `days` ago are erased for good, files
    /// and all (Notes keeps them 30 days). Returns how many.
    pub fn purge_trash(&mut self, days: f64) -> Result<usize> {
        let r = (|| -> Result<usize> {
            let _slow = Slow::new("purge_trash");
            let trash = self.pk_of("ICFolder", TRASH_FOLDER)?;
            let before = to_core(now_unix() - days * 86400.0);
            let ids: Vec<String> = {
                let mut st = self.conn.prepare(
                    "SELECT ZIDENTIFIER FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZFOLDER = ?2 AND ZFOLDERMODIFICATIONDATE IS NOT NULL AND ZFOLDERMODIFICATIONDATE < ?3",
                )?;
                st.query_map(params![self.ent("ICNote"), trash, before], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?
            };
            for id in &ids {
                self.erase_note(id)?;
            }
            Ok(ids.len())
        })();
        self.mirror()?;
        r
    }

    /// One note, body and attachments decoded (kept in a small cache until it changes).
    pub fn note(&self, id: &str) -> Result<Note> {
        let _slow = Slow::new("note");
        let key = self.change_key(id)?;
        let mut cache = self.cache.borrow_mut();
        if let Some(i) = cache.iter().position(|(c, k, _)| c == id && *k == key) {
            let hit = cache.remove(i);
            let n = hit.2.clone();
            cache.push(hit);
            return Ok(n);
        }
        drop(cache);
        let n = self.note_with(id, &import::Options::default())?;
        let mut cache = self.cache.borrow_mut();
        cache.retain(|(c, _, _)| c != id);
        cache.push((id.to_string(), key, n.clone()));
        if cache.len() > NOTE_CACHE {
            cache.remove(0);
        }
        Ok(n)
    }

    /// The notes whose title or text contains `query` (case-insensitive substring), as identifiers in
    /// library order. Locked notes match by title only. The first call reads every body (~100 ms on
    /// 1200 notes); later ones re-read only notes that changed.
    pub fn search(&self, query: &str) -> Result<Vec<String>> {
        let _slow = Slow::new("search");
        let q = query.to_lowercase();
        let mut index = self.search.borrow_mut();
        let rows: Vec<(i64, String, String, bool, Option<String>)> = {
            let mut st = self.conn.prepare_cached(
                "SELECT Z_PK, ZIDENTIFIER, Z_OPT || ':' || IFNULL(ZMODIFICATIONDATE1, ''), IFNULL(ZISPASSWORDPROTECTED, 0), ZTITLE1
                 FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND IFNULL(ZMARKEDFORDELETION, 0) = 0 ORDER BY Z_PK",
            )?;
            st.query_map([self.ent("ICNote")], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0, r.get(4)?)))?.collect::<std::result::Result<_, _>>()?
        };
        let live: std::collections::HashSet<i64> = rows.iter().map(|r| r.0).collect();
        index.retain(|pk, _| live.contains(pk));
        let mut out = Vec::new();
        for (pk, id, key, locked, title) in rows {
            if index.get(&pk).is_none_or(|(k, _, _)| *k != key) {
                let text = if locked {
                    title.unwrap_or_default()
                } else {
                    let data: Option<Vec<u8>> = self
                        .conn
                        .query_row("SELECT ZDATA FROM ZICNOTEDATA WHERE Z_PK = (SELECT ZNOTEDATA FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1)", [pk], |r| r.get(0))
                        .optional()?
                        .flatten();
                    data.and_then(|d| body::decode_note(&d).ok()).map(|b| b.text()).unwrap_or_else(|| title.unwrap_or_default())
                };
                index.insert(pk, (key, id.clone(), text.to_lowercase()));
            }
            if index[&pk].2.contains(&q) {
                out.push(id);
            }
        }
        Ok(out)
    }

    /// A note's body (see [`Store::note`]).
    pub fn load_body(&self, id: &str) -> Result<Body> {
        self.note(id).map(|n| n.body)
    }

    /// A note's attachments (see [`Store::note`]).
    pub fn load_attachments(&self, id: &str) -> Result<Vec<Attachment>> {
        self.note(id).map(|n| n.attachments)
    }

    fn note_with(&self, id: &str, opts: &import::Options) -> Result<Note> {
        let opts = import::Options { only: Some(id.to_string()), ..opts.clone() };
        self.library_with(&opts)?.notes.into_iter().find(|n| n.id == id).ok_or_else(|| Error::NotFound(format!("note {id}")))
    }

    /// Changes whenever the note or one of its attachments is written (their Z_OPT, its folder, pin
    /// and dates).
    fn change_key(&self, id: &str) -> Result<String> {
        let k: Option<String> = self
            .conn
            .query_row(
                "SELECT n.Z_OPT || ':' || IFNULL(n.ZFOLDER, '') || ':' || IFNULL(n.ZISPINNED, '') || ':' || IFNULL(n.ZMODIFICATIONDATE1, '') || ':' ||
                   (SELECT IFNULL(SUM(a.Z_OPT), 0) || '/' || COUNT(*) FROM ZICCLOUDSYNCINGOBJECT a WHERE a.ZNOTE = n.Z_PK) || ':' ||
                   (SELECT IFNULL(SUM(i.Z_OPT), 0) || '/' || COUNT(*) FROM ZICCLOUDSYNCINGOBJECT i WHERE i.ZNOTE1 = n.Z_PK) || ':' ||
                   (SELECT IFNULL(SUM(p.Z_OPT), 0) || '/' || COUNT(*) FROM ZICCLOUDSYNCINGOBJECT p
                     WHERE p.ZPARENTATTACHMENT IN (SELECT Z_PK FROM ZICCLOUDSYNCINGOBJECT WHERE ZNOTE = n.Z_PK))
                 FROM ZICCLOUDSYNCINGOBJECT n WHERE n.Z_ENT = ?1 AND n.ZIDENTIFIER = ?2",
                params![self.ent("ICNote"), id],
                |r| r.get(0),
            )
            .optional()?;
        let mut k = k.ok_or_else(|| Error::NotFound(format!("note {id}")))?;
        // and its drawings' PaperKit bundles (Notes may change them)
        let mut st = self.conn.prepare_cached(
            "SELECT a.ZIDENTIFIER FROM ZICCLOUDSYNCINGOBJECT a JOIN ZICCLOUDSYNCINGOBJECT n ON a.ZNOTE = n.Z_PK
             WHERE n.Z_ENT = ?1 AND n.ZIDENTIFIER = ?2 AND a.ZTYPEUTI LIKE 'com.apple.paper%'",
        )?;
        let acct = self.dir.join("Accounts").join(ACCOUNT_ID);
        for aid in st.query_map(params![self.ent("ICNote"), id], |r| r.get::<_, String>(0))? {
            let db = paperkit::bundle_dir(&acct, &aid?).join("Database/data.sqlite3");
            if let Ok(t) = std::fs::metadata(&db).and_then(|m| m.modified()) {
                k.push_str(&format!(":{:?}", t));
            }
        }
        Ok(k)
    }

    /// The attachment a note's list row shows: the first picture, scan, drawing or PDF in its body.
    fn list_thumbnail_of(body: &Body) -> String {
        body.tokens()
            .find(|t| !t.is_inline() && matches!(AttachmentKind::from_uti(&t.uti), AttachmentKind::Image | AttachmentKind::Scan | AttachmentKind::Drawing | AttachmentKind::Pdf))
            .map(|t| t.id.clone())
            .unwrap_or_default()
    }

    /// Notes saved before list thumbnails were kept get theirs (one pass, once).
    fn fill_list_thumbnails(&self) -> Result<()> {
        let missing: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZTHUMBNAILATTACHMENTIDENTIFIER IS NULL",
            [self.ent("ICNote")],
            |r| r.get(0),
        )?;
        if missing == 0 {
            return Ok(());
        }
        // the bodies only (their tokens say which attachment comes first)
        let todo: Vec<(i64, Option<Vec<u8>>)> = {
            let mut st = self.conn.prepare(
                "SELECT n.Z_PK, d.ZDATA FROM ZICCLOUDSYNCINGOBJECT n LEFT JOIN ZICNOTEDATA d ON d.Z_PK = n.ZNOTEDATA
                 WHERE n.Z_ENT = ?1 AND n.ZTHUMBNAILATTACHMENTIDENTIFIER IS NULL AND IFNULL(n.ZISPASSWORDPROTECTED, 0) = 0",
            )?;
            st.query_map([self.ent("ICNote")], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?
        };
        let tx = self.conn.unchecked_transaction()?;
        for (pk, data) in todo {
            let thumb = data.and_then(|d| body::decode_note(&d).ok()).map(|b| Self::list_thumbnail_of(&b)).unwrap_or_default();
            tx.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZTHUMBNAILATTACHMENTIDENTIFIER = ?2 WHERE Z_PK = ?1", params![pk, thumb])?;
        }
        // locked ones (their bodies unread) get theirs when next saved
        tx.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZTHUMBNAILATTACHMENTIDENTIFIER = '' WHERE Z_ENT = ?1 AND ZTHUMBNAILATTACHMENTIDENTIFIER IS NULL", [self.ent("ICNote")])?;
        tx.commit()?;
        Ok(())
    }

    // ------------------------------------------------------------------ folders

    /// A new folder (inside `parent` when given); returns its id.
    pub fn create_folder(&mut self, name: &str, parent: Option<&str>) -> Result<String> {
        let r = (|| -> Result<String> {
            let _slow = Slow::new("create_folder");
            self.create_folder_with_id(&pb::new_uuid(), name, parent, None)
        })();
        self.mirror()?;
        r
    }

    /// A new Smart Folder with Notes' query JSON, e.g.
    /// `{"type":{"and":[{"deleted":false},{"and":[{"pinned":true}]}]},"entity":"note"}`.
    pub fn create_smart_folder(&mut self, name: &str, query_json: &str) -> Result<String> {
        let r = (|| -> Result<String> {
            let _slow = Slow::new("create_smart_folder");
            self.create_folder_with_id(&pb::new_uuid(), name, None, Some(query_json))
        })();
        self.mirror()?;
        r
    }

    pub(crate) fn create_folder_with_id(&mut self, id: &str, name: &str, parent: Option<&str>, smart: Option<&str>) -> Result<String> {
        let tx = self.conn.unchecked_transaction()?;
        let ppk = parent.map(|p| self.pk_of("ICFolder", p)).transpose()?;
        self.new_folder(id, name, ppk, smart)?;
        tx.commit()?;
        Ok(id.to_string())
    }

    /// Insert a folder after the existing ones (no transaction of its own).
    fn new_folder(&self, id: &str, name: &str, parent: Option<i64>, smart: Option<&str>) -> Result<i64> {
        self.new_folder_in(id, name, parent, smart, None)
    }

    /// A folder in `account` (pk; None: its parent's, else this library's own).
    fn new_folder_in(&self, id: &str, name: &str, parent: Option<i64>, smart: Option<&str>, account: Option<i64>) -> Result<i64> {
        let order: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(ZSORTORDER), 0) + 1 FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1",
            [self.ent("ICFolder")],
            |r| r.get(0),
        )?;
        let account = match (account, parent) {
            (Some(a), _) => a,
            (None, Some(p)) => self.folder_account(p)?,
            (None, None) => self.account_pk,
        };
        self.insert_folder_row_in(id, name, parent, if smart.is_some() { 2 } else { 0 }, smart, order, account)
    }

    /// A folder made with a given id in an account (sync, .bnotes); the account is made when missing.
    pub(crate) fn create_folder_in_account(&mut self, id: &str, name: &str, parent: Option<&str>, smart: Option<&str>, account: Option<(&str, &str)>) -> Result<String> {
        let tx = self.conn.unchecked_transaction()?;
        let ppk = parent.map(|p| self.pk_of("ICFolder", p)).transpose()?;
        let acct = account.map(|(i, n)| self.ensure_account_named(i, n)).transpose()?;
        self.new_folder_in(id, name, ppk, smart, acct)?;
        tx.commit()?;
        Ok(id.to_string())
    }

    /// A new folder in another account (by its id, as [`Library::accounts`] lists them), at its top.
    pub fn create_folder_in(&mut self, account: &str, name: &str) -> Result<String> {
        let r = (|| -> Result<String> {
            let acct = self.pk_of("ICAccount", account).or_else(|e| if account == ACCOUNT_ID { Ok(self.account_pk) } else { Err(e) })?;
            let id = pb::new_uuid();
            let tx = self.conn.unchecked_transaction()?;
            self.new_folder_in(&id, name, None, None, Some(acct))?;
            tx.commit()?;
            Ok(id)
        })();
        self.mirror()?;
        r
    }

    pub fn rename_folder(&mut self, id: &str, name: &str) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("rename_folder");
            let pk = self.pk_of("ICFolder", id)?;
            self.conn.execute(
                "UPDATE ZICCLOUDSYNCINGOBJECT SET ZTITLE2 = ?2, ZDATEFORLASTTITLEMODIFICATION = ?3 WHERE Z_PK = ?1",
                params![pk, name, to_core(now_unix())],
            )?;
            self.touch(pk)
        })();
        self.mirror()?;
        r
    }

    pub fn set_smart_query(&mut self, id: &str, query_json: &str) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("set_smart_query");
            let pk = self.pk_of("ICFolder", id)?;
            // a folder with a query is a Smart Folder (ZFOLDERTYPE 2), as Notes keeps it
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZSMARTFOLDERQUERYJSON = ?2, ZFOLDERTYPE = 2 WHERE Z_PK = ?1", params![pk, query_json])?;
            self.touch(pk)
        })();
        self.mirror()?;
        r
    }

    /// Add `#tag` to a note (a hashtag at the end of its last line, or on a new line), unless it has
    /// it already. Locked notes are refused.
    pub fn add_tag(&mut self, note: &str, tag: &str) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("add_tag");
            let tag = tag.trim().trim_start_matches('#');
            if tag.is_empty() {
                return Err(Error::Format("an empty tag".into()));
            }
            let mut n = self.note(note)?;
            if n.locked {
                return Err(Error::Format("the note is locked".into()));
            }
            if n.tags().iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                return Ok(());
            }
            let mut token = Span::token("", UTI_HASHTAG);
            token.token.as_mut().unwrap().alt = format!("#{tag}");
            match n.body.paragraphs.last_mut().filter(|p| p.style == ParaStyle::Body && !p.spans.is_empty() && p.spans.iter().all(|s| s.token.is_none() || s.token.as_ref().is_some_and(|t| t.is_inline()))) {
                Some(p) => {
                    p.spans.push(Span::plain(" "));
                    p.spans.push(token);
                }
                None => n.body.paragraphs.push(Paragraph { spans: vec![token], ..Default::default() }),
            }
            self.save_note(&mut n)
        })();
        self.mirror()?;
        r
    }

    /// Notes' Convert to Smart Folder: the folder's notes go to the Notes folder, each tagged with
    /// the folder's name (as a tag: spaces and punctuation left out), and the folder becomes a Smart
    /// Folder of that tag (`query_json`, the shell's query for it). Folders with subfolders or locked
    /// notes are refused, as on the Mac. Returns the tag.
    pub fn convert_to_smart_folder(&mut self, id: &str, query_json: impl Fn(&str) -> String) -> Result<String> {
        let r = (|| -> Result<String> {
            let _slow = Slow::new("convert_to_smart_folder");
            if id == DEFAULT_FOLDER || id == TRASH_FOLDER {
                return Err(Error::Format("the Notes and Recently Deleted folders can't be converted".into()));
            }
            let lib = self.library_meta()?;
            let folder = lib.folder(id).cloned().ok_or_else(|| Error::NotFound(format!("folder {id}")))?;
            if lib.folders.iter().any(|f| f.parent.as_deref() == Some(id)) {
                return Err(Error::Format("Can't Convert Folder: it has subfolders".into()));
            }
            let notes: Vec<&Note> = lib.notes.iter().filter(|n| n.folder == id && !n.deleted).collect();
            if notes.iter().any(|n| n.locked) {
                return Err(Error::Format("Can't Convert Folder: it has locked notes".into()));
            }
            let tag: String = folder.name.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
            if tag.is_empty() {
                return Err(Error::Format("Can't Convert Folder: its name makes no tag".into()));
            }
            let ids: Vec<String> = notes.iter().map(|n| n.id.clone()).collect();
            for nid in &ids {
                self.add_tag(nid, &tag)?;
                self.move_note(nid, DEFAULT_FOLDER)?;
            }
            self.set_smart_query(id, &query_json(&tag))?;
            Ok(tag)
        })();
        self.mirror()?;
        r
    }

    /// Move a folder into `parent` (None = top level). Refuses to move a folder into itself.
    pub fn move_folder(&mut self, id: &str, parent: Option<&str>) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("move_folder");
            let pk = self.pk_of("ICFolder", id)?;
            let ppk = parent.map(|p| self.pk_of("ICFolder", p)).transpose()?;
            if let Some(mut p) = ppk {
                loop {
                    if p == pk {
                        return Err(Error::Format("can't move a folder into itself".into()));
                    }
                    match self.conn.query_row("SELECT ZPARENT FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1", [p], |r| r.get::<_, Option<i64>>(0))? {
                        Some(up) => p = up,
                        None => break,
                    }
                }
            }
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZPARENT = ?2 WHERE Z_PK = ?1", params![pk, ppk])?;
            self.touch(pk)
        })();
        self.mirror()?;
        r
    }

    /// Set the manual order of a folder among its siblings.
    pub fn set_folder_order(&mut self, id: &str, order: i64) -> Result<()> {
        let r = (|| -> Result<()> {
            let pk = self.pk_of("ICFolder", id)?;
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZSORTORDER = ?2 WHERE Z_PK = ?1", params![pk, order])?;
            self.touch(pk)
        })();
        self.mirror()?;
        r
    }

    /// Delete a folder like Notes does: its notes (and its subfolders' notes) go to Recently Deleted,
    /// the folders go away. The Notes and Recently Deleted folders can't be deleted.
    pub fn delete_folder(&mut self, id: &str) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("delete_folder");
            if id == DEFAULT_FOLDER || id == TRASH_FOLDER {
                return Err(Error::Format("the Notes and Recently Deleted folders can't be deleted".into()));
            }
            let tx = self.conn.unchecked_transaction()?;
            let root = self.pk_of("ICFolder", id)?;
            let trash = self.pk_of("ICFolder", TRASH_FOLDER)?;
            let mut todo = vec![root];
            let mut all = Vec::new();
            while let Some(f) = todo.pop() {
                all.push(f);
                let mut st = self.conn.prepare("SELECT Z_PK FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZPARENT = ?2")?;
                todo.extend(st.query_map(params![self.ent("ICFolder"), f], |r| r.get::<_, i64>(0))?.collect::<std::result::Result<Vec<_>, _>>()?);
            }
            let now = to_core(now_unix());
            for f in all {
                self.conn.execute(
                    "UPDATE ZICCLOUDSYNCINGOBJECT SET ZFOLDER = ?2, ZFOLDERMODIFICATIONDATE = ?3, Z_OPT = Z_OPT + 1 WHERE Z_ENT = ?4 AND ZFOLDER = ?1",
                    params![f, trash, now, self.ent("ICNote")],
                )?;
                self.delete_object_row(f)?;
            }
            tx.commit()?;
            Ok(())
        })();
        self.mirror()?;
        r
    }

    fn delete_object_row(&self, pk: i64) -> Result<()> {
        self.conn.execute("DELETE FROM ZICCLOUDSTATE WHERE Z_PK = (SELECT ZCLOUDSTATE FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1)", [pk])?;
        self.conn.execute("DELETE FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1", [pk])?;
        Ok(())
    }

    // ------------------------------------------------------------------ notes

    /// A new note in `folder` (None = the Notes folder). Missing paragraph/checklist/token ids are
    /// filled in `body`. Returns the note as it now reads back.
    pub fn create_note(&mut self, folder: Option<&str>, body: &mut Body) -> Result<Note> {
        let r = (|| -> Result<Note> {
            let _slow = Slow::new("create_note");
            let id = pb::new_uuid();
            let tx = self.conn.unchecked_transaction()?;
            self.create_note_with(&id, folder.unwrap_or(DEFAULT_FOLDER), body, None, None)?;
            tx.commit()?;
            self.note(&id)
        })();
        self.mirror()?;
        r
    }

    /// Insert a note (no transaction of its own).
    fn create_note_with(&self, id: &str, folder: &str, body: &mut Body, created: Option<f64>, modified: Option<f64>) -> Result<i64> {
        let fpk = self.pk_of("ICFolder", folder)?;
        let pk = self.insert_object("ICNote", id, 0)?;
        let dpk = self.next_pk("ICNoteData")?;
        self.conn.execute("INSERT INTO ZICNOTEDATA (Z_PK, Z_ENT, Z_OPT, ZNOTE) VALUES (?1, ?2, 1, ?3)", params![dpk, self.ent("ICNoteData"), pk])?;
        let now = to_core(now_unix());
        let created = created.map(to_core).unwrap_or(now);
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZACCOUNT7 = ?2, ZFOLDER = ?3, ZNOTEDATA = ?4, ZCREATIONDATE3 = ?5, ZFOLDERMODIFICATIONDATE = ?6,
               ZLASTOPENEDDATE = ?6, ZATTACHMENTVIEWTYPE = 0, ZHASSYSTEMTEXTATTACHMENTS = 0, ZISPINNED = 0, ZISSYSTEMPAPER = 0,
               ZPAPERSTYLETYPE = 0
             WHERE Z_PK = ?1",
            params![pk, self.folder_account(fpk)?, fpk, dpk, created, now],
        )?;
        self.write_body(pk, body, modified, false)?;
        Ok(pk)
    }

    /// Replace a note's body (title, snippet, flags, inline tokens and collapse state follow it).
    /// Missing ids are filled in `body`.
    pub fn update_note(&mut self, id: &str, body: &mut Body) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("update_note");
            let tx = self.conn.unchecked_transaction()?;
            let pk = self.pk_of("ICNote", id)?;
            if self.is_locked(pk)? {
                return Err(Error::Format("the note is locked: open it with unlock_note and save it with save_note".into()));
            }
            self.write_body(pk, body, None, false)?;
            self.touch(pk)?;
            tx.commit()?;
            self.remove_unused_files();
            Ok(())
        })();
        self.mirror()?;
        r
    }

    /// Save a note as the editor holds it: its body, plus its attachments. New attachments (an id not
    /// in the store yet) are added, with `file` copied into Media/ (and `fallback` for drawings);
    /// tables whose grid changed are rewritten. Missing ids are filled in `note.body`; attachment
    /// paths in `note` are updated to the copies.
    /// A locked note (opened with [`Store::unlock_note`]) stays locked: it is encrypted again with its key.
    pub fn save_note(&mut self, note: &mut Note) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("save_note");
            let tx = self.conn.unchecked_transaction()?;
            let pk = self.pk_of("ICNote", &note.id)?;
            if self.is_locked(pk)? && note.lock.is_none() {
                return Err(Error::Format("the note is locked: open it with unlock_note first".into()));
            }
            for a in &mut note.attachments {
                if a.id.is_empty() {
                    a.id = pb::new_uuid();
                }
                match self.pk_of("ICAttachment", &a.id) {
                    Err(_) => {
                        let media = a.file.as_ref().map(std::fs::read).transpose()?;
                        let fallback = match (&a.fallback, &media) {
                            (Some(f), None) => Some((std::fs::read(f)?, f.extension().is_some_and(|e| e == "pdf"))),
                            _ => None,
                        };
                        if a.name.is_empty() {
                            a.name = a.file.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        }
                        if a.uti.is_empty() {
                            a.uti = a.file.as_deref().map(uti_for).unwrap_or("public.data").to_string();
                        }
                        self.insert_attachment(pk, None, a, media.as_deref(), fallback.as_ref().map(|(b, p)| (b.as_slice(), *p)))?;
                    }
                    Ok(apk) => {
                        let stored: Option<Vec<u8>> = self.conn.query_row("SELECT ZMERGEABLEPREFERREDVIEWSIZE FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1", [apk], |r| r.get(0))?;
                        if stored.as_deref().map(paperkit::view_size_of).unwrap_or_default() != a.view_size {
                            self.conn.execute(
                                "UPDATE ZICCLOUDSYNCINGOBJECT SET ZMERGEABLEPREFERREDVIEWSIZE = ?2, Z_OPT = Z_OPT + 1 WHERE Z_PK = ?1",
                                params![apk, paperkit::view_size_bytes(a.view_size)],
                            )?;
                        }
                        if let Some(d) = &a.drawing {
                            self.update_drawing(apk, &a.id, d, a.fallback.as_deref())?;
                        }
                        if let Some(t) = &a.table {
                            let old: Option<Vec<u8>> = self.conn.query_row("SELECT ZMERGEABLEDATA1 FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1", [apk], |r| r.get(0))?;
                            if old.and_then(|b| table::decode_table(&b).ok()).as_ref() != Some(t) {
                                self.conn.execute(
                                    "UPDATE ZICCLOUDSYNCINGOBJECT SET ZMERGEABLEDATA1 = ?2, ZSUMMARY = ?3, ZMODIFICATIONDATE = ?4, Z_OPT = Z_OPT + 1 WHERE Z_PK = ?1",
                                    params![apk, table::encode_table(t), t.summary(), to_core(now_unix())],
                                )?;
                            }
                        }
                    }
                }
            }
            self.write_body(pk, &mut note.body, None, true)?;
            if let Some(l) = &note.lock {
                self.seal_note(pk, l)?;
            }
            self.touch(pk)?;
            tx.commit()?;
            self.remove_unused_files();
            if note.lock.is_some() {
                self.forget_old_pages()?;
            }
            // point the note at the stored copies
            let opts = import::Options { locks: note.lock.iter().cloned().collect(), ..Default::default() };
            let saved = self.note_with(&note.id, &opts)?;
            for a in &mut note.attachments {
                if let Some(s) = saved.attachment(&a.id) {
                    a.file = s.file.clone();
                    a.fallback = s.fallback.clone();
                    a.width = s.width;
                    a.height = s.height;
                }
            }
            let (t, sn) = body::title_and_snippet(&note.body);
            note.title = t;
            note.snippet = sn;
            note.modified = saved.modified;
            Ok(())
        })();
        self.mirror()?;
        r
    }

    fn write_body(&self, pk: i64, body: &mut Body, modified: Option<f64>, purge: bool) -> Result<()> {
        body::assign_ids(body);
        for t in body.tokens_mut() {
            if t.id.is_empty() {
                t.id = pb::new_uuid();
            }
        }
        let (title, snippet) = body::title_and_snippet(body);
        let has_check = body.paragraphs.iter().any(|p| p.style == ParaStyle::Checklist);
        let open_check = body.paragraphs.iter().any(|p| p.style == ParaStyle::Checklist && !p.checked);
        let emphasis = body.paragraphs.iter().flat_map(|p| &p.spans).any(|s| s.highlight.is_some());
        let outline = body::encode_outline_state(body);
        let data = body::encode_note(body);
        let modified = modified.map(to_core).unwrap_or_else(|| to_core(now_unix()));
        self.conn.execute(
            "UPDATE ZICNOTEDATA SET ZDATA = ?2, Z_OPT = Z_OPT + 1 WHERE Z_PK = (SELECT ZNOTEDATA FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1)",
            params![pk, data],
        )?;
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZTITLE1 = ?2, ZSNIPPET = ?3, ZWIDGETSNIPPET = ?4, ZMODIFICATIONDATE1 = ?5,
               ZHASCHECKLIST = ?6, ZHASCHECKLISTINPROGRESS = ?7, ZHASEMPHASIS = ?8, ZMINIMUMSUPPORTEDNOTESVERSION = ?9,
               ZOUTLINESTATEDATA = ?10, ZNEEDSTOSAVEUSERSPECIFICRECORD = CASE WHEN ?10 IS NULL THEN ZNEEDSTOSAVEUSERSPECIFICRECORD ELSE 1 END,
               ZTHUMBNAILATTACHMENTIDENTIFIER = ?11
             WHERE Z_PK = ?1",
            params![
                pk,
                title,
                snippet,
                format!("{title}\n{snippet}"),
                modified,
                has_check,
                open_check,
                emphasis,
                note_min_version(body),
                outline,
                Self::list_thumbnail_of(body)
            ],
        )?;
        self.sync_tokens(pk, body, purge)
    }

    /// Keep ICInlineAttachment (and ICHashtag) rows in step with the body's inline tokens. Block
    /// attachments the body no longer shows are marked deleted (unmarked when they come back), or
    /// with `purge` (a save with the note's whole attachment list) deleted with their scan pages,
    /// their files going once the save commits.
    fn sync_tokens(&self, note_pk: i64, body: &Body, purge: bool) -> Result<()> {
        let inline_ent = self.ent("ICInlineAttachment");
        let mut existing: HashMap<String, i64> = HashMap::new();
        {
            let mut st = self.conn.prepare("SELECT ZIDENTIFIER, Z_PK FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZNOTE1 = ?2")?;
            for r in st.query_map(params![inline_ent, note_pk], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (id, pk) = r?;
                existing.insert(id, pk);
            }
        }
        let now = to_core(now_unix());
        let mut seen = HashSet::new();
        for t in body.tokens().filter(|t| t.is_inline()) {
            seen.insert(t.id.clone());
            let content = if t.uti == UTI_HASHTAG && t.content.is_empty() { t.alt.trim_start_matches('#').to_uppercase() } else { t.content.clone() };
            let pk = match existing.get(&t.id) {
                Some(&pk) => pk,
                None => {
                    let pk = self.insert_object("ICInlineAttachment", &t.id, inline_min_version(t))?;
                    self.conn.execute(
                        "UPDATE ZICCLOUDSYNCINGOBJECT SET ZNOTE1 = ?2, ZACCOUNT4 = ?3, ZCREATIONDATE2 = ?4, ZMENTIONNOTIFICATIONATTEMPTCOUNT = 0,
                           ZMENTIONNOTIFICATIONSTATE = 0 WHERE Z_PK = ?1",
                        params![pk, note_pk, self.account_pk, now],
                    )?;
                    pk
                }
            };
            self.conn.execute(
                "UPDATE ZICCLOUDSYNCINGOBJECT SET ZTYPEUTI1 = ?2, ZALTTEXT = ?3, ZTOKENCONTENTIDENTIFIER = ?4, ZMARKEDFORDELETION = 0 WHERE Z_PK = ?1",
                params![pk, t.uti, t.alt, content],
            )?;
            if t.uti == UTI_HASHTAG {
                self.ensure_hashtag(t.alt.trim_start_matches('#'), &content)?;
            }
        }
        for (id, pk) in existing {
            if !seen.contains(&id) {
                self.delete_object_row(pk)?;
            }
        }
        // block attachments
        let shown: HashSet<&str> = body.tokens().filter(|t| !t.is_inline()).map(|t| t.id.as_str()).collect();
        let rows: Vec<(i64, String, Option<i64>)> = self.note_attachment_rows(note_pk)?;
        let gone: HashSet<i64> = rows.iter().filter(|(_, id, _)| !shown.contains(id.as_str())).map(|r| r.0).collect();
        let parent_of: HashMap<i64, Option<i64>> = {
            let mut st = self.conn.prepare("SELECT Z_PK, ZPARENTATTACHMENT FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZPARENTATTACHMENT IS NOT NULL")?;
            st.query_map([self.ent("ICAttachment")], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?
        };
        if !purge {
            for (apk, id, _) in &rows {
                if parent_of.get(apk).copied().flatten().is_some() {
                    continue;
                }
                let want = i64::from(!shown.contains(id.as_str()));
                self.conn.execute(
                    "UPDATE ZICCLOUDSYNCINGOBJECT SET ZMARKEDFORDELETION = ?2, Z_OPT = Z_OPT + 1 WHERE Z_PK = ?1 AND IFNULL(ZMARKEDFORDELETION, 0) != ?2",
                    params![apk, want],
                )?;
            }
            return Ok(());
        }
        for (apk, aid, media) in rows {
            // a scan page goes with its gallery; a top-level attachment when the body dropped it
            let doomed = match parent_of.get(&apk).copied().flatten() {
                Some(parent) => gone.contains(&parent),
                None => gone.contains(&apk),
            };
            if doomed {
                let files = self.purge_attachment(apk, &aid, media)?;
                self.unused.borrow_mut().extend(files);
            }
        }
        Ok(())
    }

    fn ensure_hashtag(&self, display: &str, standard: &str) -> Result<()> {
        let ent = self.ent("ICHashtag");
        let have: Option<i64> = self
            .conn
            .query_row(
                "SELECT Z_PK FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZSTANDARDIZEDCONTENT = ?2",
                params![ent, standard],
                |r| r.get(0),
            )
            .optional()?;
        if have.is_none() {
            let pk = self.insert_object("ICHashtag", &pb::new_uuid(), 0)?;
            self.conn.execute(
                "UPDATE ZICCLOUDSYNCINGOBJECT SET ZACCOUNT3 = ?2, ZCREATIONDATE1 = ?3, ZDISPLAYTEXT = ?4, ZSTANDARDIZEDCONTENT = ?5 WHERE Z_PK = ?1",
                params![pk, self.account_pk, to_core(now_unix()), display, standard],
            )?;
        }
        Ok(())
    }

    pub fn move_note(&mut self, id: &str, folder: &str) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("move_note");
            let pk = self.pk_of("ICNote", id)?;
            let fpk = self.pk_of("ICFolder", folder)?;
            self.conn.execute(
                "UPDATE ZICCLOUDSYNCINGOBJECT SET ZFOLDER = ?2, ZFOLDERMODIFICATIONDATE = ?3, ZACCOUNT7 = ?4 WHERE Z_PK = ?1",
                params![pk, fpk, to_core(now_unix()), self.folder_account(fpk)?],
            )?;
            self.touch(pk)
        })();
        self.mirror()?;
        r
    }

    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("set_pinned");
            let pk = self.pk_of("ICNote", id)?;
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZISPINNED = ?2, ZNEEDSTOSAVEUSERSPECIFICRECORD = 1 WHERE Z_PK = ?1", params![pk, pinned])?;
            self.touch(pk)
        })();
        self.mirror()?;
        r
    }

    /// Delete: the note goes to Recently Deleted.
    pub fn delete_note(&mut self, id: &str) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("delete_note");
            self.move_note(id, TRASH_FOLDER)
        })();
        self.mirror()?;
        r
    }

    /// Recover a note from Recently Deleted into `folder` (None = the Notes folder).
    pub fn recover_note(&mut self, id: &str, folder: Option<&str>) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("recover_note");
            self.move_note(id, folder.unwrap_or(DEFAULT_FOLDER))
        })();
        self.mirror()?;
        r
    }

    /// Delete for good: the note, its body, attachments, inline tokens and media files.
    pub fn erase_note(&mut self, id: &str) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("erase_note");
            let tx = self.conn.unchecked_transaction()?;
            let pk = self.pk_of("ICNote", id)?;
            let files = self.erase_rows(pk)?;
            tx.commit()?;
            for f in files {
                let _ = std::fs::remove_dir_all(f);
            }
            Ok(())
        })();
        self.mirror()?;
        r
    }

    /// A note's rows gone (no transaction of its own); returns its files, to remove after commit.
    fn erase_rows(&self, pk: i64) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let atts: Vec<(i64, String, Option<i64>)> = {
            let mut st = self.conn.prepare(
                "SELECT Z_PK, ZIDENTIFIER, ZMEDIA FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1
                   AND (ZNOTE = ?2 OR ZPARENTATTACHMENT IN (SELECT Z_PK FROM ZICCLOUDSYNCINGOBJECT WHERE ZNOTE = ?2))",
            )?;
            st.query_map(params![self.ent("ICAttachment"), pk], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<std::result::Result<_, _>>()?
        };
        for (apk, aid, media) in atts {
            files.extend(self.purge_attachment(apk, &aid, media)?);
        }
        self.conn.execute("DELETE FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZNOTE1 = ?2", params![self.ent("ICInlineAttachment"), pk])?;
        self.conn.execute("DELETE FROM ZICNOTEDATA WHERE ZNOTE = ?1 OR Z_PK = (SELECT ZNOTEDATA FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1)", [pk])?;
        self.delete_object_row(pk)?;
        Ok(files)
    }

    /// Notes as another device has them (sync): each replaces this library's copy with the same id,
    /// keeping its folder (when it exists here), dates, pin, body and attachments, or its encrypted
    /// rows when locked. All in one transaction: on failure nothing here changes.
    pub fn put_synced_notes(&mut self, notes: &[Note]) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("put_synced_notes");
            let tx = self.conn.unchecked_transaction()?;
            // the old copies' files, set aside (attachment ids are reused) and removed once committed
            let mut aside: Vec<(PathBuf, PathBuf)> = Vec::new();
            let result = (|| -> Result<()> {
                for n in notes {
                    if let Ok(pk) = self.pk_of("ICNote", &n.id) {
                        for f in self.erase_rows(pk)? {
                            if f.exists() {
                                let to = f.with_file_name(format!("{}.synced-{}", f.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
                                let _ = std::fs::remove_dir_all(&to);
                                std::fs::rename(&f, &to)?;
                                aside.push((f, to));
                            }
                        }
                    }
                    let folder = if !n.folder.is_empty() && self.pk_of("ICFolder", &n.folder).is_ok() {
                        n.folder.clone()
                    } else if n.deleted {
                        TRASH_FOLDER.into()
                    } else {
                        DEFAULT_FOLDER.into()
                    };
                    self.put_note_rows(n, &folder)?;
                }
                Ok(())
            })();
            match result.and_then(|_| tx.commit().map_err(Error::from)) {
                Ok(()) => {
                    for (_, old) in aside {
                        let _ = std::fs::remove_dir_all(old);
                    }
                    self.cache.borrow_mut().clear();
                    Ok(())
                }
                Err(e) => {
                    for (path, old) in aside {
                        let _ = std::fs::remove_dir_all(&path);
                        let _ = std::fs::rename(old, path);
                    }
                    Err(e)
                }
            }
        })();
        self.mirror()?;
        r
    }

    /// The change key of every note ([`Store::note`]'s cache key), by identifier: one query each.
    pub fn change_keys(&self) -> Result<HashMap<String, String>> {
        let _slow = Slow::new("change_keys");
        let mut out = HashMap::new();
        let mut st = self.conn.prepare(
            "SELECT n.ZIDENTIFIER, n.Z_OPT || ':' || IFNULL(n.ZFOLDER, '') || ':' || IFNULL(n.ZISPINNED, '') || ':' || IFNULL(n.ZMODIFICATIONDATE1, '') || ':' ||
               IFNULL(a.s, '0/0') || ':' || IFNULL(i.s, '0/0') || ':' || IFNULL(p.s, '0/0')
             FROM ZICCLOUDSYNCINGOBJECT n
             LEFT JOIN (SELECT ZNOTE k, SUM(Z_OPT) || '/' || COUNT(*) s FROM ZICCLOUDSYNCINGOBJECT WHERE ZNOTE IS NOT NULL GROUP BY ZNOTE) a ON a.k = n.Z_PK
             LEFT JOIN (SELECT ZNOTE1 k, SUM(Z_OPT) || '/' || COUNT(*) s FROM ZICCLOUDSYNCINGOBJECT WHERE ZNOTE1 IS NOT NULL GROUP BY ZNOTE1) i ON i.k = n.Z_PK
             LEFT JOIN (SELECT g.ZNOTE k, SUM(c.Z_OPT) || '/' || COUNT(*) s FROM ZICCLOUDSYNCINGOBJECT c JOIN ZICCLOUDSYNCINGOBJECT g ON c.ZPARENTATTACHMENT = g.Z_PK
                        GROUP BY g.ZNOTE) p ON p.k = n.Z_PK
             WHERE n.Z_ENT = ?1 AND IFNULL(n.ZMARKEDFORDELETION, 0) = 0",
        )?;
        for r in st.query_map([self.ent("ICNote")], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (id, k) = r?;
            out.insert(id, k);
        }
        Ok(out)
    }

    /// Delete an attachment's rows (its media and previews too); returns its files to remove once
    /// the transaction commits.
    fn purge_attachment(&self, apk: i64, aid: &str, media: Option<i64>) -> Result<Vec<PathBuf>> {
        let acct = self.dir.join("Accounts").join(ACCOUNT_ID);
        let mut files = Vec::new();
        if let Some(m) = media {
            if let Some(mid) = self
                .conn
                .query_row("SELECT ZIDENTIFIER FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1", [m], |r| r.get::<_, Option<String>>(0))
                .optional()?
                .flatten()
            {
                files.push(acct.join("Media").join(mid));
            }
            self.delete_object_row(m)?;
        }
        files.push(acct.join("FallbackImages").join(aid));
        files.push(acct.join("FallbackPDFs").join(aid));
        files.push(paperkit::bundle_dir(&acct, aid));
        self.conn.execute("DELETE FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZATTACHMENT = ?2", params![self.ent("ICAttachmentPreviewImage"), apk])?;
        self.delete_object_row(apk)?;
        Ok(files)
    }

    /// Remove the files of attachments dropped by the last committed save.
    fn remove_unused_files(&self) {
        for f in self.unused.take() {
            let _ = std::fs::remove_dir_all(f);
        }
    }

    /// Erase every note in Recently Deleted.
    pub fn empty_trash(&mut self) -> Result<usize> {
        let r = (|| -> Result<usize> {
            let _slow = Slow::new("empty_trash");
            let trash = self.pk_of("ICFolder", TRASH_FOLDER)?;
            let ids: Vec<String> = {
                let mut st = self.conn.prepare("SELECT ZIDENTIFIER FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZFOLDER = ?2")?;
                st.query_map(params![self.ent("ICNote"), trash], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?
            };
            for id in &ids {
                self.erase_note(id)?;
            }
            Ok(ids.len())
        })();
        self.mirror()?;
        r
    }

    // ------------------------------------------------------------------ attachments

    /// Attach a copy of `file` to a note (Media/<id>/<generation>/<name>, like Notes). Put
    /// `Span::token(&a.id, &a.uti)` into the body and [`Store::update_note`] to show it.
    pub fn add_attachment(&mut self, note: &str, file: &Path) -> Result<Attachment> {
        let r = (|| -> Result<Attachment> {
            let _slow = Slow::new("add_attachment");
            let bytes = std::fs::read(file)?;
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
            let tx = self.conn.unchecked_transaction()?;
            let npk = self.pk_of("ICNote", note)?;
            let a = Attachment {
                id: pb::new_uuid(),
                uti: uti_for(file).to_string(),
                kind: AttachmentKind::from_uti(uti_for(file)),
                name,
                ..Attachment::default()
            };
            self.insert_attachment(npk, None, &a, Some(&bytes), None)?;
            tx.commit()?;
            self.attachment(note, &a.id)
        })();
        self.mirror()?;
        r
    }

    /// A new table attachment; returns its id (uti com.apple.notes.table).
    pub fn add_table(&mut self, note: &str, t: &Table) -> Result<String> {
        let r = (|| -> Result<String> {
            let _slow = Slow::new("add_table");
            let tx = self.conn.unchecked_transaction()?;
            let npk = self.pk_of("ICNote", note)?;
            let a = Attachment { id: pb::new_uuid(), uti: UTI_TABLE.into(), kind: AttachmentKind::Table, table: Some(t.clone()), ..Default::default() };
            self.insert_attachment(npk, None, &a, None, None)?;
            tx.commit()?;
            Ok(a.id)
        })();
        self.mirror()?;
        r
    }

    /// Replace a table attachment's grid.
    pub fn update_table(&mut self, attachment: &str, t: &Table) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("update_table");
            let pk = self.pk_of("ICAttachment", attachment)?;
            self.conn.execute(
                "UPDATE ZICCLOUDSYNCINGOBJECT SET ZMERGEABLEDATA1 = ?2, ZSUMMARY = ?3, ZMODIFICATIONDATE = ?4 WHERE Z_PK = ?1",
                params![pk, table::encode_table(t), t.summary(), to_core(now_unix())],
            )?;
            self.touch(pk)
        })();
        self.mirror()?;
        r
    }

    /// A drawing edited here: new strokes, and its new fallback picture when `fallback` is a file
    /// outside the library (the editor's render). Nothing is written when neither changed.
    fn update_drawing(&self, apk: i64, id: &str, d: &Drawing, fallback: Option<&Path>) -> Result<()> {
        let (old, generation): (Option<Vec<u8>>, Option<String>) = self.conn.query_row(
            "SELECT ZMERGEABLEDATA1, ZFALLBACKIMAGEGENERATION FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1",
            [apk],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let acct = self.dir.join("Accounts").join(ACCOUNT_ID);
        let new_picture = fallback.filter(|f| !f.starts_with(&acct) && !f.starts_with(lock::unlocked_dir()));
        let sealed = self.is_locked(apk)?;
        // a locked note's strokes are in its encrypted values: compare what the reader decoded
        let same = if sealed { false } else { old.as_deref().and_then(paper::decode_ink).as_ref() == Some(d) };
        let no_bundle = !sealed && paperkit::writable(d) && !paperkit::bundle_dir(&acct, id).is_dir();
        if same && new_picture.is_none() && !no_bundle {
            return Ok(());
        }
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZMERGEABLEDATA1 = ?2, ZSIZEWIDTH = ?3, ZSIZEHEIGHT = ?4, ZMODIFICATIONDATE = ?5, Z_OPT = Z_OPT + 1 WHERE Z_PK = ?1",
            params![apk, paper::encode_ink(d), d.width as f64, d.height as f64, to_core(now_unix())],
        )?;
        if !sealed {
            self.write_bundle(apk, id, d)?;
        }
        if let Some(src) = new_picture {
            let b = std::fs::read(src)?;
            let _ = std::fs::remove_dir_all(acct.join("FallbackImages").join(id));
            let generation = next_generation(generation.as_deref());
            let dir = acct.join("FallbackImages").join(id).join(&generation);
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("FallbackImage.png"), b)?;
            self.conn.execute(
                "UPDATE ZICCLOUDSYNCINGOBJECT SET ZFALLBACKIMAGEGENERATION = ?2, ZFALLBACKIMAGECRYPTOINITIALIZATIONVECTOR = NULL,
                   ZFALLBACKIMAGECRYPTOTAG = NULL WHERE Z_PK = ?1",
                params![apk, generation],
            )?;
        }
        Ok(())
    }

    /// Write a drawing's PaperKit bundle (what Notes reads) and remember its root in our copy of the
    /// strokes; a drawing Notes couldn't show from a bundle gets none.
    fn write_bundle(&self, apk: i64, id: &str, d: &Drawing) -> Result<()> {
        let bundle = paperkit::bundle_dir(&self.dir.join("Accounts").join(ACCOUNT_ID), id);
        if !paperkit::writable(d) {
            paperkit::remove(&bundle);
            return Ok(());
        }
        let root = paperkit::write(&bundle, id, d)?;
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZMERGEABLEDATA1 = ?2, ZMETADATADATA = ?3 WHERE Z_PK = ?1",
            params![apk, paper::encode_ink_for(d, Some(&root)), paperkit::metadata(d).into_bytes()],
        )?;
        Ok(())
    }

    fn attachment(&self, note: &str, id: &str) -> Result<Attachment> {
        self.note(note)?.attachments.into_iter().find(|a| a.id == id).ok_or_else(|| Error::NotFound(format!("attachment {id}")))
    }

    /// Insert ICAttachment (+ ICMedia and the file, or a drawing's fallback image) rows.
    /// `media` = the original file's bytes, `fallback` = (bytes, is_pdf) for drawings/scans.
    fn insert_attachment(&self, note_pk: i64, parent_pk: Option<i64>, a: &Attachment, media: Option<&[u8]>, fallback: Option<(&[u8], bool)>) -> Result<i64> {
        self.insert_attachment_as(note_pk, parent_pk, a, media, fallback, false)
    }

    /// [`Store::insert_attachment`]; `keep_uti` (importing) keeps a PDF in the form it came in.
    fn insert_attachment_as(&self, note_pk: i64, parent_pk: Option<i64>, a: &Attachment, media: Option<&[u8]>, fallback: Option<(&[u8], bool)>, keep_uti: bool) -> Result<i64> {
        let kind = AttachmentKind::from_uti(&a.uti);
        // a new PDF is kept as Notes 4.13 does: a PaperKit document of its pages, no ICMedia
        let paper_doc = a.uti.starts_with("com.apple.paper.doc");
        let doc = kind == AttachmentKind::Pdf && media.is_some() && (paper_doc || !keep_uti);
        // a drawing is stored as a PaperKit one (a legacy com.apple.drawing's PKDrawing data isn't kept)
        let uti = if doc && !paper_doc {
            UTI_PAPER_PDF
        } else if kind == AttachmentKind::Drawing && a.drawing.is_some() {
            UTI_PAPER
        } else {
            a.uti.as_str()
        };
        let pk = self.insert_object("ICAttachment", &a.id, if doc { 13 } else { attachment_min_version(kind) })?;
        let now = to_core(now_unix());
        let (mut w, mut h) = (a.width, a.height);
        if let Some(b) = media {
            if let Some((iw, ih)) = image_size(b).filter(|_| kind == AttachmentKind::Image) {
                (w, h) = (iw, ih);
            }
        }
        let acct = self.dir.join("Accounts").join(ACCOUNT_ID);
        let mut pages = a.pages.clone();
        if let (true, Some(b)) = (doc, media) {
            if pages.is_empty() {
                let tmp = acct.join(format!(".pages-{}.pdf", pb::new_uuid()));
                std::fs::create_dir_all(&acct)?;
                std::fs::write(&tmp, b)?;
                pages = paperkit::pdf_pages(&tmp);
                let _ = std::fs::remove_file(&tmp);
            }
            if let Some(p) = pages.first() {
                (w, h) = (f64::from(p.width), f64::from(p.height));
            }
        }
        let summary = a.table.as_ref().map(Table::summary).unwrap_or_else(|| a.summary.clone());
        let mergeable = a.table.as_ref().map(table::encode_table).or_else(|| a.drawing.as_ref().map(paper::encode_ink));
        let title = if doc {
            Some(Path::new(&a.name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
        } else if media.is_none() && !a.name.is_empty() {
            Some(a.name.clone())
        } else {
            None
        };
        let created = if a.created > 0.0 { to_core(a.created) } else { now };
        let modified = if a.modified > 0.0 { to_core(a.modified) } else { now };
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZTYPEUTI = ?2, ZNOTE = ?3, ZPARENTATTACHMENT = ?4, ZACCOUNT1 = ?5, ZCREATIONDATE = ?6,
               ZMODIFICATIONDATE = ?7, ZSIZEWIDTH = ?8, ZSIZEHEIGHT = ?9, ZSECTION = ?10, ZSUMMARY = ?11, ZMERGEABLEDATA1 = ?12,
               ZURLSTRING = ?13, ZTITLE = ?14, ZFILESIZE = ?15, ZCHECKEDFORLOCATION = 0, ZDIDRUNPAPERFORMDETECTION = 0,
               ZHANDWRITINGSUMMARYVERSION = 0, ZHASMARKUPDATA = 0, ZIMAGECLASSIFICATIONSUMMARYVERSION = 0, ZIMAGEFILTERTYPE = 0,
               ZNEEDSINITIALRELATIONSHIPSETUP = 0, ZOCRSUMMARYVERSION = 0, ZORIENTATION = 0, ZURLEXPIRED = 0, ZDURATION = 0.0,
               ZORIGINX = 0.0, ZORIGINY = 0.0, ZCROPPINGQUADBOTTOMLEFTX = 0.0, ZCROPPINGQUADBOTTOMLEFTY = 0.0,
               ZCROPPINGQUADBOTTOMRIGHTX = 1.0, ZCROPPINGQUADBOTTOMRIGHTY = 0.0, ZCROPPINGQUADTOPLEFTX = 0.0,
               ZCROPPINGQUADTOPLEFTY = 1.0, ZCROPPINGQUADTOPRIGHTX = 1.0, ZCROPPINGQUADTOPRIGHTY = 1.0
             WHERE Z_PK = ?1",
            params![
                pk,
                uti,
                if parent_pk.is_some() { None } else { Some(note_pk) },
                parent_pk,
                self.account_pk,
                created,
                modified,
                w,
                h,
                section_for(kind),
                summary,
                mergeable,
                a.url,
                title,
                media.map(|b| b.len() as i64).unwrap_or(0)
            ],
        )?;
        if let Some(d) = a.drawing.as_ref().filter(|_| kind == AttachmentKind::Drawing) {
            self.write_bundle(pk, &a.id, d)?;
        }
        if a.not_downloaded {
            // kept as it is, so it can still come down on a Mac
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZNEEDSINITIALFETCHFROMCLOUD = 1 WHERE Z_PK = ?1", [pk])?;
        }
        if doc || a.view_size != ViewSize::Large {
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZMERGEABLEPREFERREDVIEWSIZE = ?2 WHERE Z_PK = ?1", params![pk, paperkit::view_size_bytes(a.view_size)])?;
        }
        if let (true, Some(b)) = (doc, media) {
            paperkit::write_document(&paperkit::bundle_dir(&acct, &a.id), b, &pages)?;
            // what other devices and exports use (Notes writes a Quartz copy; the original serves)
            let generation = format!("1_{}", pb::new_uuid());
            let dir = acct.join("FallbackPDFs").join(&a.id).join(&generation);
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("FallbackPDF.pdf"), b)?;
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZFALLBACKPDFGENERATION = ?2 WHERE Z_PK = ?1", params![pk, generation])?;
            return Ok(pk);
        }
        if let Some(b) = media {
            let mid = pb::new_uuid();
            let generation = format!("1_{}", pb::new_uuid());
            let name = if a.name.is_empty() { "file".to_string() } else { sanitize(&a.name) };
            let dir = acct.join("Media").join(&mid).join(&generation);
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join(&name), b)?;
            let mpk = self.insert_object("ICMedia", &mid, 0)?;
            self.conn.execute(
                "UPDATE ZICCLOUDSYNCINGOBJECT SET ZACCOUNT6 = ?2, ZATTACHMENT1 = ?3, ZFILENAME = ?4, ZGENERATION1 = ?5 WHERE Z_PK = ?1",
                params![mpk, self.account_pk, pk, name, generation],
            )?;
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZMEDIA = ?2 WHERE Z_PK = ?1", params![pk, mpk])?;
        }
        if let Some((b, pdf)) = fallback {
            let generation = format!("1_{}", pb::new_uuid());
            let (folder, file, col) = if pdf {
                ("FallbackPDFs", "FallbackPDF.pdf", "ZFALLBACKPDFGENERATION")
            } else {
                ("FallbackImages", "FallbackImage.png", "ZFALLBACKIMAGEGENERATION")
            };
            let dir = acct.join(folder).join(&a.id).join(&generation);
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join(file), b)?;
            self.conn.execute(&format!("UPDATE ZICCLOUDSYNCINGOBJECT SET {col} = ?2 WHERE Z_PK = ?1"), params![pk, generation])?;
        }
        Ok(pk)
    }

    // ------------------------------------------------------------------ importing a Mac library

    /// Copy a library read by [`import::read_library`] into this one: each source account becomes a
    /// top-level folder named after it (its Notes folder and folder tree inside), Smart Folders are
    /// kept, notes in Recently Deleted go to Recently Deleted, attachment files are copied. Notes
    /// already here (same id) are skipped, so importing the same library twice adds nothing.
    pub fn import_library(&mut self, lib: &Library) -> Result<ImportSummary> {
        let r = (|| -> Result<ImportSummary> {
            let _slow = Slow::new("import_library");
            let mut sum = ImportSummary::default();
            let tx = self.conn.unchecked_transaction()?;
            let mut folder_map: HashMap<String, String> = HashMap::new();
            let existing = self.library()?;
            for acct in &lib.accounts {
                // each account stays an account (the Mac's "On My Mac" is this library's own)
                let acct_pk = self.ensure_account_named(&acct.id, &acct.name)?;
                // parents before children
                let mut pending: Vec<&Folder> = lib.folders.iter().filter(|f| f.account == acct.id).collect();
                while !pending.is_empty() {
                    let before = pending.len();
                    pending.retain(|f| {
                        let parent = match &f.parent {
                            None => None,
                            Some(p) => match folder_map.get(p) {
                                Some(q) => Some(q.clone()),
                                None => return true,
                            },
                        };
                        let id = match f.kind {
                            FolderKind::Trash => Ok(TRASH_FOLDER.to_string()),
                            FolderKind::Normal if f.default && acct_pk == self.account_pk => Ok(DEFAULT_FOLDER.to_string()),
                            FolderKind::Smart => self.reuse_or_create(&existing, &f.id, &f.name, None, f.smart.as_deref(), acct_pk, &mut sum),
                            FolderKind::Normal => self.reuse_or_create(&existing, &f.id, &f.name, parent.as_deref(), None, acct_pk, &mut sum),
                        };
                        if let Ok(id) = id {
                            folder_map.insert(f.id.clone(), id);
                        }
                        false
                    });
                    if pending.len() == before {
                        break;
                    }
                }
            }
            for n in &lib.notes {
                if n.locked && n.lock.is_none() && n.sealed.is_none() {
                    sum.skipped_locked += 1;
                    continue;
                }
                if n.not_downloaded {
                    sum.skipped_not_downloaded += 1;
                    continue;
                }
                if existing.note(&n.id).is_some() {
                    sum.skipped_existing += 1;
                    continue;
                }
                let folder = folder_map.get(&n.folder).cloned().unwrap_or_else(|| if n.deleted { TRASH_FOLDER.into() } else { DEFAULT_FOLDER.into() });
                sum.attachments += self.put_note_rows(n, &folder)?;
                sum.notes += 1;
            }
            tx.commit()?;
            if lib.notes.iter().any(|n| n.lock.is_some()) {
                self.forget_old_pages()?;
            }
            Ok(sum)
        })();
        self.mirror()?;
        r
    }

    /// Insert a note read from elsewhere (no transaction of its own); returns the attachments added.
    fn put_note_rows(&self, n: &Note, folder: &str) -> Result<usize> {
        if let (Some(sealed), None) = (&n.sealed, &n.lock) {
            // locked and not opened: copied encrypted, its own password opens it here
            let pk = self.create_note_with(&n.id, folder, &mut Body::default(), Some(n.created), Some(n.modified))?;
            return self.import_sealed(pk, n, sealed);
        }
        let mut added = 0;
        let mut body = n.body.clone();
        let pk = self.create_note_with(&n.id, folder, &mut body, Some(n.created), Some(n.modified))?;
        if n.pinned {
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZISPINNED = 1, ZNEEDSTOSAVEUSERSPECIFICRECORD = 1 WHERE Z_PK = ?1", [pk])?;
        }
        for a in &n.attachments {
            if self.import_attachment(pk, None, a)? {
                added += 1;
            }
        }
        // attachments exist now: refresh the deleted marks
        self.sync_tokens(pk, &body, false)?;
        if let Some(l) = &n.lock {
            // a locked note opened with its password stays locked with it
            self.seal_note(pk, l)?;
        }
        Ok(added)
    }

    fn set_sealed_crypto(&self, pk: i64, c: &Crypto) -> Result<()> {
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZISPASSWORDPROTECTED = 1, ZCRYPTOSALT = ?2, ZCRYPTOITERATIONCOUNT = ?3, ZCRYPTOWRAPPEDKEY = ?4,
               ZCRYPTOVERIFIER = ?5, ZCRYPTOINITIALIZATIONVECTOR = ?6, ZCRYPTOTAG = ?7, ZENCRYPTEDVALUESJSON = ?8 WHERE Z_PK = ?1",
            params![pk, c.salt, c.iterations, c.wrapped_key, c.verifier, c.iv, c.tag, c.values],
        )?;
        Ok(())
    }

    /// A locked note's encrypted rows and files, as Notes stored them (its body, attachments, media,
    /// inline tokens); returns the attachments added.
    fn import_sealed(&self, pk: i64, n: &Note, s: &Sealed) -> Result<usize> {
        self.set_sealed_crypto(pk, &s.crypto)?;
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZTITLE1 = ?2, ZSNIPPET = ?3, ZPASSWORDHINT = ?4, ZISPINNED = ?5 WHERE Z_PK = ?1",
            params![pk, n.title, n.snippet, s.hint, n.pinned],
        )?;
        self.conn.execute(
            "UPDATE ZICNOTEDATA SET ZDATA = ?2, ZCRYPTOINITIALIZATIONVECTOR = ?3, ZCRYPTOTAG = ?4 WHERE Z_PK = (SELECT ZNOTEDATA FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1)",
            params![pk, s.data, s.data_iv, s.data_tag],
        )?;
        let mut pks: HashMap<String, i64> = HashMap::new();
        for a in &s.attachments {
            let apk = self.insert_object("ICAttachment", &a.id, attachment_min_version(AttachmentKind::from_uti(&a.uti)))?;
            let parent = a.parent.as_ref().and_then(|p| pks.get(p).copied());
            self.conn.execute(
                "UPDATE ZICCLOUDSYNCINGOBJECT SET ZTYPEUTI = ?2, ZNOTE = ?3, ZPARENTATTACHMENT = ?4, ZACCOUNT1 = ?5, ZSIZEWIDTH = ?6, ZSIZEHEIGHT = ?7,
                   ZCREATIONDATE = ?8, ZMODIFICATIONDATE = ?9, ZSECTION = ?10, ZFALLBACKIMAGEGENERATION = ?11, ZFALLBACKPDFGENERATION = ?12,
                   ZFALLBACKIMAGECRYPTOINITIALIZATIONVECTOR = ?13, ZFALLBACKIMAGECRYPTOTAG = ?14, ZFALLBACKPDFCRYPTOINITIALIZATIONVECTOR = ?15,
                   ZFALLBACKPDFCRYPTOTAG = ?16 WHERE Z_PK = ?1",
                params![
                    apk,
                    a.uti,
                    if parent.is_some() { None } else { Some(pk) },
                    parent,
                    self.account_pk,
                    a.width,
                    a.height,
                    a.created,
                    a.modified,
                    section_for(AttachmentKind::from_uti(&a.uti)),
                    a.fallback_image,
                    a.fallback_pdf,
                    a.fallback_image_iv,
                    a.fallback_image_tag,
                    a.fallback_pdf_iv,
                    a.fallback_pdf_tag
                ],
            )?;
            self.set_sealed_crypto(apk, &a.crypto)?;
            if let Some(m) = &a.media {
                let mpk = self.insert_object("ICMedia", &m.id, 0)?;
                self.conn.execute(
                    "UPDATE ZICCLOUDSYNCINGOBJECT SET ZACCOUNT6 = ?2, ZATTACHMENT1 = ?3, ZGENERATION1 = ?4, ZASSETCRYPTOINITIALIZATIONVECTOR = ?5,
                       ZASSETCRYPTOTAG = ?6 WHERE Z_PK = ?1",
                    params![mpk, self.account_pk, apk, m.generation, m.asset_iv, m.asset_tag],
                )?;
                self.set_sealed_crypto(mpk, &m.crypto)?;
                self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZMEDIA = ?2 WHERE Z_PK = ?1", params![apk, mpk])?;
            }
            pks.insert(a.id.clone(), apk);
        }
        for t in &s.inline {
            let ipk = self.insert_object("ICInlineAttachment", &t.id, 0)?;
            self.conn.execute(
                "UPDATE ZICCLOUDSYNCINGOBJECT SET ZNOTE1 = ?2, ZACCOUNT4 = ?3, ZTYPEUTI1 = ?4, ZMENTIONNOTIFICATIONATTEMPTCOUNT = 0,
                   ZMENTIONNOTIFICATIONSTATE = 0 WHERE Z_PK = ?1",
                params![ipk, pk, self.account_pk, t.uti],
            )?;
            self.set_sealed_crypto(ipk, &t.crypto)?;
        }
        let acct = self.dir.join("Accounts").join(ACCOUNT_ID);
        for (src, rel) in &s.files {
            copy_tree(src, &acct.join(rel))?;
        }
        Ok(s.attachments.iter().filter(|a| a.parent.is_none()).count())
    }

    #[allow(clippy::too_many_arguments)]
    fn reuse_or_create(&self, existing: &Library, id: &str, name: &str, parent: Option<&str>, smart: Option<&str>, account: i64, sum: &mut ImportSummary) -> Result<String> {
        if existing.folder(id).is_some() {
            return Ok(id.to_string());
        }
        // a source default folder ("DefaultFolder-…") would collide with ours: give it a new id
        let new_id = if id.starts_with("DefaultFolder-") || id.starts_with("TrashFolder-") || self.pk_of("ICFolder", id).is_ok() {
            pb::new_uuid()
        } else {
            id.to_string()
        };
        let ppk = parent.map(|p| self.pk_of("ICFolder", p)).transpose()?;
        self.new_folder_in(&new_id, name, ppk, smart, Some(account))?;
        sum.folders += 1;
        Ok(new_id)
    }

    /// Copy one source attachment (and its scan pages); false when there was nothing to copy.
    fn import_attachment(&self, note_pk: i64, parent: Option<i64>, a: &Attachment) -> Result<bool> {
        if self.pk_of("ICAttachment", &a.id).is_ok() {
            return Ok(false);
        }
        let media = a.file.as_ref().and_then(|p| std::fs::read(p).ok());
        let fallback = a.fallback.as_ref().and_then(|p| std::fs::read(p).ok().map(|b| (b, p.extension().is_some_and(|e| e == "pdf"))));
        let mut a2 = a.clone();
        if media.is_none() && a.kind == AttachmentKind::Image {
            // no original on disk: keep the preview as the image
            if let Some(p) = &a.preview {
                let b = std::fs::read(p).ok();
                let pk = self.insert_attachment_as(note_pk, parent, &a2, b.as_deref(), None, true)?;
                return Ok(pk > 0);
            }
        }
        if media.is_some() && a2.name.is_empty() {
            a2.name = a.file.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        }
        let pk = self.insert_attachment_as(note_pk, parent, &a2, media.as_deref(), fallback.as_ref().map(|(b, pdf)| (b.as_slice(), *pdf)), true)?;
        for p in &a.parts {
            self.import_attachment(note_pk, Some(pk), p)?;
        }
        Ok(true)
    }

    // ------------------------------------------------------------------ locked notes

    fn is_locked(&self, pk: i64) -> Result<bool> {
        let v: Option<i64> = self.conn.query_row("SELECT ZISPASSWORDPROTECTED FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1", [pk], |r| r.get(0))?;
        Ok(v.unwrap_or(0) != 0)
    }

    /// The account's locked-notes password: (salt, iterations, verifier).
    fn account_crypto(&self) -> Result<Option<(Vec<u8>, u32, Vec<u8>)>> {
        let (salt, it, v): (Option<Vec<u8>>, Option<i64>, Option<Vec<u8>>) = self.conn.query_row(
            "SELECT ZCRYPTOSALT, ZCRYPTOITERATIONCOUNT, ZCRYPTOVERIFIER FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1",
            [self.account_pk],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        Ok(match (salt, v) {
            (Some(s), Some(v)) if !v.is_empty() => Some((s, it.unwrap_or(0) as u32, v)),
            _ => None,
        })
    }

    /// A password for locked notes is set (Settings > Locked notes).
    pub fn has_password(&self) -> Result<bool> {
        Ok(self.account_crypto()?.is_some())
    }

    pub fn password_hint(&self) -> Result<Option<String>> {
        let h: Option<String> = self.conn.query_row("SELECT ZPASSWORDHINT FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1", [self.account_pk], |r| r.get(0))?;
        Ok(h.filter(|h| !h.is_empty()))
    }

    pub fn check_password(&self, password: &str) -> Result<bool> {
        Ok(self.account_crypto()?.is_some_and(|(s, i, v)| lock::unwrap(&lock::derive(password, &s, i), &v).is_some()))
    }

    /// Set the password locked notes use (once; then [`Store::change_password`]).
    pub fn set_password(&mut self, password: &str, hint: Option<&str>) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("set_password");
            if self.has_password()? {
                return Err(Error::Format("a password is already set".into()));
            }
            self.write_account_password(password, hint)
        })();
        self.mirror()?;
        r
    }

    /// Settings > Locked notes > Reset Password…: a new password for notes locked from now on, the
    /// old one not asked for. Notes already locked keep theirs (their own keys are untouched): they
    /// open with the password they were locked with.
    pub fn reset_password(&mut self, password: &str, hint: Option<&str>) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("reset_password");
            self.write_account_password(password, hint)
        })();
        self.mirror()?;
        r
    }

    fn write_account_password(&self, password: &str, hint: Option<&str>) -> Result<()> {
        let l = Lock::new(password, hint);
        let (_, verifier) = l.new_key();
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZCRYPTOSALT = ?2, ZCRYPTOITERATIONCOUNT = ?3, ZCRYPTOVERIFIER = ?4, ZPASSWORDHINT = ?5 WHERE Z_PK = ?1",
            params![self.account_pk, l.salt, l.iterations, verifier, hint],
        )?;
        self.touch(self.account_pk)
    }

    /// Change the password: every note locked with the old one is locked with the new one (only the
    /// wrapped keys change). Notes locked with another password (imported) keep theirs.
    pub fn change_password(&mut self, old: &str, new: &str, hint: Option<&str>) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("change_password");
            if !self.check_password(old)? {
                return Err(Error::Format("wrong password".into()));
            }
            let tx = self.conn.unchecked_transaction()?;
            let rows: Vec<(i64, Vec<u8>, i64, Vec<u8>)> = {
                let mut st = self.conn.prepare(
                    "SELECT Z_PK, ZCRYPTOSALT, ZCRYPTOITERATIONCOUNT, ZCRYPTOWRAPPEDKEY FROM ZICCLOUDSYNCINGOBJECT
                     WHERE ZISPASSWORDPROTECTED = 1 AND ZCRYPTOSALT IS NOT NULL AND ZCRYPTOWRAPPEDKEY IS NOT NULL",
                )?;
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<std::result::Result<_, _>>()?
            };
            let mut locks: HashMap<(Vec<u8>, i64), (Lock, Lock)> = HashMap::new();
            for (pk, salt, it, wrapped) in rows {
                let (from, to) = locks
                    .entry((salt.clone(), it))
                    .or_insert_with(|| (Lock::with_salt(old, &salt, it as u32, None), Lock::new(new, hint)))
                    .clone();
                if let Some(w) = from.rewrap(&to, &wrapped) {
                    self.conn.execute(
                        "UPDATE ZICCLOUDSYNCINGOBJECT SET ZCRYPTOSALT = ?2, ZCRYPTOITERATIONCOUNT = ?3, ZCRYPTOWRAPPEDKEY = ?4,
                           ZPASSWORDHINT = CASE WHEN Z_ENT = ?5 THEN ?6 ELSE ZPASSWORDHINT END, Z_OPT = Z_OPT + 1 WHERE Z_PK = ?1",
                        params![pk, to.salt, to.iterations, w, self.ent("ICNote"), hint],
                    )?;
                }
            }
            self.write_account_password(new, hint)?;
            tx.commit()?;
            Ok(())
        })();
        self.mirror()?;
        r
    }

    /// Lock a note with the account's password (set one first with [`Store::set_password`]).
    pub fn lock_note(&mut self, id: &str, password: &str) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("lock_note");
            if !self.has_password()? {
                return Err(Error::NotFound("a password for locked notes (set one first)".into()));
            }
            if !self.check_password(password)? {
                return Err(Error::Format("wrong password".into()));
            }
            let tx = self.conn.unchecked_transaction()?;
            let pk = self.pk_of("ICNote", id)?;
            if !self.is_locked(pk)? {
                let l = Lock::new(password, self.password_hint()?.as_deref());
                self.seal_note(pk, &l)?;
                self.touch(pk)?;
            }
            tx.commit()?;
            self.forget_old_pages()
        })();
        self.mirror()?;
        r
    }

    /// Drop the WAL's old page versions (which still hold a just-locked note's plain text).
    fn forget_old_pages(&self) -> Result<()> {
        self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }

    /// Open a locked note with its password (the store is not changed). Its attachment files are
    /// decrypted into [`lock::unlocked_dir`]; save edits with [`Store::save_note`].
    pub fn unlock_note(&self, id: &str, password: &str) -> Result<Note> {
        let _slow = Slow::new("unlock_note");
        let n = self.note_with(id, &import::Options::with_passwords(&[password]))?;
        if n.locked && n.lock.is_none() {
            return Err(Error::Format("wrong password".into()));
        }
        Ok(n)
    }

    /// Remove a note's lock (Notes' "Remove Lock"): it is stored unencrypted again.
    pub fn remove_lock(&mut self, id: &str, password: &str) -> Result<()> {
        let r = (|| -> Result<()> {
            let _slow = Slow::new("remove_lock");
            let n = self.unlock_note(id, password)?;
            let Some(l) = n.lock else { return Ok(()) };
            let tx = self.conn.unchecked_transaction()?;
            let pk = self.pk_of("ICNote", id)?;
            self.unseal_note(pk, &l)?;
            let mut body = n.body;
            self.write_body(pk, &mut body, Some(n.modified), false)?;
            self.touch(pk)?;
            tx.commit()?;
            self.remove_unused_files();
            Ok(())
        })();
        self.mirror()?;
        r
    }

    /// A row's own key: its current one when it is already encrypted, else a new one.
    fn row_key(&self, pk: i64, l: &Lock) -> Result<([u8; 16], Vec<u8>)> {
        let (on, salt, it, wrapped): (Option<i64>, Option<Vec<u8>>, Option<i64>, Option<Vec<u8>>) = self.conn.query_row(
            "SELECT ZISPASSWORDPROTECTED, ZCRYPTOSALT, ZCRYPTOITERATIONCOUNT, ZCRYPTOWRAPPEDKEY FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1",
            [pk],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        if on.unwrap_or(0) != 0 {
            if let (Some(s), Some(w)) = (salt, wrapped) {
                return match l.key(&s, it.unwrap_or(0) as u32, &w) {
                    Some(k) => Ok((k, w)),
                    None => Err(Error::Format("an object of the note is locked with another password".into())),
                };
            }
        }
        Ok(l.new_key())
    }

    fn set_crypto(&self, pk: i64, l: &Lock, wrapped: &[u8]) -> Result<()> {
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZISPASSWORDPROTECTED = 1, ZCRYPTOSALT = ?2, ZCRYPTOITERATIONCOUNT = ?3, ZCRYPTOWRAPPEDKEY = ?4,
               Z_OPT = Z_OPT + 1 WHERE Z_PK = ?1",
            params![pk, l.salt, l.iterations, wrapped],
        )?;
        Ok(())
    }

    fn clear_crypto(&self, pk: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZISPASSWORDPROTECTED = 0, ZCRYPTOSALT = NULL, ZCRYPTOITERATIONCOUNT = 0, ZCRYPTOWRAPPEDKEY = NULL,
               ZCRYPTOVERIFIER = NULL, ZCRYPTOINITIALIZATIONVECTOR = NULL, ZCRYPTOTAG = NULL, ZENCRYPTEDVALUESJSON = NULL,
               ZASSETCRYPTOINITIALIZATIONVECTOR = NULL, ZASSETCRYPTOTAG = NULL, ZFALLBACKIMAGECRYPTOINITIALIZATIONVECTOR = NULL,
               ZFALLBACKIMAGECRYPTOTAG = NULL, ZFALLBACKPDFCRYPTOINITIALIZATIONVECTOR = NULL, ZFALLBACKPDFCRYPTOTAG = NULL,
               ZPASSWORDHINT = NULL, Z_OPT = Z_OPT + 1 WHERE Z_PK = ?1",
            [pk],
        )?;
        Ok(())
    }

    fn seal_values(&self, pk: i64, key: &[u8; 16], values: &serde_json::Value) -> Result<()> {
        let (iv, tag, data) = lock::seal(key, values.to_string().as_bytes());
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZENCRYPTEDVALUESJSON = ?2, ZCRYPTOINITIALIZATIONVECTOR = ?3, ZCRYPTOTAG = ?4 WHERE Z_PK = ?1",
            params![pk, data, iv, tag],
        )?;
        Ok(())
    }

    fn open_values(&self, pk: i64, key: &[u8; 16]) -> Result<serde_json::Value> {
        let (d, iv, tag): (Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>) = self.conn.query_row(
            "SELECT ZENCRYPTEDVALUESJSON, ZCRYPTOINITIALIZATIONVECTOR, ZCRYPTOTAG FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1",
            [pk],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        Ok(match (d, iv, tag) {
            (Some(d), Some(iv), Some(tag)) => lock::open(key, &iv, &tag, &d).and_then(|p| serde_json::from_slice(&p).ok()).unwrap_or_default(),
            _ => serde_json::Value::Null,
        })
    }

    /// A note's attachments and their scan pages: (pk, identifier, media pk).
    fn note_attachment_rows(&self, pk: i64) -> Result<Vec<(i64, String, Option<i64>)>> {
        let mut st = self.conn.prepare(
            "SELECT Z_PK, ZIDENTIFIER, ZMEDIA FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1
               AND (ZNOTE = ?2 OR ZPARENTATTACHMENT IN (SELECT Z_PK FROM ZICCLOUDSYNCINGOBJECT WHERE ZNOTE = ?2))",
        )?;
        Ok(st.query_map(params![self.ent("ICAttachment"), pk], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<std::result::Result<_, _>>()?)
    }

    /// Encrypt a note in place, the way Notes stores a locked note: its body, its inline tokens' text,
    /// its attachments' data (tables, titles, summaries, URLs), media files and fallback images. Parts
    /// already encrypted with this lock keep their keys; plain parts (new or just rewritten) are sealed.
    fn seal_note(&self, pk: i64, l: &Lock) -> Result<()> {
        let acct = self.dir.join("Accounts").join(ACCOUNT_ID);
        // body
        let (key, wrapped) = l.new_key();
        let data: Option<Vec<u8>> =
            self.conn.query_row("SELECT ZDATA FROM ZICNOTEDATA WHERE Z_PK = (SELECT ZNOTEDATA FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1)", [pk], |r| r.get(0))?;
        let plain = match data {
            Some(d) if self.is_locked(pk)? => {
                // already sealed (nothing rewrote the body): re-encrypt under the new key
                let (iv, tag): (Option<Vec<u8>>, Option<Vec<u8>>) = self.conn.query_row(
                    "SELECT ZCRYPTOINITIALIZATIONVECTOR, ZCRYPTOTAG FROM ZICNOTEDATA WHERE Z_PK = (SELECT ZNOTEDATA FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1)",
                    [pk],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                match (iv, tag) {
                    (Some(iv), Some(tag)) => {
                        let (k, _) = self.row_key(pk, l)?;
                        lock::open(&k, &iv, &tag, &d).unwrap_or(d)
                    }
                    _ => d,
                }
            }
            d => d.unwrap_or_default(),
        };
        let (iv, tag, ct) = lock::seal(&key, &plain);
        self.conn.execute(
            "UPDATE ZICNOTEDATA SET ZDATA = ?2, ZCRYPTOINITIALIZATIONVECTOR = ?3, ZCRYPTOTAG = ?4, Z_OPT = Z_OPT + 1
             WHERE Z_PK = (SELECT ZNOTEDATA FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1)",
            params![pk, ct, iv, tag],
        )?;
        self.set_crypto(pk, l, &wrapped)?;
        self.conn.execute(
            "UPDATE ZICCLOUDSYNCINGOBJECT SET ZSNIPPET = NULL, ZWIDGETSNIPPET = NULL, ZPASSWORDHINT = ?2 WHERE Z_PK = ?1",
            params![pk, l.hint],
        )?;
        // inline tokens
        let inline: Vec<(i64, Option<String>, Option<String>)> = {
            let mut st = self.conn.prepare(
                "SELECT Z_PK, ZALTTEXT, ZTOKENCONTENTIDENTIFIER FROM ZICCLOUDSYNCINGOBJECT
                 WHERE Z_ENT = ?1 AND ZNOTE1 = ?2 AND (ZISPASSWORDPROTECTED = 0 OR ZALTTEXT IS NOT NULL)",
            )?;
            st.query_map(params![self.ent("ICInlineAttachment"), pk], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<std::result::Result<_, _>>()?
        };
        for (ipk, alt, content) in inline {
            let (k, w) = self.row_key(ipk, l)?;
            self.seal_values(ipk, &k, &serde_json::json!({"altText": alt.unwrap_or_default(), "tokenContentIdentifier": content.unwrap_or_default()}))?;
            self.set_crypto(ipk, l, &w)?;
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZALTTEXT = NULL, ZTOKENCONTENTIDENTIFIER = NULL WHERE Z_PK = ?1", [ipk])?;
        }
        // attachments
        for (apk, aid, media) in self.note_attachment_rows(pk)? {
            type Plain = (Option<Vec<u8>>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);
            let (mergeable, summary, title, url, img_gen, pdf_gen): Plain = self.conn.query_row(
                "SELECT ZMERGEABLEDATA1, ZSUMMARY, ZTITLE, ZURLSTRING, ZFALLBACKIMAGEGENERATION, ZFALLBACKPDFGENERATION FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1",
                [apk],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )?;
            let sealed = self.is_locked(apk)?;
            let (k, w) = self.row_key(apk, l)?;
            if !sealed || mergeable.is_some() || summary.is_some() || title.is_some() || url.is_some() {
                let mut v = if sealed { self.open_values(apk, &k)? } else { serde_json::Value::Null };
                if !v.is_object() {
                    v = serde_json::json!({});
                }
                let o = v.as_object_mut().unwrap();
                if let Some(m) = mergeable {
                    o.insert("mergeableData".into(), lock::base64(&m).into());
                }
                for (name, val) in [("summary", summary), ("title", title), ("urlString", url)] {
                    if let Some(val) = val {
                        o.insert(name.into(), val.into());
                    }
                }
                self.seal_values(apk, &k, &v)?;
                self.conn.execute(
                    "UPDATE ZICCLOUDSYNCINGOBJECT SET ZMERGEABLEDATA1 = NULL, ZSUMMARY = NULL, ZTITLE = NULL, ZURLSTRING = NULL WHERE Z_PK = ?1",
                    [apk],
                )?;
            }
            self.set_crypto(apk, l, &w)?;
            // the bundle isn't encrypted: a locked drawing keeps only its sealed copy
            paperkit::remove(&paperkit::bundle_dir(&acct, &aid));
            for (folder, file, generation, cols) in [
                ("FallbackImages", "FallbackImage.png", &img_gen, ("ZFALLBACKIMAGECRYPTOINITIALIZATIONVECTOR", "ZFALLBACKIMAGECRYPTOTAG")),
                ("FallbackPDFs", "FallbackPDF.pdf", &pdf_gen, ("ZFALLBACKPDFCRYPTOINITIALIZATIONVECTOR", "ZFALLBACKPDFCRYPTOTAG")),
            ] {
                let Some(g) = generation else { continue };
                let p = acct.join(folder).join(&aid).join(g).join(file);
                if let Ok(b) = std::fs::read(&p) {
                    let (iv, tag, ct) = lock::seal(&k, &b);
                    std::fs::write(p.with_file_name(format!("{file}.encrypted")), ct)?;
                    std::fs::remove_file(&p)?;
                    self.conn.execute(&format!("UPDATE ZICCLOUDSYNCINGOBJECT SET {} = ?2, {} = ?3 WHERE Z_PK = ?1", cols.0, cols.1), params![apk, iv, tag])?;
                }
            }
            // its media file
            let Some(mpk) = media else { continue };
            if self.is_locked(mpk)? {
                continue;
            }
            let (mid, name, generation): (String, Option<String>, Option<String>) = self.conn.query_row(
                "SELECT ZIDENTIFIER, ZFILENAME, ZGENERATION1 FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1",
                [mpk],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let name = name.unwrap_or_default();
            let dir = generation.map(|g| acct.join("Media").join(&mid).join(g)).unwrap_or_else(|| acct.join("Media").join(&mid));
            let (mk, mw) = l.new_key();
            if let Ok(b) = std::fs::read(dir.join(&name)) {
                let (iv, tag, ct) = lock::seal(&mk, &b);
                std::fs::write(dir.join(&mid), ct)?;
                if name != mid {
                    std::fs::remove_file(dir.join(&name))?;
                }
                self.conn.execute(
                    "UPDATE ZICCLOUDSYNCINGOBJECT SET ZASSETCRYPTOINITIALIZATIONVECTOR = ?2, ZASSETCRYPTOTAG = ?3 WHERE Z_PK = ?1",
                    params![mpk, iv, tag],
                )?;
            }
            self.seal_values(mpk, &mk, &serde_json::json!({"filename": name}))?;
            self.set_crypto(mpk, l, &mw)?;
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZFILENAME = NULL WHERE Z_PK = ?1", [mpk])?;
        }
        Ok(())
    }

    /// Undo [`Store::seal_note`]: every part decrypted back into its plain columns and files (the
    /// body is rewritten by the caller).
    fn unseal_note(&self, pk: i64, l: &Lock) -> Result<()> {
        let acct = self.dir.join("Accounts").join(ACCOUNT_ID);
        self.conn.execute(
            "UPDATE ZICNOTEDATA SET ZCRYPTOINITIALIZATIONVECTOR = NULL, ZCRYPTOTAG = NULL WHERE Z_PK = (SELECT ZNOTEDATA FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1)",
            [pk],
        )?;
        self.clear_crypto(pk)?;
        let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
        let inline: Vec<i64> = {
            let mut st = self.conn.prepare("SELECT Z_PK FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZNOTE1 = ?2 AND ZISPASSWORDPROTECTED = 1")?;
            st.query_map(params![self.ent("ICInlineAttachment"), pk], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?
        };
        for ipk in inline {
            let (k, _) = self.row_key(ipk, l)?;
            let v = self.open_values(ipk, &k)?;
            self.clear_crypto(ipk)?;
            self.conn.execute(
                "UPDATE ZICCLOUDSYNCINGOBJECT SET ZALTTEXT = ?2, ZTOKENCONTENTIDENTIFIER = ?3 WHERE Z_PK = ?1",
                params![ipk, s(&v, "altText"), s(&v, "tokenContentIdentifier")],
            )?;
        }
        for (apk, aid, media) in self.note_attachment_rows(pk)? {
            if self.is_locked(apk)? {
                let (k, _) = self.row_key(apk, l)?;
                let v = self.open_values(apk, &k)?;
                type Sealed = (Option<String>, Option<String>, Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>);
                let (img_gen, pdf_gen, img_iv, img_tag, pdf_iv, pdf_tag): Sealed = self.conn.query_row(
                    "SELECT ZFALLBACKIMAGEGENERATION, ZFALLBACKPDFGENERATION, ZFALLBACKIMAGECRYPTOINITIALIZATIONVECTOR, ZFALLBACKIMAGECRYPTOTAG,
                       ZFALLBACKPDFCRYPTOINITIALIZATIONVECTOR, ZFALLBACKPDFCRYPTOTAG FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1",
                    [apk],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                )?;
                for (folder, file, generation, iv, tag) in
                    [("FallbackImages", "FallbackImage.png", img_gen, img_iv, img_tag), ("FallbackPDFs", "FallbackPDF.pdf", pdf_gen, pdf_iv, pdf_tag)]
                {
                    let (Some(g), Some(iv), Some(tag)) = (generation, iv, tag) else { continue };
                    let p = acct.join(folder).join(&aid).join(g).join(format!("{file}.encrypted"));
                    if let Some(plain) = std::fs::read(&p).ok().and_then(|b| lock::open(&k, &iv, &tag, &b)) {
                        std::fs::write(p.with_file_name(file), plain)?;
                        std::fs::remove_file(&p)?;
                    }
                }
                self.clear_crypto(apk)?;
                self.conn.execute(
                    "UPDATE ZICCLOUDSYNCINGOBJECT SET ZMERGEABLEDATA1 = ?2, ZSUMMARY = ?3, ZTITLE = ?4, ZURLSTRING = ?5 WHERE Z_PK = ?1",
                    params![apk, s(&v, "mergeableData").and_then(|b| lock::unbase64(&b)), s(&v, "summary"), s(&v, "title"), s(&v, "urlString")],
                )?;
                if let Some(d) = s(&v, "mergeableData").and_then(|b| lock::unbase64(&b)).and_then(|b| paper::decode_ink(&b)) {
                    self.write_bundle(apk, &aid, &d)?;
                }
                let (uti, pdf_gen): (Option<String>, Option<String>) =
                    self.conn.query_row("SELECT ZTYPEUTI, ZFALLBACKPDFGENERATION FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1", [apk], |r| Ok((r.get(0)?, r.get(1)?)))?;
                if let (Some(UTI_PAPER_PDF), Some(g)) = (uti.as_deref(), pdf_gen) {
                    let f = acct.join("FallbackPDFs").join(&aid).join(g).join("FallbackPDF.pdf");
                    if let Ok(b) = std::fs::read(&f) {
                        paperkit::write_document(&paperkit::bundle_dir(&acct, &aid), &b, &paperkit::pdf_pages(&f))?;
                    }
                }
            }
            let Some(mpk) = media else { continue };
            if !self.is_locked(mpk)? {
                continue;
            }
            let (k, _) = self.row_key(mpk, l)?;
            let v = self.open_values(mpk, &k)?;
            let (mid, generation, iv, tag): (String, Option<String>, Option<Vec<u8>>, Option<Vec<u8>>) = self.conn.query_row(
                "SELECT ZIDENTIFIER, ZGENERATION1, ZASSETCRYPTOINITIALIZATIONVECTOR, ZASSETCRYPTOTAG FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1",
                [mpk],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
            let name = s(&v, "filename").filter(|n| !n.is_empty()).map(|n| sanitize(&n)).unwrap_or_else(|| mid.clone());
            let dir = generation.map(|g| acct.join("Media").join(&mid).join(g)).unwrap_or_else(|| acct.join("Media").join(&mid));
            if let (Some(iv), Some(tag)) = (iv, tag) {
                if let Some(plain) = std::fs::read(dir.join(&mid)).ok().and_then(|b| lock::open(&k, &iv, &tag, &b)) {
                    std::fs::write(dir.join(&name), plain)?;
                    if name != mid {
                        std::fs::remove_file(dir.join(&mid))?;
                    }
                }
            }
            self.clear_crypto(mpk)?;
            self.conn.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZFILENAME = ?2 WHERE Z_PK = ?1", params![mpk, name])?;
        }
        Ok(())
    }
}

/// Notes' generation names: "<n>_<UUID>", n counting up.
fn next_generation(old: Option<&str>) -> String {
    let n = old.and_then(|g| g.split('_').next()).and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
    format!("{}_{}", n + 1, pb::new_uuid())
}

/// A file name safe to put on disk (no path separators).
fn sanitize(name: &str) -> String {
    let s: String = name.chars().map(|c| if c == '/' || c == '\0' { '_' } else { c }).collect();
    if s.is_empty() || s == "." || s == ".." { "file".into() } else { s }
}

/// Copy a file or folder (Notes' AppleDouble "._" files left out).
fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)?.flatten() {
            if !e.file_name().to_string_lossy().starts_with("._") {
                copy_tree(&e.path(), &dst.join(e.file_name()))?;
            }
        }
    } else {
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(src, dst)?;
    }
    Ok(())
}
