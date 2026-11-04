//! The window's library and its saving, and opening a Mac's Notes library.
//!
//! The app works on its own library, nt-core's Store (a NoteStore-format SQLite in
//! ~/.local/share/bramble-notes, or NT_LIBRARY=<dir>): opened at startup (empty at first, as
//! Notes is), every change written through the Store as it's made; note text
//! a second after the last keystroke, when another note is opened, and on quit. Snapshots use
//! the demo (or a store named on the command line, read only) and never the user's library.
//!
//! File > Import to Notes… with a Mac's NoteStore.sqlite (or its group.com.apple.notes folder)
//! copies it into the library (the Mac's store is only read, from a copy).

use std::path::Path;

use nt_core::model::Library;
pub use nt_core::store::Store;

use crate::app::App;

/// How long after the last edit a note's text is saved (Notes saves as you pause).
pub const SAVE_AFTER_S: f64 = 1.0;

/// The app's own library: NT_LIBRARY=<dir>, else ~/.local/share/bramble-notes.
pub fn open_own() -> Result<Store, String> {
    let r = match std::env::var_os("NT_LIBRARY") {
        // (a test's library opens the way the app's own does: converted if it's in the old layout)
        Some(d) => Store::open_library(Path::new(&d)),
        None => Store::open_default(),
    };
    // (a new library starts empty, as Notes does: no demo notes in the user's own library;
    // the demo is only for snapshots and when no library opens)
    r.map_err(|e| e.to_string())
}

/// Run `f` on the app's library (when there is one); an error shows as an alert.
pub fn with_db<T>(app: &mut App, what: &str, f: impl FnOnce(&mut Store) -> nt_core::Result<T>) -> Option<T> {
    let st = app.db.as_mut()?;
    // (a change to sync a little later; reads come through here too, which only makes it sooner)
    app.sync.changed();
    // (and the library's files changed by this window, not another process)
    app.disk.own = true;
    match f(st) {
        Ok(v) => Some(v),
        Err(e) => {
            app.dialog = Some(crate::app::Dialog::Alert(format!("{what} failed."), e.to_string()));
            None
        }
    }
}

/// The library's files as last seen (library.json, notes/, the index), checked every couple of
/// seconds: another process writing them (an import from the command line, a sync worker, another
/// window) is read in without a restart.
#[derive(Default)]
pub struct DiskWatch {
    seen: Option<Vec<Option<std::time::SystemTime>>>,
    checked: f64,
    /// this window wrote through the Store since the last look: what changed is its own
    pub own: bool,
}

const WATCH_EVERY_S: f64 = 2.0;

fn disk_stamps(dir: &Path) -> Vec<Option<std::time::SystemTime>> {
    ["library.json", "notes", "index/NoteStore.sqlite", "index/NoteStore.sqlite-wal"].iter().map(|f| std::fs::metadata(dir.join(f)).and_then(|m| m.modified()).ok()).collect()
}

/// Each frame: when the library's files changed and not by this window, brought in step with them
/// and read again (not while a note has unsaved edits: that waits for its save).
pub fn watch(app: &mut App, ctx: &egui::Context) {
    let now = crate::dates::now();
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(WATCH_EVERY_S));
    if now - app.disk.checked < WATCH_EVERY_S || !app.jobs.is_empty() {
        return;
    }
    app.disk.checked = now;
    let Some(dir) = app.db.as_ref().filter(|st| st.is_native()).map(|st| st.dir().to_path_buf()) else { return };
    let stamps = disk_stamps(&dir);
    if std::mem::take(&mut app.disk.own) || app.disk.seen.is_none() {
        app.disk.seen = Some(stamps);
        return;
    }
    if app.disk.seen.as_ref() == Some(&stamps) || !app.dirty.is_empty() {
        return;
    }
    let _t = crate::PartTimer::start("library changed on disk");
    let r = app.db.as_mut().map(|st| st.refresh());
    // (what the refresh wrote itself isn't another change)
    app.disk.seen = Some(disk_stamps(&dir));
    match r {
        Some(Ok(r)) if r.read > 0 || r.erased > 0 || r.folders => reload(app),
        Some(Err(e)) => app.notify(format!("Couldn’t read the library’s changes: {e}")),
        _ => {}
    }
}

/// Reads the library again from the Store (after changes to many notes or folders at once).
pub fn reload(app: &mut App) {
    let Some(st) = app.db.as_ref() else { return };
    // (light notes, as at startup: the selected one is read in full again as it's shown)
    match st.library_meta() {
        Ok(mut lib) => {
            // notes opened with their password stay open
            for n in lib.notes.iter_mut() {
                if let Some(open) = app.library.notes.iter().find(|o| o.id == n.id && o.lock.is_some()) {
                    *n = open.clone();
                }
            }
            app.library = lib;
        }
        Err(e) => app.notify(format!("Couldn’t read the library: {e}")),
    }
}

/// Saves a note's text (and its attachments) now.
pub fn save_note(app: &mut App, id: &str) {
    let _t = crate::PartTimer::start("save note");
    app.dirty.remove(id);
    app.sync.changed();
    let Some(mut n) = app.note_by_id(id).cloned() else { return };
    if n.locked && n.lock.is_none() {
        return;
    }
    if let Some(saved) = with_db(app, "Saving the note", |st| st.save_note(&mut n).map(|_| n)) {
        // (the copies the Store made of attached files)
        if let Some(m) = app.library.notes.iter_mut().find(|m| m.id == id) {
            m.attachments = saved.attachments;
        }
    }
}

/// Saves the notes edited more than SAVE_AFTER_S ago (all of them when `all`).
pub fn flush(app: &mut App, now: f64, all: bool) {
    if app.db.is_none() {
        app.dirty.clear();
        return;
    }
    let due: Vec<String> = app.dirty.iter().filter(|(_, t)| all || now - **t >= SAVE_AFTER_S).map(|(id, _)| id.clone()).collect();
    for id in due {
        save_note(app, &id);
    }
}

/// Whether `p` looks like a Mac Notes library (Import to Notes… takes these; other files are
/// text, Markdown or HTML imports).
pub fn is_store(p: &Path) -> bool {
    // (a folder is a library when it holds a NoteStore.sqlite; any other folder is imported as files)
    if p.is_dir() {
        return p.join("NoteStore.sqlite").exists();
    }
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sqlite"))
}

/// Read a NoteStore.sqlite (or the group.com.apple.notes folder holding it) with nt-core's reader
/// (it copies the store first and never writes it).
pub fn open(p: &Path) -> Result<Library, String> {
    if !p.exists() {
        return Err(format!("“{}” doesn’t exist.", p.display()));
    }
    nt_core::import::read_library(p).map_err(|e| e.to_string())
}

/// Add an imported library's accounts, folders and notes (an account already there by name
/// gets the new folders and notes).
pub fn merge(app: &mut App, lib: Library) {
    let mut map = std::collections::HashMap::new();
    for a in lib.accounts {
        match app.library.accounts.iter().find(|x| x.name == a.name) {
            Some(x) => {
                map.insert(a.id.clone(), x.id.clone());
            }
            None => {
                map.insert(a.id.clone(), a.id.clone());
                app.library.accounts.push(a);
            }
        }
    }
    for mut f in lib.folders {
        if let Some(a) = map.get(&f.account) {
            f.account = a.clone();
        }
        if !app.library.folders.iter().any(|x| x.id == f.id) {
            app.library.folders.push(f);
        }
    }
    for n in lib.notes {
        if !app.library.notes.iter().any(|x| x.id == n.id) {
            app.library.notes.push(n);
        }
    }
    app.library.tags.extend(lib.tags);
}

/// File > Import to Notes… with a Mac library: read it and copy it into the app's library.
/// Locked notes come over still locked, to open here with their own password; only ones locked
/// with the Mac login password (its key is in the Mac's keychain) are left out.
pub fn import_mac(app: &mut App, p: &Path) {
    let (lib, _) = match nt_core::import::read_library_with(p, &nt_core::import::Options::default()) {
        Ok(v) => v,
        Err(e) => {
            app.dialog = Some(crate::app::Dialog::Alert("Import to Notes failed.".into(), e.to_string()));
            return;
        }
    };
    let Some(sum) = with_db(app, "Import to Notes", |st| st.import_library(&lib)) else { return };
    reload(app);
    let mut msg = format!("Imported {} notes", sum.notes);
    if sum.skipped_locked > 0 {
        let n = sum.skipped_locked;
        msg.push_str(&format!(" ({n} {} locked with your Mac login password left out: switch Notes to a custom password on the Mac to bring {} over)", if n == 1 { "note" } else { "notes" }, if n == 1 { "it" } else { "them" }));
    }
    if sum.skipped_existing > 0 {
        msg.push_str(&format!(", {} already here", sum.skipped_existing));
    }
    app.notify(msg);
}

#[cfg(test)]
mod watch_tests {
    use super::*;
    use nt_core::model::{Body, ParaStyle, Paragraph};

    /// Another process's write (an import, a sync worker) shows in the library's files, and this
    /// window's Store reads it in at its refresh.
    #[test]
    fn another_stores_write_is_seen_and_read_in() {
        let dir = std::env::temp_dir().join(format!("nt-app-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut mine = Store::open_library(&dir).unwrap();
        let _ = mine.refresh().unwrap();
        let before = disk_stamps(&dir);
        // (an empty library has no notes/ yet)
        assert!(before[0].is_some() && before[2].is_some(), "{before:?}");
        std::thread::sleep(std::time::Duration::from_millis(20));
        {
            let mut other = Store::open_library(&dir).unwrap();
            let mut body = Body::default();
            body.paragraphs.push(Paragraph::plain(ParaStyle::Title, "From elsewhere"));
            other.create_note(None, &mut body).unwrap();
        }
        assert_ne!(disk_stamps(&dir), before);
        let r = mine.refresh().unwrap();
        assert_eq!(r.read, 1, "{r:?}");
        assert!(mine.library_meta().unwrap().notes.iter().any(|n| n.title == "From elsewhere"));
        // (nothing new the second time: no reload)
        let r = mine.refresh().unwrap();
        assert!(r.read == 0 && r.erased == 0 && !r.folders, "{r:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upnote_summary_says_what_came_in() {
        let r = nt_core::upnote::UpNoteImport { notes: 766, trashed: 70, ..Default::default() };
        assert_eq!(crate::menus::upnote_summary(&r), "Imported 766 notes (70 to Recently Deleted)");
        let r = nt_core::upnote::UpNoteImport { notes: 1, skipped: 765, ..Default::default() };
        assert_eq!(crate::menus::upnote_summary(&r), "Imported 1 note, 765 already imported");
    }
}
