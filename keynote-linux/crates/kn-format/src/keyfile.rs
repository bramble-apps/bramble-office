//! A whole `.key` package: ZIP (or folder bundle) of `Index/*.iwa`, `Data/*`, `Metadata/*`, previews.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::iwa::{Archive, IwaFile};
use crate::msg::{Msg, ObjId};
use crate::schema::short_name;
use crate::Error;

#[derive(Debug, Clone)]
pub enum EntryData {
    Iwa(IwaFile),
    /// Shared so that whole-file undo snapshots don't copy every image.
    Raw(std::sync::Arc<Vec<u8>>),
    /// Large media left in the source file and read on demand (keeps big decks out of RAM):
    /// (source, index of the entry in the source zip).
    Lazy(Arc<LazySource>, usize),
}

/// The opened source `.key`, kept open so lazy entries stay readable even after the path is
/// replaced by a save (the open handle still refers to the original file).
pub struct LazySource {
    pub path: PathBuf,
    zip: Mutex<zip::ZipArchive<std::fs::File>>,
}

impl std::fmt::Debug for LazySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LazySource({})", self.path.display())
    }
}

impl LazySource {
    fn read(&self, index: usize) -> Result<Vec<u8>, Error> {
        let mut z = self.zip.lock().map_err(|_| Error::Invalid("zip lock poisoned".into()))?;
        let mut f = z.by_index(index)?;
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// Stream an entry into a writer without holding it all in memory.
    fn copy_to(&self, index: usize, w: &mut impl Write) -> Result<(), Error> {
        let mut z = self.zip.lock().map_err(|_| Error::Invalid("zip lock poisoned".into()))?;
        let mut f = z.by_index(index)?;
        std::io::copy(&mut f, w)?;
        Ok(())
    }
}

/// Entry name as UTF-8: Keynote writes non-ASCII names in UTF-8 without setting the ZIP
/// UTF-8 flag, so the zip crate's CP437 fallback would garble them.
fn entry_name(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => raw.iter().map(|&b| b as char).collect(),
    }
}

/// Media larger than this stay in the file until needed.
const LAZY_MIN: u64 = 256 * 1024;

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub data: EntryData,
}

#[derive(Debug, Clone)]
pub struct KeyFile {
    pub entries: Vec<Entry>,
    /// object id -> (entry index, archive index)
    index: HashMap<ObjId, (usize, usize)>,
}

impl KeyFile {
    pub fn open(path: impl AsRef<Path>) -> Result<KeyFile, Error> {
        let path = path.as_ref();
        if path.is_dir() {
            let mut files = Vec::new();
            collect_dir(path, path, &mut files)?;
            // a Keynote package keeps its Index/ files in Index.zip
            if let Some(pos) = files.iter().position(|f| f.0 == "Index.zip") {
                let (_, zip) = files.remove(pos);
                let mut ar = zip::ZipArchive::new(std::io::Cursor::new(zip))?;
                for i in 0..ar.len() {
                    let mut f = ar.by_index(i)?;
                    if f.is_dir() {
                        continue;
                    }
                    let name = f.name()?.to_string();
                    let mut b = Vec::new();
                    std::io::Read::read_to_end(&mut f, &mut b)?;
                    files.push((name, b));
                }
            }
            files.sort_by(|a, b| a.0.cmp(&b.0));
            return KeyFile::from_files(files);
        }
        // on a network / cloud mount every picture read would go back over the network
        // (rclone without a read cache re-downloads each one, and can stall): copy the file
        // here in one sequential read first, then open the copy; saves still go to `path`
        if is_remote(path) {
            match local_copy(path) {
                Ok(local) => {
                    crate::progress::span(0.85, 1.0);
                    let r = KeyFile::open_lazy(&local);
                    crate::progress::span(0.0, 1.0);
                    return r;
                }
                Err(Error::Invalid(m)) if m == "cancelled" => return Err(Error::Invalid(m)),
                Err(_) => crate::progress::span(0.0, 1.0),
            }
        }
        KeyFile::open_lazy(path)
    }

    /// Open a single-file .key reading index files now and leaving big Data/ files on disk.
    fn open_lazy(path: &Path) -> Result<KeyFile, Error> {
        let file = std::fs::File::open(path)?;
        let mut z = zip::ZipArchive::new(file)?;
        if z.index_for_name(crate::crypto::VERIFIER).is_some() {
            let hint = z
                .by_name(crate::crypto::HINT)
                .ok()
                .and_then(|mut f| {
                    let mut s = String::new();
                    f.read_to_string(&mut s).ok().map(|_| s)
                })
                .unwrap_or_default();
            return Err(Error::NeedsPassword(hint));
        }
        let mut eager: Vec<(String, Vec<u8>)> = Vec::new();
        let mut lazy_names: Vec<(usize, String, usize)> = Vec::new();
        let mut order: Vec<String> = Vec::new();
        // progress: reading the index files is the first half, parsing them the second
        let total: u64 = (0..z.len()).filter_map(|i| z.by_index_raw(i).ok().filter(|f| !(f.name().is_ok_and(|n| n.starts_with("Data/")) && f.size() >= LAZY_MIN)).map(|f| f.compressed_size())).sum::<u64>().max(1);
        let mut done = 0u64;
        for i in 0..z.len() {
            let mut f = z.by_index(i)?;
            if f.is_dir() {
                continue;
            }
            let name = entry_name(f.name_raw());
            order.push(name.clone());
            if name.starts_with("Data/") && f.size() >= LAZY_MIN {
                lazy_names.push((order.len() - 1, name, i));
                continue;
            }
            done += f.compressed_size();
            let mut buf = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut buf)?;
            eager.push((name, buf));
            if !crate::progress::report(0.5 * done as f32 / total as f32) {
                return Err(Error::Invalid("cancelled".into()));
            }
        }
        let source = Arc::new(LazySource { path: path.to_path_buf(), zip: Mutex::new(z) });
        let mut k = KeyFile::from_files(eager)?;
        // put lazy entries back in their original order
        let mut entries: Vec<Entry> = Vec::with_capacity(order.len());
        let mut eager_iter = std::mem::take(&mut k.entries).into_iter();
        let mut lazy_iter = lazy_names.into_iter().peekable();
        for (i, _) in order.iter().enumerate() {
            if lazy_iter.peek().is_some_and(|(li, _, _)| *li == i) {
                let (_, name, zi) = lazy_iter.next().unwrap();
                entries.push(Entry { name, data: EntryData::Lazy(source.clone(), zi) });
            } else if let Some(e) = eager_iter.next() {
                entries.push(e);
            }
        }
        k.entries = entries;
        k.reindex();
        Ok(k)
    }

    pub fn from_zip(bytes: &[u8]) -> Result<KeyFile, Error> {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes))?;
        let mut files = Vec::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i)?;
            if f.is_dir() {
                continue;
            }
            let name = entry_name(f.name_raw());
            let mut buf = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut buf)?;
            files.push((name, buf));
        }
        KeyFile::from_files(files)
    }

    pub fn from_files(files: Vec<(String, Vec<u8>)>) -> Result<KeyFile, Error> {
        let mut entries = Vec::with_capacity(files.len());
        let total: usize = files.iter().map(|f| f.1.len()).sum::<usize>().max(1);
        let mut done = 0usize;
        let mut lzfse = false;
        for (name, raw) in files {
            done += raw.len();
            lzfse |= name.ends_with(".iwa") && crate::wire::is_lzfse(&raw);
            if !crate::progress::report(0.5 + 0.45 * done as f32 / total as f32) {
                return Err(Error::Invalid("cancelled".into()));
            }
            let data = if name.ends_with(".iwa") {
                EntryData::Iwa(IwaFile::parse(&raw).map_err(|e| Error::InFile(name.clone(), Box::new(e)))?)
            } else {
                EntryData::Raw(std::sync::Arc::new(raw))
            };
            entries.push(Entry { name, data });
        }
        let mut k = KeyFile { entries, index: HashMap::new() };
        k.reindex();
        // LZFSE parts were decoded, and every part is written back with Snappy: their marks go
        if lzfse {
            if let Some(pm) = k.package_metadata_mut() {
                for list in ["components", "versioned_components"] {
                    for c in pm.msgs_mut(list) {
                        if c.has("compression_algorithm") {
                            c.remove("compression_algorithm");
                        }
                    }
                }
            }
        }
        Ok(k)
    }

    /// Rebuild the id index; call after adding/removing archives or entries.
    pub fn reindex(&mut self) {
        self.index.clear();
        for (ei, e) in self.entries.iter().enumerate() {
            if let EntryData::Iwa(iwa) = &e.data {
                for (ai, a) in iwa.archives.iter().enumerate() {
                    self.index.entry(a.id()).or_insert((ei, ai));
                }
            }
        }
    }

    pub fn to_zip(&self) -> Result<Vec<u8>, Error> {
        Ok(self.write_zip(Cursor::new(Vec::new()))?.into_inner())
    }

    fn write_zip<W: Write + Seek>(&self, out: W) -> Result<W, Error> {
        let mut w = zip::ZipWriter::new(out);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .large_file(false)
            .last_modified_time(zip::DateTime::from_date_and_time(2026, 1, 1, 0, 0, 0).unwrap());
        for e in &self.entries {
            w.start_file(e.name.as_str(), opts)?;
            match &e.data {
                EntryData::Iwa(iwa) => w.write_all(&iwa.encode())?,
                EntryData::Raw(b) => w.write_all(b)?,
                EntryData::Lazy(src, i) => src.copy_to(*i, &mut w)?,
            }
        }
        Ok(w.finish()?)
    }

    /// Open a password-protected presentation (wrong password: Error::NeedsPassword).
    pub fn open_with_password(path: impl AsRef<Path>, password: &str) -> Result<KeyFile, Error> {
        let mut z = zip::ZipArchive::new(std::fs::File::open(path.as_ref())?)?;
        let mut verifier = Vec::new();
        z.by_name(crate::crypto::VERIFIER)?.read_to_end(&mut verifier)?;
        let key = crate::crypto::check(&verifier, password).ok_or_else(|| Error::NeedsPassword(String::new()))?;
        let mut files = Vec::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i)?;
            if f.is_dir() {
                continue;
            }
            let name = entry_name(f.name_raw());
            if name == crate::crypto::VERIFIER || name == crate::crypto::HINT {
                continue;
            }
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            if name.starts_with("Index/") || name.starts_with("Data/") {
                buf = crate::crypto::decrypt(&key, &buf).ok_or(Error::Corrupt("can't decrypt"))?;
            }
            files.push((name, buf));
        }
        KeyFile::from_files(files)
    }

    /// Save password protected: Index/ and Data/ encrypted, with the verifier and hint.
    pub fn save_encrypted(&self, path: impl AsRef<Path>, password: &str, hint: &str) -> Result<(), Error> {
        let path = path.as_ref();
        let (verifier, key) = crate::crypto::make_verifier(password);
        let tmp = path.with_extension("key.tmp");
        let file = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        let mut w = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(false);
        w.start_file(crate::crypto::VERIFIER, opts)?;
        w.write_all(&verifier)?;
        w.start_file(crate::crypto::HINT, opts)?;
        w.write_all(hint.as_bytes())?;
        for e in &self.entries {
            let bytes: Vec<u8> = match &e.data {
                EntryData::Iwa(iwa) => iwa.encode(),
                EntryData::Raw(b) => b.to_vec(),
                EntryData::Lazy(..) => self.entry_bytes(e).map(|c| c.into_owned()).unwrap_or_default(),
            };
            // the unencrypted previews would show the content
            if e.name.starts_with("preview") || e.name.ends_with("thumbnail.jpg") {
                continue;
            }
            let out = if e.name.starts_with("Index/") || e.name.starts_with("Data/") { crate::crypto::encrypt(&key, &bytes) } else { bytes };
            w.start_file(e.name.as_str(), opts)?;
            w.write_all(&out)?;
        }
        w.finish()?.flush()?;
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Save as a package (File > Advanced > Change File Type > Package): a folder with the
    /// Index/ files zipped in Index.zip and everything else (Data/, Metadata/, previews) as files.
    pub fn save_package(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        let tmp = path.with_extension("key.tmp-package");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp)?;
        let mut index = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for e in &self.entries {
            let bytes: Vec<u8> = match &e.data {
                EntryData::Iwa(iwa) => iwa.encode(),
                EntryData::Raw(b) => b.to_vec(),
                EntryData::Lazy(..) => self.entry_bytes(e).map(|c| c.into_owned()).unwrap_or_default(),
            };
            if e.name.starts_with("Index/") {
                index.start_file(e.name.as_str(), opts)?;
                index.write_all(&bytes)?;
            } else {
                let p = tmp.join(&e.name);
                if let Some(d) = p.parent() {
                    std::fs::create_dir_all(d)?;
                }
                std::fs::write(p, bytes)?;
            }
        }
        let zip = index.finish()?.into_inner();
        std::fs::write(tmp.join("Index.zip"), zip)?;
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else if path.exists() {
            std::fs::remove_file(path)?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Save as a single-file `.key`, streamed to a temp file then renamed into place (atomic).
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        let tmp = path.with_extension("key.tmp");
        let file = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        let result = self.write_zip(file).and_then(|mut w| w.flush().map_err(Error::from));
        if let Err(e) = result {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        // a package becoming a single file
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Bytes of a non-index entry (reads lazy entries from the source file).
    pub fn entry_bytes<'a>(&self, e: &'a Entry) -> Option<Cow<'a, [u8]>> {
        match &e.data {
            EntryData::Raw(b) => Some(Cow::Borrowed(b.as_slice())),
            EntryData::Lazy(src, i) => src.read(*i).ok().map(Cow::Owned),
            EntryData::Iwa(_) => None,
        }
    }

    // ------------------------------------------------------------------ lookup

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    pub fn iwa(&self, name: &str) -> Option<&IwaFile> {
        match &self.entry(name)?.data {
            EntryData::Iwa(i) => Some(i),
            _ => None,
        }
    }

    pub fn iwa_mut(&mut self, name: &str) -> Option<&mut IwaFile> {
        match &mut self.entries.iter_mut().find(|e| e.name == name)?.data {
            EntryData::Iwa(i) => Some(i),
            _ => None,
        }
    }

    pub fn raw(&self, name: &str) -> Option<Cow<'_, [u8]>> {
        self.entry_bytes(self.entry(name)?)
    }

    pub fn archive(&self, id: ObjId) -> Option<&Archive> {
        let (e, a) = *self.index.get(&id)?;
        match &self.entries[e].data {
            EntryData::Iwa(iwa) => iwa.archives.get(a),
            _ => None,
        }
    }

    pub fn archive_mut(&mut self, id: ObjId) -> Option<&mut Archive> {
        let (e, a) = *self.index.get(&id)?;
        match &mut self.entries[e].data {
            EntryData::Iwa(iwa) => iwa.archives.get_mut(a),
            _ => None,
        }
    }

    /// The object (first payload) with this id.
    pub fn obj(&self, id: ObjId) -> Option<&Msg> {
        self.archive(id).map(|a| a.obj())
    }

    pub fn obj_mut(&mut self, id: ObjId) -> Option<&mut Msg> {
        self.archive_mut(id).map(|a| a.obj_mut())
    }

    pub fn type_name(&self, id: ObjId) -> &'static str {
        self.obj(id).and_then(|m| m.ty).map(short_name).unwrap_or("?")
    }

    /// Name of the .iwa file holding the object.
    pub fn file_of(&self, id: ObjId) -> Option<&str> {
        let (e, _) = *self.index.get(&id)?;
        Some(&self.entries[e].name)
    }

    pub fn archives(&self) -> impl Iterator<Item = (&str, &Archive)> {
        self.entries.iter().flat_map(|e| match &e.data {
            EntryData::Iwa(iwa) => iwa.archives.iter().map(move |a| (e.name.as_str(), a)).collect::<Vec<_>>(),
            _ => Vec::new(),
        })
    }

    /// All objects of a type, e.g. `objects_of("KN.ShowArchive")`.
    pub fn objects_of(&self, type_name: &str) -> impl Iterator<Item = (ObjId, &Msg)> + '_ {
        let ty = crate::schema::schema().type_idx(type_name);
        self.archives()
            .filter(move |(_, a)| ty.is_some() && a.obj().ty == ty)
            .map(|(_, a)| (a.id(), a.obj()))
    }

    pub fn first_of(&self, type_name: &str) -> Option<(ObjId, &Msg)> {
        self.objects_of(type_name).next()
    }

    // ------------------------------------------------------------------ package metadata

    pub fn package_metadata(&self) -> Option<&Msg> {
        self.first_of("TSP.PackageMetadata").map(|(_, m)| m)
    }

    pub fn package_metadata_mut(&mut self) -> Option<&mut Msg> {
        let id = self.first_of("TSP.PackageMetadata")?.0;
        self.obj_mut(id)
    }

    /// Highest object identifier handed out so far.
    pub fn last_object_identifier(&self) -> ObjId {
        self.package_metadata().and_then(|pm| pm.u64("last_object_identifier")).unwrap_or(0)
    }

    /// Take an archive out of its .iwa file (returns the file name and the archive).
    pub fn remove_archive(&mut self, id: ObjId) -> Option<(String, Archive)> {
        let (e, a) = *self.index.get(&id)?;
        let name = self.entries[e].name.clone();
        let EntryData::Iwa(iwa) = &mut self.entries[e].data else { return None };
        let arch = iwa.archives.remove(a);
        self.reindex();
        Some((name, arch))
    }

    /// An archive's place in its .iwa file.
    pub fn archive_position(&self, id: ObjId) -> Option<usize> {
        self.index.get(&id).map(|(_, a)| *a)
    }

    /// Put an archive back at `pos` of an .iwa file (undo of a removal keeps the file's order).
    pub fn insert_archive_at(&mut self, file: &str, pos: usize, a: Archive) -> bool {
        let Some(e) = self.entries.iter().position(|x| x.name == file) else { return false };
        let EntryData::Iwa(iwa) = &mut self.entries[e].data else { return false };
        let at = pos.min(iwa.archives.len());
        iwa.archives.insert(at, a);
        // only this file's later archives moved (a full reindex is slow in a large document)
        for (ai, a) in iwa.archives.iter().enumerate().skip(at) {
            match self.index.entry(a.id()) {
                // (the first copy of an id wins, as reindex has it)
                std::collections::hash_map::Entry::Occupied(mut o) => {
                    if o.get().0 > e || (o.get().0 == e && o.get().1 >= at) {
                        o.insert((e, ai));
                    }
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert((e, ai));
                }
            }
        }
        true
    }

    /// Append an archive to an .iwa file.
    pub fn insert_archive(&mut self, file: &str, a: Archive) -> bool {
        let Some(e) = self.entries.iter().position(|x| x.name == file) else { return false };
        let EntryData::Iwa(iwa) = &mut self.entries[e].data else { return false };
        let id = a.id();
        iwa.archives.push(a);
        // (appended: nothing else moved)
        let at = (e, iwa.archives.len() - 1);
        let slot = self.index.entry(id).or_insert(at);
        if slot.0 > e {
            *slot = at;
        }
        true
    }

    pub fn remove_entry(&mut self, name: &str) -> Option<Entry> {
        let pos = self.entries.iter().position(|e| e.name == name)?;
        let e = self.entries.remove(pos);
        self.reindex();
        Some(e)
    }

    pub fn add_entry(&mut self, e: Entry) {
        self.entries.push(e);
        let ei = self.entries.len() - 1;
        if let EntryData::Iwa(iwa) = &self.entries[ei].data {
            for (ai, a) in iwa.archives.iter().enumerate() {
                self.index.entry(a.id()).or_insert((ei, ai));
            }
        }
    }

    /// Reserve `n` new object identifiers.
    pub fn alloc_ids(&mut self, n: u64) -> Vec<ObjId> {
        let pm = self.package_metadata_mut().expect("PackageMetadata");
        let last = pm.u64("last_object_identifier").unwrap_or(0);
        pm.set_u64("last_object_identifier", last + n);
        (last + 1..=last + n).collect()
    }

    /// Bytes of a data file by data identifier (images, movies, audio).
    pub fn data(&self, data_id: u64) -> Option<(&str, Cow<'_, [u8]>)> {
        if let Some(e) = self.data_entry(data_id) {
            return Some((e.name.as_str(), self.entry_bytes(e)?));
        }
        // theme artwork Keynote keeps in its app bundle: the file only names it
        // (`document_resource_locator`, e.g. "gradient/gradient_hd"); look in the resource folders
        let pm = self.package_metadata()?;
        let info = pm.msgs("datas").find(|d| d.u64("identifier") == Some(data_id))?;
        let locator = info.str("document_resource_locator")?;
        let name = info.str("preferred_file_name").unwrap_or(locator);
        let digest = info.bytes("digest").map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<String>());
        resource_bytes(locator)
            .or_else(|| digest.and_then(|d| resource_by_digest(&d)))
            // never downloaded: its average colour (Keynote's resource index) stands in
            .or_else(|| resource_average(locator))
            .map(|b| (name, Cow::Owned(b)))
    }

    /// The entry holding a data file (no bytes read).
    pub fn data_entry(&self, data_id: u64) -> Option<&Entry> {
        let pm = self.package_metadata()?;
        let info = pm.msgs("datas").find(|d| d.u64("identifier") == Some(data_id))?;
        let candidates = [info.str("file_name"), info.str("preferred_file_name")];
        for c in candidates.into_iter().flatten() {
            if let Some(e) = self.entry(&format!("Data/{c}")) {
                return Some(e);
            }
        }
        // fall back to the "<name>-<id>.<ext>" naming convention
        let tag = format!("-{data_id}.");
        self.entries
            .iter()
            .find(|e| e.name.starts_with("Data/") && e.name.contains(&tag) && !e.name.contains("-small-"))
    }

    /// TSP.ComponentInfo whose root object is `id`.
    pub fn component(&self, id: ObjId) -> Option<&Msg> {
        self.package_metadata()?.msgs("components").find(|c| c.u64("identifier") == Some(id))
    }

    pub fn component_mut(&mut self, id: ObjId) -> Option<&mut Msg> {
        self.package_metadata_mut()?.msgs_mut("components").find(|c| c.u64("identifier") == Some(id))
    }

    /// Component (by root id) that is stored in the given .iwa file.
    pub fn component_for_file(&self, file: &str) -> Option<ObjId> {
        let stem = file.strip_prefix("Index/")?.strip_suffix(".iwa")?;
        self.package_metadata()?
            .msgs("components")
            .find(|c| c.str("locator").or(c.str("preferred_locator")) == Some(stem))
            .and_then(|c| c.u64("identifier"))
    }
}

fn collect_dir(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), Error> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            collect_dir(root, &p, out)?;
        } else {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, std::fs::read(&p)?));
        }
    }
    Ok(())
}

/// Folders holding Keynote's theme artwork (copied from Keynote.app for local use):
/// `$KN_RESOURCES`, then `~/.local/share/keynote-linux/resources`.
fn resource_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = std::env::var_os("KN_RESOURCES") {
        v.push(PathBuf::from(d));
    }
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(b) = base {
        // Keydeck's folder (and the one from before the app was renamed)
        v.push(b.join("keydeck/resources"));
        v.push(b.join("keynote-linux/resources"));
    }
    // installed with Keydeck (e.g. /usr/share/keydeck/resources)
    let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    v.extend(system.split(':').filter(|d| !d.is_empty()).map(|d| PathBuf::from(d).join("keydeck/resources")));
    v
}

/// The bytes of a document resource: `<dir>/<locator>.<any extension>` (or the file named
/// exactly like the locator's last part), first match.
fn resource_bytes(locator: &str) -> Option<Vec<u8>> {
    let rel = std::path::Path::new(locator);
    for dir in resource_dirs() {
        let full = dir.join(rel);
        if full.is_file() {
            return std::fs::read(full).ok();
        }
        let (Some(parent), Some(stem)) = (full.parent(), full.file_name().and_then(|s| s.to_str())) else { continue };
        let Ok(rd) = std::fs::read_dir(parent) else { continue };
        for e in rd.flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            if n.strip_prefix(stem).is_some_and(|rest| rest.starts_with('.')) {
                return std::fs::read(e.path()).ok();
            }
        }
    }
    None
}

/// Theme artwork by its SHA-1 (as Keynote's own resource cache names it): a file named
/// `<sha1>.<ext>` in a resource folder, or one a MANIFEST.tsv lists with that digest.
fn resource_by_digest(sha1: &str) -> Option<Vec<u8>> {
    use std::sync::OnceLock;
    static INDEX: OnceLock<std::collections::HashMap<String, PathBuf>> = OnceLock::new();
    let index = INDEX.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        for dir in resource_dirs() {
            // MANIFEST.tsv: theme, locator, file name, sha1, file (relative to the folder's parent)
            if let Ok(text) = std::fs::read_to_string(dir.join("MANIFEST.tsv")) {
                for line in text.lines().skip(1) {
                    let cols: Vec<&str> = line.split('\t').collect();
                    if let (Some(d), Some(f)) = (cols.get(3), cols.get(4)) {
                        let path = dir.parent().map(|p| p.join(f)).filter(|p| p.is_file()).unwrap_or_else(|| dir.join(f.trim_start_matches("resources/")));
                        // (rows for files that aren't there, e.g. "MISSING", don't count)
                        if !path.is_file() {
                            continue;
                        }
                        m.entry(d.to_ascii_lowercase()).or_insert(path.clone());
                        // the digest decks recorded for it, when Keynote now uses a newer file
                        if let Some(old) = cols.get(6).filter(|o| o.len() == 40) {
                            m.entry(old.to_ascii_lowercase()).or_insert(path);
                        }
                    }
                }
            }
            // and any file named by its digest, anywhere under the folder
            let mut stack = vec![dir.clone()];
            while let Some(d) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&d) else { continue };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        if stem.len() == 40 && stem.chars().all(|c| c.is_ascii_hexdigit()) {
                            m.entry(stem.to_ascii_lowercase()).or_insert(p);
                        }
                    }
                }
            }
        }
        m
    });
    index.get(&sha1.to_ascii_lowercase()).and_then(|p| std::fs::read(p).ok())
}

/// A one-pixel picture (BMP) of a theme resource's average colour, from
/// keynote-resource-index.tsv (locator, …, avg_color "#RRGGBB" in column 7) in a resource folder.
fn resource_average(locator: &str) -> Option<Vec<u8>> {
    use std::sync::OnceLock;
    static INDEX: OnceLock<std::collections::HashMap<String, [u8; 3]>> = OnceLock::new();
    let index = INDEX.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        for dir in resource_dirs() {
            let Ok(text) = std::fs::read_to_string(dir.join("keynote-resource-index.tsv")) else { continue };
            for line in text.lines().skip(1) {
                let cols: Vec<&str> = line.split('\t').collect();
                let (Some(loc), Some(c)) = (cols.first(), cols.get(6)) else { continue };
                let c = c.trim_start_matches('#');
                if c.len() == 6 {
                    if let Ok(v) = u32::from_str_radix(c, 16) {
                        m.entry(loc.to_string()).or_insert([(v >> 16) as u8, (v >> 8) as u8, v as u8]);
                    }
                }
            }
        }
        m
    });
    let [r, g, b] = *index.get(locator)?;
    // 1x1 24-bit BMP (bottom-up, rows padded to 4 bytes)
    let mut out = Vec::with_capacity(58);
    out.extend(b"BM");
    out.extend(58u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(54u32.to_le_bytes());
    out.extend(40u32.to_le_bytes());
    out.extend(1i32.to_le_bytes());
    out.extend(1i32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(24u16.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(4u32.to_le_bytes());
    out.extend([0u8; 16]);
    out.extend([b, g, r, 0]);
    Some(out)
}

/// Whether `path` is on a network or cloud filesystem (FUSE mounts such as rclone or sshfs,
/// NFS, SMB...), from the mount table.
pub fn is_remote(path: &Path) -> bool {
    let Ok(abs) = std::fs::canonicalize(path) else { return false };
    let Ok(info) = std::fs::read_to_string("/proc/self/mountinfo") else { return false };
    let mut best: Option<(usize, String)> = None;
    for line in info.lines() {
        // id parent major:minor root mountpoint options ... - fstype source options
        let mut it = line.split(' ');
        let Some(mp) = it.nth(4) else { continue };
        let mp = mp.replace("\\040", " ");
        let Some(ty) = line.split(" - ").nth(1).and_then(|r| r.split(' ').next()) else { continue };
        if abs.starts_with(&mp) && best.as_ref().is_none_or(|b| mp.len() > b.0) {
            best = Some((mp.len(), ty.to_string()));
        }
    }
    best.is_some_and(|(_, t)| {
        (t.starts_with("fuse") && t != "fuseblk") || matches!(t.as_str(), "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "9p" | "ceph" | "afs" | "davfs")
    })
}

/// A local copy of a remote file in the cache (~/.cache/keydeck/remote), reused while the
/// remote file's size and modification time are unchanged. Copies with progress and Cancel.
pub fn local_copy(path: &Path) -> Result<PathBuf, Error> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path)?;
    let stamp = format!("{} {:?}", meta.len(), meta.modified().ok());
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"))).ok_or(Error::Invalid("no cache folder".into()))?;
    let dir = base.join("keydeck").join("remote");
    std::fs::create_dir_all(&dir)?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::fs::canonicalize(path).unwrap_or(path.to_path_buf()).hash(&mut h);
    let key = format!("{:016x}", h.finish());
    let (copy, stamp_file) = (dir.join(format!("{key}.key")), dir.join(format!("{key}.stamp")));
    if copy.is_file() && std::fs::read_to_string(&stamp_file).is_ok_and(|s| s == stamp) {
        crate::progress::report(1.0);
        return Ok(copy);
    }
    // forget copies not used for two weeks
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a.as_secs() > 14 * 86400);
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    crate::progress::span(0.0, 0.85);
    let tmp = dir.join(format!("{key}.part"));
    let r = (|| -> Result<(), Error> {
        let mut src = std::fs::File::open(path)?;
        let mut dst = std::fs::File::create(&tmp)?;
        let total = meta.len().max(1);
        let mut buf = vec![0u8; 4 << 20];
        let mut done = 0u64;
        loop {
            let n = src.read(&mut buf)?;
            if n == 0 {
                break;
            }
            std::io::Write::write_all(&mut dst, &buf[..n])?;
            done += n as u64;
            if !crate::progress::report(done as f32 / total as f32) {
                return Err(Error::Invalid("cancelled".into()));
            }
        }
        if done != meta.len() {
            return Err(Error::Invalid("the file changed while it was being read".into()));
        }
        Ok(())
    })();
    if let Err(e) = r {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &copy)?;
    let _ = std::fs::write(&stamp_file, stamp);
    Ok(copy)
}

#[cfg(test)]
mod resource_tests {
    /// Theme artwork that was never downloaded stands in as its average colour (a 1x1 BMP).
    #[test]
    fn average_colour_stand_in() {
        let dir = std::env::temp_dir().join(format!("kd-res-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("keynote-resource-index.tsv"), "locator\tcurrent_sha1\ttype\tbytes\twidth\theight\tavg_color\ttags\tlocal_file\nzz_test/photo_1x1\tx\tjpeg\t1\t1\t1\t#336699\tT\tnot on this Mac\n").unwrap();
        // SAFETY: tests in this module run before anything else reads the variable
        unsafe { std::env::set_var("KN_RESOURCES", &dir) };
        let bmp = super::resource_average("zz_test/photo_1x1").expect("stand-in");
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(&bmp[54..57], &[0x99, 0x66, 0x33]);
        assert!(super::resource_average("zz_test/unknown").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
