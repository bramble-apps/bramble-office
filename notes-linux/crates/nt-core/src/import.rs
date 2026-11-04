//! Reading a Mac's Notes library: a copied `group.com.apple.notes` folder (NoteStore.sqlite +
//! -wal/-shm + Accounts/<id>/Media, FallbackImages, FallbackPDFs, Previews, Paper) or just a
//! NoteStore.sqlite, into [`model::Library`].
//!
//! The store we are handed is never written: it is copied (with its -wal/-shm, so recent changes that
//! are still in the WAL are seen) to a temporary folder and the copy is opened read-only.
//! Entity ids come from Z_PRIMARYKEY by name; columns are resolved per entity (Core Data numbers
//! colliding attribute names, and the numbers differ between macOS versions), see [`Schema`].
//! Logic ported in part from apple_cloud_notes_parser (MIT, see THIRD-PARTY.md).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::body::{self, APPLE_EPOCH};
use crate::lock;
use crate::model::*;
use crate::table;
use crate::Error;

type Result<T> = std::result::Result<T, Error>;

/// What an import read and what it couldn't.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub accounts: usize,
    pub folders: usize,
    pub notes: usize,
    pub attachments: usize,
    /// locked notes that no given password opened (their bodies were not read)
    pub locked: Vec<String>,
    /// locked notes opened with a given password
    pub unlocked: usize,
    /// accounts with a locked-notes password: (account id, its hint or "")
    pub password_hints: Vec<(String, String)>,
    /// notes or tables whose data could not be decoded: "title: reason"
    pub unreadable: Vec<String>,
    /// iCloud notes whose body the Mac hadn't downloaded (open them on the Mac first)
    pub not_downloaded: usize,
    /// attachments iCloud hadn't downloaded to the Mac (no data or file)
    pub attachments_not_downloaded: usize,
    /// locked notes using the Mac's login password ("Use login password"): their key is in the Mac's
    /// keychain, so only switching to a custom Notes password on the Mac makes them openable here
    pub locked_with_login_password: usize,
    /// attachments whose file is not in the copied folder (e.g. only NoteStore.sqlite was given,
    /// or iCloud hadn't downloaded it): "note title: file name"
    pub missing_files: Vec<String>,
}

/// Read a library: `path` is a group.com.apple.notes folder, or a NoteStore.sqlite file.
pub fn read_library(path: &Path) -> Result<Library> {
    read_library_with_report(path).map(|(l, _)| l)
}

pub fn read_library_with_report(path: &Path) -> Result<(Library, Report)> {
    read_library_with(path, &Options::default())
}

/// How to read a library.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// passwords to open locked notes with: each is tried on each locked note
    pub passwords: Vec<String>,
    /// keys of notes already opened (the store's own notes being saved)
    pub(crate) locks: Vec<Lock>,
    /// read only this note (by identifier)
    pub(crate) only: Option<String>,
    /// read what the notes list needs, decoding no bodies, tables, drawings or documents
    pub(crate) meta: bool,
    /// (with `meta`) every attachment too, light (the attachments browser)
    pub(crate) all_attachments: bool,
}

impl Options {
    pub fn with_passwords<S: AsRef<str>>(passwords: &[S]) -> Options {
        Options { passwords: passwords.iter().map(|p| p.as_ref().to_string()).collect(), ..Options::default() }
    }
}

/// [`read_library_with_report`], opening locked notes with `opts.passwords`. Their decrypted
/// attachment files go to [`lock::unlocked_dir`].
pub fn read_library_with(path: &Path, opts: &Options) -> Result<(Library, Report)> {
    let (db, root) = if path.is_dir() {
        let db = path.join("NoteStore.sqlite");
        if !db.is_file() {
            return Err(Error::NotFound(format!("{} has no NoteStore.sqlite", path.display())));
        }
        (db, path.to_path_buf())
    } else {
        (path.to_path_buf(), path.parent().unwrap_or(Path::new(".")).to_path_buf())
    };
    let copy = TempCopy::new(&db)?;
    let conn = Connection::open_with_flags(&copy.db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    read_conn(&conn, root, opts)
}

/// Read an open NoteStore connection; media paths are resolved under `root` (the store's folder).
pub(crate) fn read_conn(conn: &Connection, root: PathBuf, opts: &Options) -> Result<(Library, Report)> {
    let schema = Schema::load(conn)?;
    Reader::new(conn, root, opts, &schema).read()
}

/// [`read_conn`] with a schema already loaded (the store keeps one: loading it counts columns).
pub(crate) fn read_conn_with(conn: &Connection, root: PathBuf, opts: &Options, schema: &Schema) -> Result<(Library, Report)> {
    Reader::new(conn, root, opts, schema).read()
}

/// A private copy of the store (and its -wal/-shm) in the temp folder, removed on drop.
struct TempCopy {
    dir: PathBuf,
    db: PathBuf,
}

impl TempCopy {
    fn new(db: &Path) -> Result<TempCopy> {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("bramble-notes-import-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let name = db.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let out = dir.join(&name);
        std::fs::copy(db, &out)?;
        for ext in ["-wal", "-shm"] {
            let side = db.with_file_name(format!("{name}{ext}"));
            if side.is_file() {
                std::fs::copy(&side, dir.join(format!("{name}{ext}")))?;
            }
        }
        Ok(TempCopy { dir, db: out })
    }
}

impl Drop for TempCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Entity ids and per-entity column names of one store.
pub struct Schema {
    ents: HashMap<String, i64>,
    cols: Vec<String>,
    /// the columns an index starts with
    indexed: HashSet<String>,
    /// (entity, base name) -> resolved column
    cache: std::cell::RefCell<HashMap<(i64, String), Option<String>>>,
}

/// Notes 4.13's column for each (entity, attribute base) where it is numbered (format/COLUMN-MAP.txt);
/// used to break ties.
const PREFERRED: &[(&str, &str, &str)] = &[
    ("ICNote", "ZTITLE", "ZTITLE1"),
    ("ICNote", "ZCREATIONDATE", "ZCREATIONDATE3"),
    ("ICNote", "ZMODIFICATIONDATE", "ZMODIFICATIONDATE1"),
    ("ICNote", "ZACCOUNT", "ZACCOUNT7"),
    ("ICFolder", "ZTITLE", "ZTITLE2"),
    ("ICFolder", "ZACCOUNT", "ZACCOUNT8"),
    ("ICAttachment", "ZMERGEABLEDATA", "ZMERGEABLEDATA1"),
    ("ICAttachment", "ZACCOUNT", "ZACCOUNT1"),
    ("ICMedia", "ZGENERATION", "ZGENERATION1"),
    ("ICMedia", "ZACCOUNT", "ZACCOUNT6"),
    ("ICMedia", "ZATTACHMENT", "ZATTACHMENT1"),
    ("ICInlineAttachment", "ZNOTE", "ZNOTE1"),
    ("ICInlineAttachment", "ZTYPEUTI", "ZTYPEUTI1"),
    ("ICHashtag", "ZACCOUNT", "ZACCOUNT3"),
];

impl Schema {
    pub fn load(conn: &Connection) -> Result<Schema> {
        let mut ents = HashMap::new();
        let mut st = conn.prepare("SELECT Z_ENT, Z_NAME FROM Z_PRIMARYKEY")?;
        for r in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (e, n) = r?;
            ents.insert(n, e);
        }
        if !ents.contains_key("ICNote") {
            return Err(Error::Format("not a Notes store (no ICNote entity); notes from before macOS 10.11 are not supported".into()));
        }
        let mut st = conn.prepare("PRAGMA table_info(ZICCLOUDSYNCINGOBJECT)")?;
        let cols = st.query_map([], |r| r.get::<_, String>(1))?.collect::<std::result::Result<Vec<_>, _>>()?;
        let mut indexed = HashSet::new();
        let names: Vec<String> = conn.prepare("SELECT name FROM pragma_index_list('ZICCLOUDSYNCINGOBJECT')")?.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
        for n in names {
            if let Ok(c) = conn.query_row("SELECT name FROM pragma_index_info(?1) WHERE seqno = 0", [&n], |r| r.get::<_, Option<String>>(0)) {
                indexed.extend(c);
            }
        }
        Ok(Schema { ents, cols, indexed, cache: Default::default() })
    }

    pub fn ent(&self, name: &str) -> Option<i64> {
        self.ents.get(name).copied()
    }

    fn ent_name(&self, ent: i64) -> &str {
        self.ents.iter().find(|(_, e)| **e == ent).map(|(n, _)| n.as_str()).unwrap_or("")
    }

    /// The column entity `ent` stores attribute `base` in: `base` itself or `base<digits>`, whichever has
    /// the most values for that entity's rows (ties: Notes 4.13's column, then the plain name).
    pub fn col(&self, conn: &Connection, ent: i64, base: &str) -> Option<String> {
        let key = (ent, base.to_string());
        if let Some(c) = self.cache.borrow().get(&key) {
            return c.clone();
        }
        let cands: Vec<&String> = self
            .cols
            .iter()
            .filter(|c| c.as_str() == base || c.strip_prefix(base).is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())))
            .collect();
        let preferred = PREFERRED.iter().find(|(e, b, _)| *e == self.ent_name(ent) && *b == base).map(|(_, _, c)| *c);
        // one candidate: no need to count (each count reads the whole table)
        if cands.len() <= 1 {
            let out = cands.first().map(|c| c.to_string());
            self.cache.borrow_mut().insert(key, out.clone());
            return out;
        }
        // several: their counts in one pass (by typeof, which reads no values: COUNT reads every blob)
        let sql = format!(
            "SELECT {} FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1",
            cands.iter().map(|c| format!("IFNULL(SUM(typeof({c}) != 'null'), 0)")).collect::<Vec<_>>().join(", ")
        );
        let counts: Vec<i64> = conn.query_row(&sql, [ent], |r| (0..cands.len()).map(|i| r.get::<_, i64>(i)).collect()).unwrap_or_else(|_| vec![0; cands.len()]);
        let mut best: Option<(i64, u8, String)> = None;
        for (c, n) in cands.into_iter().zip(counts) {
            let rank = if Some(c.as_str()) == preferred { 2 } else if c == base { 1 } else { 0 };
            if best.as_ref().is_none_or(|(bn, br, _)| (n, rank) > (*bn, *br)) {
                best = Some((n, rank, c.clone()));
            }
        }
        let out = best.map(|(_, _, c)| c);
        self.cache.borrow_mut().insert(key, out.clone());
        out
    }
}

/// One row of ZICCLOUDSYNCINGOBJECT as attribute -> value.
struct Row(HashMap<&'static str, Value>);

impl Row {
    fn i(&self, k: &str) -> Option<i64> {
        match self.0.get(k)? {
            Value::Integer(i) => Some(*i),
            Value::Real(f) => Some(*f as i64),
            _ => None,
        }
    }
    fn f(&self, k: &str) -> Option<f64> {
        match self.0.get(k)? {
            Value::Integer(i) => Some(*i as f64),
            Value::Real(f) => Some(*f),
            _ => None,
        }
    }
    fn s(&self, k: &str) -> Option<String> {
        match self.0.get(k)? {
            Value::Text(t) => Some(t.clone()),
            Value::Blob(b) => Some(String::from_utf8_lossy(b).into_owned()),
            _ => None,
        }
    }
    fn b(&self, k: &str) -> Option<Vec<u8>> {
        match self.0.get(k)? {
            Value::Blob(b) => Some(b.clone()),
            Value::Text(t) => Some(t.as_bytes().to_vec()),
            _ => None,
        }
    }
    fn flag(&self, k: &str) -> bool {
        self.i(k).unwrap_or(0) != 0
    }
    /// A Core Data date (2001 epoch) as Unix seconds.
    fn date(&self, k: &str) -> f64 {
        self.f(k).map(|d| d + APPLE_EPOCH).unwrap_or(0.0)
    }
}

struct Reader<'a> {
    conn: &'a Connection,
    schema: &'a Schema,
    root: PathBuf,
    opts: &'a Options,
    /// locks found by trying the passwords, tried first on the next encrypted row
    found: std::cell::RefCell<Vec<Lock>>,
    /// [`Options::only`]: the pk of the note to read
    want: std::cell::Cell<Option<i64>>,
    /// [`Options::meta`]: the attachments to resolve (the notes' list thumbnails)
    thumbs: std::cell::RefCell<std::collections::HashSet<String>>,
}

/// The crypto columns of an encrypted row.
const CRYPTO: [&str; 7] = [
    "ZISPASSWORDPROTECTED",
    "ZCRYPTOSALT",
    "ZCRYPTOITERATIONCOUNT",
    "ZCRYPTOWRAPPEDKEY",
    "ZCRYPTOVERIFIER",
    "ZCRYPTOINITIALIZATIONVECTOR",
    "ZCRYPTOTAG",
];

impl<'a> Reader<'a> {
    fn new(conn: &'a Connection, root: PathBuf, opts: &'a Options, schema: &'a Schema) -> Reader<'a> {
        Reader { conn, schema, root, opts, found: Default::default(), want: Default::default(), thumbs: Default::default() }
    }

    /// An encrypted row's own key, and the lock (password) that unwrapped it.
    fn open_row(&self, r: &Row) -> Option<([u8; 16], Lock)> {
        let salt = r.b("ZCRYPTOSALT")?;
        let iterations = r.i("ZCRYPTOITERATIONCOUNT")? as u32;
        let wrapped = r.b("ZCRYPTOWRAPPEDKEY").or_else(|| r.b("ZCRYPTOVERIFIER"))?;
        let known: Vec<Lock> = self.opts.locks.iter().cloned().chain(self.found.borrow().iter().cloned()).collect();
        for l in known {
            if let Some(k) = l.key(&salt, iterations, &wrapped) {
                return Some((k, l));
            }
        }
        for pw in &self.opts.passwords {
            let l = Lock::with_salt(pw, &salt, iterations, None);
            if let Some(k) = l.key(&salt, iterations, &wrapped) {
                self.found.borrow_mut().push(l.clone());
                return Some((k, l));
            }
        }
        None
    }

    /// An encrypted row's ZENCRYPTEDVALUESJSON, decrypted.
    fn open_values(r: &Row, key: &[u8; 16]) -> Option<serde_json::Value> {
        let plain = lock::open(key, &r.b("ZCRYPTOINITIALIZATIONVECTOR")?, &r.b("ZCRYPTOTAG")?, &r.b("ZENCRYPTEDVALUESJSON")?)?;
        serde_json::from_slice(&plain).ok()
    }

    /// Every row of `entity`, with the given attributes (base column names) resolved for it.
    fn rows(&self, entity: &str, attrs: &[&'static str]) -> Result<Vec<Row>> {
        let Some(ent) = self.schema.ent(entity) else { return Ok(Vec::new()) };
        let mut sel = vec!["Z_PK".to_string()];
        let mut names: Vec<&'static str> = vec!["Z_PK"];
        for &a in attrs {
            if let Some(c) = self.schema.col(self.conn, ent, a) {
                sel.push(c);
                names.push(a);
            }
        }
        let sql = format!("SELECT {} FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 ORDER BY Z_PK", sel.join(", "));
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([ent], |r| {
            let mut m = HashMap::new();
            for (i, n) in names.iter().enumerate() {
                m.insert(*n, r.get::<_, Value>(i)?);
            }
            Ok(Row(m))
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    fn crypto_of(r: &Row) -> Crypto {
        Crypto {
            salt: r.b("ZCRYPTOSALT"),
            iterations: r.i("ZCRYPTOITERATIONCOUNT").unwrap_or(0),
            wrapped_key: r.b("ZCRYPTOWRAPPEDKEY"),
            verifier: r.b("ZCRYPTOVERIFIER"),
            iv: r.b("ZCRYPTOINITIALIZATIONVECTOR"),
            tag: r.b("ZCRYPTOTAG"),
            values: r.b("ZENCRYPTEDVALUESJSON"),
        }
    }

    /// A locked note's attachments, media, inline tokens and encrypted files, as they are.
    fn sealed(&self, note_pk: i64) -> Result<Sealed> {
        let mut s = Sealed::default();
        let mut attrs = vec![
            "ZIDENTIFIER", "ZTYPEUTI", "ZNOTE", "ZMEDIA", "ZPARENTATTACHMENT", "ZSIZEWIDTH", "ZSIZEHEIGHT", "ZCREATIONDATE", "ZMODIFICATIONDATE",
            "ZFALLBACKIMAGEGENERATION", "ZFALLBACKPDFGENERATION", "ZFALLBACKIMAGECRYPTOINITIALIZATIONVECTOR", "ZFALLBACKIMAGECRYPTOTAG",
            "ZFALLBACKPDFCRYPTOINITIALIZATIONVECTOR", "ZFALLBACKPDFCRYPTOTAG", "ZENCRYPTEDVALUESJSON", "ZMARKEDFORDELETION",
        ];
        attrs.extend(CRYPTO);
        let rows = self.rows("ICAttachment", &attrs)?;
        let mut mine: Vec<&Row> = rows.iter().filter(|r| r.i("ZNOTE") == Some(note_pk) && !r.flag("ZMARKEDFORDELETION")).collect();
        let parents: HashMap<i64, String> = mine.iter().map(|r| (r.i("Z_PK").unwrap_or(0), r.s("ZIDENTIFIER").unwrap_or_default())).collect();
        mine.extend(rows.iter().filter(|r| r.i("ZPARENTATTACHMENT").is_some_and(|p| parents.contains_key(&p)) && !r.flag("ZMARKEDFORDELETION")));
        let mut mattrs = vec!["ZIDENTIFIER", "ZGENERATION", "ZASSETCRYPTOINITIALIZATIONVECTOR", "ZASSETCRYPTOTAG", "ZENCRYPTEDVALUESJSON"];
        mattrs.extend(CRYPTO);
        let media: HashMap<i64, Row> = self.rows("ICMedia", &mattrs)?.into_iter().map(|r| (r.i("Z_PK").unwrap_or(0), r)).collect();
        let mut file = |rel: PathBuf| {
            if let Some(src) = self.bases(None).into_iter().map(|b| b.join(&rel)).find(|p| p.exists()) {
                s.files.push((src, rel));
            }
        };
        for r in mine {
            let id = r.s("ZIDENTIFIER").unwrap_or_default();
            for dir in ["FallbackImages", "FallbackPDFs"] {
                file(PathBuf::from(dir).join(&id));
            }
            file(PathBuf::from("Paper/Bundles").join(format!("{id}.bundle")));
            let m = r.i("ZMEDIA").and_then(|m| media.get(&m)).map(|m| {
                let mid = m.s("ZIDENTIFIER").unwrap_or_default();
                file(PathBuf::from("Media").join(&mid));
                SealedMedia { id: mid, generation: m.s("ZGENERATION"), asset_iv: m.b("ZASSETCRYPTOINITIALIZATIONVECTOR"), asset_tag: m.b("ZASSETCRYPTOTAG"), crypto: Self::crypto_of(m) }
            });
            s.attachments.push(SealedAttachment {
                id,
                uti: r.s("ZTYPEUTI").unwrap_or_default(),
                parent: r.i("ZPARENTATTACHMENT").and_then(|p| parents.get(&p).cloned()),
                width: r.f("ZSIZEWIDTH").unwrap_or(0.0),
                height: r.f("ZSIZEHEIGHT").unwrap_or(0.0),
                created: r.f("ZCREATIONDATE"),
                modified: r.f("ZMODIFICATIONDATE"),
                fallback_image: r.s("ZFALLBACKIMAGEGENERATION"),
                fallback_pdf: r.s("ZFALLBACKPDFGENERATION"),
                fallback_image_iv: r.b("ZFALLBACKIMAGECRYPTOINITIALIZATIONVECTOR"),
                fallback_image_tag: r.b("ZFALLBACKIMAGECRYPTOTAG"),
                fallback_pdf_iv: r.b("ZFALLBACKPDFCRYPTOINITIALIZATIONVECTOR"),
                fallback_pdf_tag: r.b("ZFALLBACKPDFCRYPTOTAG"),
                crypto: Self::crypto_of(r),
                media: m,
            });
        }
        let mut iattrs = vec!["ZIDENTIFIER", "ZTYPEUTI", "ZNOTE", "ZENCRYPTEDVALUESJSON"];
        iattrs.extend(CRYPTO);
        for r in self.rows("ICInlineAttachment", &iattrs)?.iter().filter(|r| r.i("ZNOTE") == Some(note_pk)) {
            s.inline.push(SealedInline { id: r.s("ZIDENTIFIER").unwrap_or_default(), uti: r.s("ZTYPEUTI").unwrap_or_default(), crypto: Self::crypto_of(r) });
        }
        Ok(s)
    }

    /// [`Reader::rows`] whose `property` (a column, or Z_PK) is one of `values`.
    fn rows_in(&self, entity: &str, attrs: &[&'static str], property: &str, values: &[Value]) -> Result<Vec<Row>> {
        let Some(ent) = self.schema.ent(entity) else { return Ok(Vec::new()) };
        let col = if property == "Z_PK" { Some("Z_PK".to_string()) } else { self.schema.col(self.conn, ent, property) };
        let (Some(col), false) = (col, values.is_empty()) else { return Ok(Vec::new()) };
        let mut sel = vec!["Z_PK".to_string()];
        let mut names: Vec<&'static str> = vec!["Z_PK"];
        for &a in attrs {
            if let Some(c) = self.schema.col(self.conn, ent, a) {
                sel.push(c);
                names.push(a);
            }
        }
        let mut out = Vec::new();
        for chunk in values.chunks(500) {
            let marks = vec!["?"; chunk.len()].join(", ");
            // (by the column's own index where there is one: by the entity's, SQLite reads every row of it)
            let by = if self.schema.indexed.contains(&col) { "+Z_ENT" } else { "Z_ENT" };
            let sql = format!("SELECT {} FROM ZICCLOUDSYNCINGOBJECT WHERE {by} = {ent} AND {col} IN ({marks}) ORDER BY Z_PK", sel.join(", "));
            let mut st = self.conn.prepare(&sql)?;
            let rows = st.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                let mut m = HashMap::new();
                for (i, n) in names.iter().enumerate() {
                    m.insert(*n, r.get::<_, Value>(i)?);
                }
                Ok(Row(m))
            })?;
            out.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
        }
        Ok(out)
    }

    fn read(&self) -> Result<(Library, Report)> {
        let mut lib = Library::default();
        let mut rep = Report::default();

        // accounts
        let mut acct_id: HashMap<i64, String> = HashMap::new();
        for r in self.rows("ICAccount", &["ZIDENTIFIER", "ZNAME", "ZACCOUNTTYPE", "ZMARKEDFORDELETION", "ZCRYPTOVERIFIER", "ZPASSWORDHINT"])? {
            if r.flag("ZMARKEDFORDELETION") {
                continue;
            }
            let id = r.s("ZIDENTIFIER").unwrap_or_else(|| format!("account-{}", r.i("Z_PK").unwrap_or(0)));
            acct_id.insert(r.i("Z_PK").unwrap_or(0), id.clone());
            if r.b("ZCRYPTOVERIFIER").is_some_and(|v| !v.is_empty()) {
                rep.password_hints.push((id.clone(), r.s("ZPASSWORDHINT").unwrap_or_default()));
            }
            lib.accounts.push(Account { id, name: r.s("ZNAME").unwrap_or_default(), kind: r.i("ZACCOUNTTYPE").unwrap_or(0) });
        }

        // folders
        let mut folder_id: HashMap<i64, String> = HashMap::new();
        let mut trash: Vec<i64> = Vec::new();
        let frows = self.rows(
            "ICFolder",
            &["ZIDENTIFIER", "ZTITLE", "ZPARENT", "ZACCOUNT", "ZFOLDERTYPE", "ZSMARTFOLDERQUERYJSON", "ZSORTORDER", "ZMARKEDFORDELETION"],
        )?;
        for r in &frows {
            if !r.flag("ZMARKEDFORDELETION") {
                folder_id.insert(r.i("Z_PK").unwrap_or(0), r.s("ZIDENTIFIER").unwrap_or_else(|| format!("folder-{}", r.i("Z_PK").unwrap_or(0))));
            }
        }
        for r in &frows {
            let pk = r.i("Z_PK").unwrap_or(0);
            let Some(id) = folder_id.get(&pk).cloned() else { continue };
            let kind = match r.i("ZFOLDERTYPE").unwrap_or(0) {
                1 => FolderKind::Trash,
                2 => FolderKind::Smart,
                _ => FolderKind::Normal,
            };
            if kind == FolderKind::Trash {
                trash.push(pk);
            }
            let smart = r.s("ZSMARTFOLDERQUERYJSON").filter(|s| !s.is_empty());
            lib.folders.push(Folder {
                default: id.starts_with("DefaultFolder-"),
                account: r.i("ZACCOUNT").and_then(|a| acct_id.get(&a).cloned()).unwrap_or_default(),
                name: r.s("ZTITLE").unwrap_or_default(),
                parent: r.i("ZPARENT").and_then(|p| folder_id.get(&p).cloned()),
                kind: if smart.is_some() { FolderKind::Smart } else { kind },
                smart,
                sort_order: r.i("ZSORTORDER").unwrap_or(0),
                id,
            });
        }

        // tags
        for r in self.rows("ICHashtag", &["ZDISPLAYTEXT", "ZACCOUNT", "ZMARKEDFORDELETION"])? {
            if !r.flag("ZMARKEDFORDELETION") {
                lib.tags.push(Tag {
                    account: r.i("ZACCOUNT").and_then(|a| acct_id.get(&a).cloned()).unwrap_or_default(),
                    name: r.s("ZDISPLAYTEXT").unwrap_or_default(),
                });
            }
        }

        // one note only: its pk
        let note_ent = self.schema.ent("ICNote");
        if let (Some(only), Some(ent)) = (&self.opts.only, note_ent) {
            let pk: Option<i64> = self
                .conn
                .query_row("SELECT Z_PK FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT = ?1 AND ZIDENTIFIER = ?2", rusqlite::params![ent, only], |r| r.get(0))
                .optional()?;
            self.want.set(Some(pk.unwrap_or(-1)));
        }
        let wanted = |pk: Option<i64>| self.want.get().is_none_or(|w| pk == Some(w));

        // note bodies: ZICNOTEDATA by its pk and by its note
        // (data, IV, tag): locked notes' data is encrypted
        type NoteData = (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>);
        let mut data_by_pk: HashMap<i64, NoteData> = HashMap::new();
        let mut data_by_note: HashMap<i64, NoteData> = HashMap::new();
        if !self.opts.meta {
            let has_iv = self.conn.prepare("SELECT ZCRYPTOINITIALIZATIONVECTOR, ZCRYPTOTAG FROM ZICNOTEDATA LIMIT 0").is_ok();
            let cols = if has_iv { "Z_PK, ZNOTE, ZDATA, ZCRYPTOINITIALIZATIONVECTOR, ZCRYPTOTAG" } else { "Z_PK, ZNOTE, ZDATA, NULL, NULL" };
            let notedata = note_ent.and_then(|e| self.schema.col(self.conn, e, "ZNOTEDATA")).unwrap_or_else(|| "ZNOTEDATA".into());
            let sql = match self.want.get() {
                Some(_) => format!("SELECT {cols} FROM ZICNOTEDATA WHERE ZNOTE = ?1 OR Z_PK = (SELECT {notedata} FROM ZICCLOUDSYNCINGOBJECT WHERE Z_PK = ?1)"),
                None => format!("SELECT {cols} FROM ZICNOTEDATA WHERE ?1 IS NULL"),
            };
            let mut st = self.conn.prepare(&sql)?;
            let it = st.query_map([self.want.get()], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, Option<Vec<u8>>>(2)?, r.get::<_, Option<Vec<u8>>>(3)?, r.get::<_, Option<Vec<u8>>>(4)?))
            })?;
            for x in it {
                let (pk, note, data, iv, tag) = x?;
                if let Some(d) = data {
                    let d = (d, iv, tag);
                    if let Some(n) = note {
                        data_by_note.insert(n, d.clone());
                    }
                    data_by_pk.insert(pk, d);
                }
            }
        }

        // inline tokens by identifier
        let mut inline: HashMap<String, (String, String)> = HashMap::new();
        // the list's tags, by note pk (meta)
        let mut list_tags: HashMap<i64, Vec<String>> = HashMap::new();
        let mut attrs = vec!["ZIDENTIFIER", "ZALTTEXT", "ZTOKENCONTENTIDENTIFIER", "ZENCRYPTEDVALUESJSON", "ZTYPEUTI", "ZNOTE"];
        attrs.extend(CRYPTO);
        let irows = match self.want.get() {
            Some(w) => self.rows_in("ICInlineAttachment", &attrs, "ZNOTE", &[Value::Integer(w)])?,
            None => self.rows("ICInlineAttachment", &attrs)?,
        };
        for r in irows {
            if !wanted(r.i("ZNOTE")) {
                continue;
            }
            if self.opts.meta {
                if let (Some(n), Some(UTI_HASHTAG), Some(alt)) = (r.i("ZNOTE"), r.s("ZTYPEUTI").as_deref(), r.s("ZALTTEXT")) {
                    list_tags.entry(n).or_default().push(alt.trim_start_matches('#').to_string());
                }
                continue;
            }
            let Some(id) = r.s("ZIDENTIFIER") else { continue };
            if r.flag("ZISPASSWORDPROTECTED") {
                if let Some(v) = self.open_row(&r).and_then(|(k, _)| Self::open_values(&r, &k)) {
                    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
                    inline.insert(id, (s("altText"), s("tokenContentIdentifier")));
                }
            } else {
                inline.insert(id, (r.s("ZALTTEXT").unwrap_or_default(), r.s("ZTOKENCONTENTIDENTIFIER").unwrap_or_default()));
            }
        }

        // notes
        let nattrs = [
                "ZIDENTIFIER",
                "ZTITLE",
                "ZSNIPPET",
                "ZCREATIONDATE",
                "ZMODIFICATIONDATE",
                "ZFOLDER",
                "ZNOTEDATA",
                "ZNEEDSINITIALFETCHFROMCLOUD",
                "ZISPINNED",
                "ZISPASSWORDPROTECTED",
                "ZMARKEDFORDELETION",
                "ZOUTLINESTATEDATA",
                "ZACCOUNT",
                "ZPASSWORDHINT",
                "ZCRYPTOSALT",
                "ZCRYPTOITERATIONCOUNT",
                "ZCRYPTOWRAPPEDKEY",
                "ZCRYPTOVERIFIER",
                "ZTHUMBNAILATTACHMENTIDENTIFIER",
                "ZHASCHECKLIST",
                "ZHASCHECKLISTINPROGRESS",
                "ZFOLDERMODIFICATIONDATE",
        ];
        // the list needs no outline state or keys
        let light: Vec<&'static str> = nattrs.iter().copied().filter(|a| !matches!(*a, "ZOUTLINESTATEDATA" | "ZCRYPTOSALT" | "ZCRYPTOWRAPPEDKEY" | "ZCRYPTOVERIFIER")).collect();
        let nattrs: &[&'static str] = if self.opts.meta { &light } else { &nattrs };
        let mut nrows = match self.want.get() {
            Some(w) => self.rows_in("ICNote", nattrs, "Z_PK", &[Value::Integer(w)])?,
            None => self.rows("ICNote", nattrs)?,
        };
        nrows.retain(|r| wanted(r.i("Z_PK")));
        if self.opts.meta {
            // the list thumbnails: the attachment each note shows first (saved with the note)
            self.thumbs.borrow_mut().extend(nrows.iter().filter_map(|r| r.s("ZTHUMBNAILATTACHMENTIDENTIFIER")).filter(|t| !t.is_empty()));
        }
        // attachments by note pk
        let atts = self.attachments(&acct_id, &mut rep)?;
        // (meta) every note's attachment kinds, from their types alone
        let mut list_kinds: HashMap<i64, Vec<AttachmentKind>> = HashMap::new();
        if self.opts.meta {
            for r in self.rows("ICAttachment", &["ZTYPEUTI", "ZNOTE", "ZMARKEDFORDELETION"])? {
                if let (Some(n), false) = (r.i("ZNOTE"), r.flag("ZMARKEDFORDELETION")) {
                    let k = AttachmentKind::from_uti(&r.s("ZTYPEUTI").unwrap_or_default());
                    let v = list_kinds.entry(n).or_default();
                    if !v.contains(&k) {
                        v.push(k);
                    }
                }
            }
        }
        // open locked notes first, so their attachments' keys are known
        let mut opened: HashMap<i64, ([u8; 16], Lock)> = HashMap::new();
        for r in nrows.iter().filter(|r| r.flag("ZISPASSWORDPROTECTED") && !r.flag("ZMARKEDFORDELETION")) {
            if let Some((k, mut l)) = self.open_row(r) {
                l.hint = r.s("ZPASSWORDHINT").filter(|h| !h.is_empty());
                opened.insert(r.i("Z_PK").unwrap_or(0), (k, l));
            }
        }
        for r in nrows {
            if r.flag("ZMARKEDFORDELETION") {
                continue;
            }
            let pk = r.i("Z_PK").unwrap_or(0);
            let title = r.s("ZTITLE").unwrap_or_default();
            let locked = r.flag("ZISPASSWORDPROTECTED");
            let mut note = Note {
                id: r.s("ZIDENTIFIER").unwrap_or_else(|| format!("note-{pk}")),
                folder: r.i("ZFOLDER").and_then(|f| folder_id.get(&f).cloned()).unwrap_or_default(),
                title: title.clone(),
                snippet: r.s("ZSNIPPET").unwrap_or_default(),
                created: r.date("ZCREATIONDATE"),
                modified: r.date("ZMODIFICATIONDATE"),
                pinned: r.flag("ZISPINNED"),
                locked,
                deleted: r.i("ZFOLDER").is_some_and(|f| trash.contains(&f)),
                ..Note::default()
            };
            if self.opts.meta {
                note.light = true;
                note.list_tags = list_tags.remove(&pk).unwrap_or_default();
                note.list_kinds = list_kinds.remove(&pk).unwrap_or_default();
                note.list_checklist = (r.flag("ZHASCHECKLIST"), r.flag("ZHASCHECKLISTINPROGRESS"));
                if note.deleted {
                    note.trashed_at = r.f("ZFOLDERMODIFICATIONDATE").map(|d| d + APPLE_EPOCH);
                }
                if self.opts.all_attachments {
                    note.attachments = atts.get(&pk).cloned().unwrap_or_default();
                }
                let thumb = r.s("ZTHUMBNAILATTACHMENTIDENTIFIER");
                note.list_thumbnail = atts.get(&pk).and_then(|l| l.iter().find(|a| thumb.as_deref() == Some(a.id.as_str()))).and_then(|a| match a.kind {
                    AttachmentKind::Scan => a.parts.first().and_then(|p| p.display_file().cloned()).or(a.preview.clone()),
                    _ => a.preview.clone().or(a.fallback.clone()).or(a.file.clone()),
                });
                note.not_downloaded = !locked && r.flag("ZNEEDSINITIALFETCHFROMCLOUD") && r.i("ZNOTEDATA").is_none();
                lib.notes.push(note);
                continue;
            }
            if note.deleted {
                note.trashed_at = r.f("ZFOLDERMODIFICATIONDATE").map(|d| d + APPLE_EPOCH);
            }
            let data = r.i("ZNOTEDATA").and_then(|d| data_by_pk.get(&d)).or_else(|| data_by_note.get(&pk));
            let plain = match (locked, opened.get(&pk), data) {
                (false, _, Some((d, _, _))) => Some(Ok(d.clone())),
                (true, Some((k, l)), Some((d, iv, tag))) => {
                    note.lock = Some(l.clone());
                    rep.unlocked += 1;
                    Some(lock::open(k, iv.as_deref().unwrap_or_default(), tag.as_deref().unwrap_or_default(), d).ok_or(Error::Format("the note's key does not open its data".into())))
                }
                (true, None, d) => {
                    rep.locked.push(title.clone());
                    // kept encrypted, so it can be opened later with its own password
                    let mut sealed = self.sealed(pk)?;
                    sealed.crypto = Self::crypto_of(&r);
                    sealed.hint = r.s("ZPASSWORDHINT").filter(|h| !h.is_empty());
                    if let Some((d, iv, tag)) = d {
                        (sealed.data, sealed.data_iv, sealed.data_tag) = (d.clone(), iv.clone(), tag.clone());
                    }
                    // only a password's lock (PBKDF2 + a wrapped key) opens off the Mac; "use login
                    // password" notes keep their key in the Mac's keychain
                    if sealed.crypto.iterations > 0 && sealed.crypto.wrapped_key.as_ref().is_some_and(|k| k.len() == 24) {
                        note.sealed = Some(Box::new(sealed));
                    } else {
                        rep.locked_with_login_password += 1;
                    }
                    None
                }
                (false, _, None) if r.flag("ZNEEDSINITIALFETCHFROMCLOUD") => {
                    note.not_downloaded = true;
                    rep.not_downloaded += 1;
                    None
                }
                _ => None,
            };
            {
                match plain.map(|d| d.and_then(|d| body::decode_note(&d))) {
                    Some(Ok(mut b)) => {
                        if let Some(os) = r.b("ZOUTLINESTATEDATA") {
                            body::apply_outline_state(&mut b, &body::decode_outline_state(&os));
                        }
                        for t in b.tokens_mut() {
                            if let Some((alt, content)) = inline.get(&t.id) {
                                t.alt = alt.clone();
                                t.content = content.clone();
                            }
                        }
                        note.body = b;
                    }
                    Some(Err(e)) => rep.unreadable.push(format!("{title}: {e}")),
                    None => {}
                }
            }
            if let Some(list) = atts.get(&pk) {
                for a in list.iter().filter(|_| !locked || note.lock.is_some()) {
                    // a scan's content is its pages; a drawing read as strokes needs no picture
                    let shown = a.display_file().is_some() || a.parts.iter().any(|p| p.display_file().is_some()) || a.drawing.is_some();
                    if !shown && a.table.is_none() && a.kind != AttachmentKind::Url {
                        rep.missing_files.push(format!("{title}: {}", if a.name.is_empty() { &a.id } else { &a.name }));
                    }
                }
                note.attachments = list.clone();
            }
            lib.notes.push(note);
        }

        rep.accounts = lib.accounts.len();
        rep.folders = lib.folders.len();
        rep.notes = lib.notes.len();
        rep.attachments = lib.notes.iter().map(|n| n.attachments.len()).sum();
        Ok((lib, rep))
    }

    /// Every block attachment, by its note's pk (sub-attachments of scans nested in their gallery).
    fn attachments(&self, acct_id: &HashMap<i64, String>, rep: &mut Report) -> Result<HashMap<i64, Vec<Attachment>>> {
        let mut attrs = vec![
                "ZIDENTIFIER",
                "ZTYPEUTI",
                "ZNOTE",
                "ZMEDIA",
                "ZSIZEWIDTH",
                "ZSIZEHEIGHT",
                "ZSUMMARY",
                "ZMERGEABLEDATA",
                "ZURLSTRING",
                "ZTITLE",
                "ZUSERTITLE",
                "ZFALLBACKIMAGEGENERATION",
                "ZFALLBACKPDFGENERATION",
                "ZCREATIONDATE",
                "ZMODIFICATIONDATE",
                "ZMARKEDFORDELETION",
                "ZPARENTATTACHMENT",
                "ZACCOUNT",
                "ZENCRYPTEDVALUESJSON",
                "ZFALLBACKIMAGECRYPTOINITIALIZATIONVECTOR",
                "ZFALLBACKIMAGECRYPTOTAG",
                "ZFALLBACKPDFCRYPTOINITIALIZATIONVECTOR",
                "ZFALLBACKPDFCRYPTOTAG",
                "ZMERGEABLEPREFERREDVIEWSIZE",
                "ZNEEDSINITIALFETCHFROMCLOUD",
        ];
        attrs.extend(CRYPTO);
        if self.opts.meta {
            attrs.retain(|a| *a != "ZMERGEABLEDATA");
        }
        // one note's (or, for the list, the thumbnails'), with their scan pages; else all
        let top = if let Some(w) = self.want.get() {
            Some(self.rows_in("ICAttachment", &attrs, "ZNOTE", &[Value::Integer(w)])?)
        } else if self.opts.meta && !self.opts.all_attachments {
            let ids: Vec<Value> = self.thumbs.borrow().iter().map(|t| Value::Text(t.clone())).collect();
            Some(self.rows_in("ICAttachment", &attrs, "ZIDENTIFIER", &ids)?)
        } else {
            None
        };
        let rows = match top {
            Some(mut top) => {
                let parents: Vec<Value> = top.iter().filter_map(|r| r.i("Z_PK")).map(Value::Integer).collect();
                top.extend(self.rows_in("ICAttachment", &attrs, "ZPARENTATTACHMENT", &parents)?);
                top
            }
            None => self.rows("ICAttachment", &attrs)?,
        };
        let filtered = self.want.get().is_some() || self.opts.meta;
        // media files
        // (identifier, file name, generation, account, key + asset IV/tag of an encrypted file)
        type Sealed = Option<([u8; 16], Vec<u8>, Vec<u8>)>;
        let mut media: HashMap<i64, (String, String, Option<String>, Option<i64>, Sealed)> = HashMap::new();
        let mut attrs: Vec<&'static str> = vec!["ZIDENTIFIER", "ZFILENAME", "ZGENERATION", "ZACCOUNT", "ZENCRYPTEDVALUESJSON", "ZASSETCRYPTOINITIALIZATIONVECTOR", "ZASSETCRYPTOTAG"];
        attrs.extend(CRYPTO);
        let mrows = if filtered {
            let ids: Vec<Value> = rows.iter().filter_map(|r| r.i("ZMEDIA")).map(Value::Integer).collect();
            self.rows_in("ICMedia", &attrs, "Z_PK", &ids)?
        } else {
            self.rows("ICMedia", &attrs)?
        };
        for r in mrows {
            let mut name = r.s("ZFILENAME").unwrap_or_default();
            let mut sealed = None;
            if r.flag("ZISPASSWORDPROTECTED") {
                let Some((k, _)) = self.open_row(&r) else { continue };
                if let Some(n) = Self::open_values(&r, &k).and_then(|v| v.get("filename").and_then(|f| f.as_str()).map(str::to_string)) {
                    name = n;
                }
                sealed = Some((k, r.b("ZASSETCRYPTOINITIALIZATIONVECTOR").unwrap_or_default(), r.b("ZASSETCRYPTOTAG").unwrap_or_default()));
            }
            media.insert(r.i("Z_PK").unwrap_or(0), (r.s("ZIDENTIFIER").unwrap_or_default(), name, r.s("ZGENERATION"), r.i("ZACCOUNT"), sealed));
        }
        // previews by attachment pk: (pixel width, appearance, identifier, generation)
        let mut previews: HashMap<i64, Vec<(f64, i64, String, Option<String>)>> = HashMap::new();
        let pattrs = ["ZIDENTIFIER", "ZGENERATION", "ZATTACHMENT", "ZWIDTH", "ZSCALE", "ZAPPEARANCETYPE"];
        let prows = if filtered {
            let ids: Vec<Value> = rows.iter().filter_map(|r| r.i("Z_PK")).map(Value::Integer).collect();
            self.rows_in("ICAttachmentPreviewImage", &pattrs, "ZATTACHMENT", &ids)?
        } else {
            self.rows("ICAttachmentPreviewImage", &pattrs)?
        };
        for r in prows {
            if let (Some(a), Some(id)) = (r.i("ZATTACHMENT"), r.s("ZIDENTIFIER")) {
                let w = r.f("ZWIDTH").unwrap_or(0.0) * r.f("ZSCALE").unwrap_or(1.0);
                previews.entry(a).or_default().push((w, r.i("ZAPPEARANCETYPE").unwrap_or(0), id, r.s("ZGENERATION")));
            }
        }

        let mut by_pk: HashMap<i64, (Option<i64>, Option<i64>, Attachment)> = HashMap::new();
        let mut order = Vec::new();
        for r in rows {
            if r.flag("ZMARKEDFORDELETION") {
                continue;
            }
            let pk = r.i("Z_PK").unwrap_or(0);
            let id = r.s("ZIDENTIFIER").unwrap_or_default();
            let uti = r.s("ZTYPEUTI").unwrap_or_default();
            let kind = AttachmentKind::from_uti(&uti);
            let acct = r.i("ZACCOUNT").and_then(|a| acct_id.get(&a)).cloned();
            let mut a = Attachment {
                kind,
                width: r.f("ZSIZEWIDTH").unwrap_or(0.0),
                height: r.f("ZSIZEHEIGHT").unwrap_or(0.0),
                url: r.s("ZURLSTRING").filter(|s| !s.is_empty()),
                summary: r.s("ZSUMMARY").unwrap_or_default(),
                created: r.date("ZCREATIONDATE"),
                modified: r.date("ZMODIFICATIONDATE"),
                name: r.s("ZUSERTITLE").or_else(|| r.s("ZTITLE")).unwrap_or_default(),
                ..Attachment::default()
            };
            // a locked note's attachment: its values and files are encrypted
            let sealed = r.flag("ZISPASSWORDPROTECTED");
            let key = if sealed { self.open_row(&r).map(|(k, _)| k) } else { None };
            let values = key.and_then(|k| Self::open_values(&r, &k));
            let value = |k: &str| values.as_ref().and_then(|v| v.get(k)).and_then(|x| x.as_str()).map(str::to_string);
            if sealed {
                a.summary = value("summary").unwrap_or_default();
                a.url = value("urlString").or(a.url);
                if let Some(t) = value("title") {
                    a.name = t;
                }
            }
            if let Some((mid, fname, generation, macct, msealed)) = r.i("ZMEDIA").and_then(|m| media.get(&m)) {
                let macct = macct.and_then(|x| acct_id.get(&x)).cloned().or(acct.clone());
                match msealed {
                    None => a.file = self.media_path(macct.as_deref(), mid, generation.as_deref(), fname),
                    Some((k, iv, tag)) => {
                        a.file = self
                            .media_path(macct.as_deref(), mid, generation.as_deref(), mid)
                            .and_then(|p| std::fs::read(p).ok())
                            .and_then(|b| lock::open(k, iv, tag, &b))
                            .and_then(|b| lock::write_unlocked(mid, if fname.is_empty() { mid } else { fname }, &b).ok());
                    }
                }
                if !fname.is_empty() {
                    a.name = fname.clone();
                }
            }
            if sealed {
                if let Some(k) = &key {
                    a.fallback = self.sealed_fallback(acct.as_deref(), &id, &r, k);
                }
            } else {
                a.fallback = self.fallback_path(acct.as_deref(), &id, r.s("ZFALLBACKIMAGEGENERATION").as_deref(), r.s("ZFALLBACKPDFGENERATION").as_deref());
            }
            if let Some(list) = previews.get(&pk).filter(|_| !sealed) {
                let mut list = list.clone();
                // light previews first, biggest first
                list.sort_by(|x, y| x.1.cmp(&y.1).then(y.0.total_cmp(&x.0)));
                a.preview = list.iter().find_map(|(_, _, pid, generation)| self.preview_path(acct.as_deref(), pid, generation.as_deref()));
            }
            let mergeable = if sealed { value("mergeableData").and_then(|b| lock::unbase64(&b)) } else { r.b("ZMERGEABLEDATA") };
            if kind == AttachmentKind::Drawing && !self.opts.meta {
                // ours, unless the PaperKit bundle changed since we wrote it (or was never ours)
                let ours = mergeable.as_deref().and_then(crate::paper::decode_ink);
                let bundle = if sealed { None } else { self.bases(acct.as_deref()).into_iter().map(|b| crate::paperkit::bundle_dir(&b, &id)).find(|b| b.is_dir()) };
                a.drawing = match bundle {
                    Some(b) => {
                        let root = mergeable.as_deref().and_then(crate::paper::ink_bundle_root);
                        if ours.is_some() && root.is_some() && root == crate::paperkit::fingerprint(&b) {
                            ours
                        } else {
                            match crate::paperkit::read(&b) {
                                Some(d) => Some(d),
                                None => {
                                    rep.unreadable.push(format!("drawing {id}: unreadable PaperKit bundle"));
                                    ours
                                }
                            }
                        }
                    }
                    None => ours,
                };
                if uti == UTI_PAPER && a.drawing.is_none() && a.display_file().is_none() {
                    // its bundle (and picture) never arrived
                    a.not_downloaded = true;
                }
                // legacy PencilKit drawings keep their PKDrawing data in the row
                if a.drawing.is_none() && uti.starts_with("com.apple.drawing") {
                    a.drawing = mergeable.as_deref().and_then(crate::paperkit::decode_legacy).map(|mut d| {
                        if a.width > 0.0 && a.height > 0.0 {
                            (d.width, d.height) = (a.width as f32, a.height as f32);
                        }
                        d
                    });
                }
            }
            if let Some(v) = r.b("ZMERGEABLEPREFERREDVIEWSIZE") {
                a.view_size = crate::paperkit::view_size_of(&v);
            }
            // a PaperKit document (PDF, scan): its pages and original PDF are in its bundle
            if kind == AttachmentKind::Pdf && uti.starts_with("com.apple.paper.doc") && !sealed && !self.opts.meta {
                let doc = self.bases(acct.as_deref()).into_iter().map(|b| crate::paperkit::bundle_dir(&b, &id)).find(|b| b.is_dir()).and_then(|b| crate::paperkit::read_document(&b));
                if let Some(doc) = doc {
                    a.pages = doc.pages;
                    if a.file.is_none() {
                        a.file = doc.file;
                    }
                }
                if a.file.is_none() {
                    // no original here: the Quartz copy Notes keeps for other devices
                    a.file = a.fallback.clone().filter(|f| f.extension().is_some_and(|e| e == "pdf"));
                }
                if Path::new(&a.name).extension().is_none() && !a.name.is_empty() {
                    a.name.push_str(".pdf");
                }
            }
            if r.flag("ZNEEDSINITIALFETCHFROMCLOUD") && a.display_file().is_none() && mergeable.is_none() {
                a.not_downloaded = true;
                rep.attachments_not_downloaded += 1;
            }
            if kind == AttachmentKind::Table && !self.opts.meta {
                match mergeable.map(|d| table::decode_table(&d)) {
                    Some(Ok(t)) => a.table = Some(t),
                    Some(Err(e)) => rep.unreadable.push(format!("table {id}: {e}")),
                    None => {}
                }
            }
            a.id = id;
            a.uti = uti;
            order.push(pk);
            by_pk.insert(pk, (r.i("ZNOTE"), r.i("ZPARENTATTACHMENT"), a));
        }
        // scans: pages go into their gallery's parts
        let children: Vec<(i64, i64)> = order.iter().filter_map(|pk| by_pk[pk].1.filter(|p| by_pk.contains_key(p)).map(|p| (*pk, p))).collect();
        for (child, parent) in &children {
            let c = by_pk[child].2.clone();
            by_pk.get_mut(parent).unwrap().2.parts.push(c);
        }
        let mut out: HashMap<i64, Vec<Attachment>> = HashMap::new();
        for pk in order {
            if children.iter().any(|(c, _)| *c == pk) {
                continue;
            }
            let (note, _, a) = by_pk.remove(&pk).unwrap();
            if let Some(n) = note {
                out.entry(n).or_default().push(a);
            }
        }
        Ok(out)
    }

    /// Account folders to look in: Accounts/<id>/ first, then the root (older stores), then the
    /// other accounts (some rows, e.g. scans and PDF documents, name no account).
    fn bases(&self, acct: Option<&str>) -> Vec<PathBuf> {
        let mut v = Vec::new();
        if let Some(a) = acct {
            v.push(self.root.join("Accounts").join(a));
        }
        v.push(self.root.clone());
        let mut others: Vec<PathBuf> = std::fs::read_dir(self.root.join("Accounts")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir() && !v.contains(p)).collect();
        others.sort();
        v.extend(others);
        v
    }

    fn first_existing(cands: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
        cands.into_iter().find(|p| p.is_file())
    }

    fn media_path(&self, acct: Option<&str>, id: &str, generation: Option<&str>, name: &str) -> Option<PathBuf> {
        if id.is_empty() {
            return None;
        }
        let mut c = Vec::new();
        for b in self.bases(acct) {
            let dir = b.join("Media").join(id);
            if let Some(g) = generation {
                c.push(dir.join(g).join(name));
            }
            c.push(dir.join(name));
        }
        Self::first_existing(c).or_else(|| {
            // any file under Media/<id>/ (generation folder unknown)
            self.bases(acct).into_iter().find_map(|b| find_file(&b.join("Media").join(id), 2))
        })
    }

    fn fallback_path(&self, acct: Option<&str>, id: &str, img_gen: Option<&str>, pdf_gen: Option<&str>) -> Option<PathBuf> {
        if id.is_empty() {
            return None;
        }
        let mut c = Vec::new();
        for b in self.bases(acct) {
            if let Some(g) = img_gen {
                c.push(b.join("FallbackImages").join(id).join(g).join("FallbackImage.png"));
                c.push(b.join("FallbackImages").join(id).join(g).join("FallbackImage.jpg"));
            }
            if let Some(g) = pdf_gen {
                c.push(b.join("FallbackPDFs").join(id).join(g).join("FallbackPDF.pdf"));
            }
            c.push(b.join("FallbackImages").join(format!("{id}.png")));
            c.push(b.join("FallbackImages").join(format!("{id}.jpg")));
            c.push(b.join("FallbackPDFs").join(format!("{id}.pdf")));
        }
        Self::first_existing(c)
            .or_else(|| self.bases(acct).into_iter().find_map(|b| find_file(&b.join("FallbackImages").join(id), 2)))
            .or_else(|| self.bases(acct).into_iter().find_map(|b| find_file(&b.join("FallbackPDFs").join(id), 2)))
    }

    /// A locked note's drawing/scan fallback (`FallbackImage.png.encrypted`), decrypted.
    fn sealed_fallback(&self, acct: Option<&str>, id: &str, r: &Row, key: &[u8; 16]) -> Option<PathBuf> {
        for (folder, file, generation, iv, tag) in [
            ("FallbackImages", "FallbackImage.png", "ZFALLBACKIMAGEGENERATION", "ZFALLBACKIMAGECRYPTOINITIALIZATIONVECTOR", "ZFALLBACKIMAGECRYPTOTAG"),
            ("FallbackImages", "FallbackImage.jpeg", "ZFALLBACKIMAGEGENERATION", "ZFALLBACKIMAGECRYPTOINITIALIZATIONVECTOR", "ZFALLBACKIMAGECRYPTOTAG"),
            ("FallbackPDFs", "FallbackPDF.pdf", "ZFALLBACKPDFGENERATION", "ZFALLBACKPDFCRYPTOINITIALIZATIONVECTOR", "ZFALLBACKPDFCRYPTOTAG"),
        ] {
            let (Some(iv), Some(tag)) = (r.b(iv), r.b(tag)) else { continue };
            let ext = file.rsplit('.').next().unwrap_or("png");
            let mut c = Vec::new();
            for b in self.bases(acct) {
                if let Some(g) = r.s(generation) {
                    c.push(b.join(folder).join(id).join(g).join(format!("{file}.encrypted")));
                }
                c.push(b.join(folder).join(format!("{id}.{ext}.encrypted")));
            }
            if let Some(b) = Self::first_existing(c).and_then(|p| std::fs::read(p).ok()) {
                if let Some(plain) = lock::open(key, &iv, &tag, &b) {
                    return lock::write_unlocked(id, file, &plain).ok();
                }
            }
        }
        None
    }

    fn preview_path(&self, acct: Option<&str>, id: &str, generation: Option<&str>) -> Option<PathBuf> {
        let mut c = Vec::new();
        for b in self.bases(acct) {
            if let Some(g) = generation {
                c.push(b.join("Previews").join(id).join(g).join("Preview.png"));
                c.push(b.join("Previews").join(id).join(g).join("OrientedPreview.png"));
            }
            c.push(b.join("Previews").join(format!("{id}.png")));
            c.push(b.join("Previews").join(format!("{id}.jpeg")));
        }
        Self::first_existing(c)
    }
}

/// The first regular file under `dir`, at most `depth` folders down.
fn find_file(dir: &Path, depth: u32) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in &entries {
        if p.is_file() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            return Some(p.clone());
        }
    }
    if depth > 0 {
        for p in entries {
            if p.is_dir() {
                if let Some(f) = find_file(&p, depth - 1) {
                    return Some(f);
                }
            }
        }
    }
    None
}
