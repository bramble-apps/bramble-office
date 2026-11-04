//! Notes' engine behind a C interface, for front ends other than the egui app (the Flutter app
//! for Android). One library open at a time. Every call is `nt_call(method, json)` and answers
//! JSON (`{"error": "…"}` when it failed); strings returned to the caller are freed with
//! `nt_free_str`. Notes opened with `note` / `unlock` stay open here (with their key, for locked
//! ones) until `close`, so `save` only sends what the editor changed: the body, and edited or
//! new attachments.
//!
//! Bodies are paragraphs `{st, sp: [span…], ind, al, q, ck, cid, sn, id, col, dir}` with spans
//! `{t, b, i, u, s, sc, ln, hl, c, f, tk}` (defaults left out), as in nt-core's model.

mod smart;

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use nt_core::export;
use nt_core::model::*;
use nt_core::store::Store;
use serde_json::{Map, Value, json};

struct Session {
    st: Store,
    /// notes the editor has open, as last read or saved
    open: HashMap<String, Note>,
    /// fully read notes for Smart Folder rules that need a body (checklists, attachments)
    facts: HashMap<String, (f64, bool, bool)>,
}

static S: Mutex<Option<Session>> = Mutex::new(None);
static ERR: Mutex<String> = Mutex::new(String::new());

fn out(s: String) -> *mut c_char {
    CString::new(s.replace('\0', "")).map(CString::into_raw).unwrap_or(std::ptr::null_mut())
}

fn arg(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Frees a string this library returned.
#[unsafe(no_mangle)]
pub extern "C" fn nt_free_str(p: *mut c_char) {
    if !p.is_null() {
        drop(unsafe { CString::from_raw(p) });
    }
}

/// The last error, as text.
#[unsafe(no_mangle)]
pub extern "C" fn nt_error() -> *mut c_char {
    out(ERR.lock().unwrap_or_else(|e| e.into_inner()).clone())
}

/// Runs `method` with its JSON arguments; answers JSON.
#[unsafe(no_mangle)]
pub extern "C" fn nt_call(method: *const c_char, args: *const c_char) -> *mut c_char {
    let m = arg(method);
    let a: Value = serde_json::from_str(&arg(args)).unwrap_or(Value::Null);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(&m, &a)));
    let v = match r {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            *ERR.lock().unwrap_or_else(|e| e.into_inner()) = e.clone();
            json!({ "error": e })
        }
        Err(_) => json!({ "error": format!("{m} failed") }),
    };
    out(v.to_string())
}

type R = Result<Value, String>;

fn s<'a>(a: &'a Value, k: &str) -> &'a str {
    a.get(k).and_then(Value::as_str).unwrap_or("")
}

fn opt<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn b(a: &Value, k: &str) -> bool {
    a.get(k).and_then(Value::as_bool).unwrap_or(false)
}

/// WebDAV sync, as the desktop's Settings > Sync (nt_core::sync): the account and the last
/// outcome (`sync_get`), the account saved (`sync_set`: on, url, user, password, folder), Test
/// Connection (`sync_test`: {"problem": null or the server's complaint}) and a sync (`sync_now`,
/// force: "push" / "pull" for Replace Server / Replace Notes Here). A sync runs with a Store of its
/// own, as on the desktop, without holding the session (the app goes on meanwhile): the front end
/// reads the library again when it brought anything in.
fn sync_call(m: &str, a: &Value) -> R {
    use nt_core::sync::{self, Force, Settings, Status};
    let dir = S.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|ses| ses.st.dir().to_path_buf()).ok_or("no library open")?;
    let settings = Settings::load(&dir);
    Ok(match m {
        "sync_get" => {
            let st = Status::load(&dir);
            json!({
                "on": settings.on, "url": settings.url, "user": settings.user, "password": settings.password, "folder": settings.folder,
                "ready": settings.ready(),
                "presets": sync::PRESETS.iter().map(|(n, u)| json!([n, u])).collect::<Vec<_>>(),
                "last": st.last_sync, "summary": st.summary, "failed": st.failed,
            })
        }
        "sync_set" => {
            let d = Settings::default();
            let n = Settings {
                on: b(a, "on"),
                url: s(a, "url").trim().to_string(),
                user: s(a, "user").trim().to_string(),
                password: s(a, "password").to_string(),
                folder: opt(a, "folder").map(str::to_string).unwrap_or(d.folder),
            };
            n.save(&dir).map_err(e)?;
            json!({ "ready": n.ready() })
        }
        "sync_test" => json!({ "problem": sync::test_connection(&settings) }),
        // how far a sync running now (on its worker) is: null when none is
        "sync_progress" => match sync::progress() {
            Some(p) => json!({
                "message": p.message(), "fraction": p.fraction(), "item": p.item,
                "done": p.done, "total": p.total, "bytes_done": p.bytes_done, "bytes_total": p.bytes_total,
                "phase": format!("{:?}", p.phase).to_lowercase(),
            }),
            None => Value::Null,
        },
        "sync_now" => {
            let force = match s(a, "force") {
                "push" => Force::Push,
                "pull" => Force::Pull,
                _ => Force::None,
            };
            let mut st = Store::open(&dir).map_err(e)?;
            let r = sync::sync_now(&mut st, &settings, force).map_err(e)?;
            json!({ "sent": r.sent, "received": r.received, "removed": r.removed, "copies": r.copies, "merged": r.merged, "failed": r.failed, "summary": r.summary() })
        }
        _ => return Err(format!("unknown call {m}")),
    })
}

fn e(x: impl ToString) -> String {
    x.to_string()
}

fn call(m: &str, a: &Value) -> R {
    if m == "open" {
        // (Android has no /tmp: the app's cache folder, for .bnotes imports)
        if let Some(t) = opt(a, "tmp") {
            unsafe { std::env::set_var("TMPDIR", t) };
        }
        // the library as files (docs/FORMAT.md); an old one converts once, with a backup
        let st = match opt(a, "dir") {
            Some(d) => Store::open_library(Path::new(d)),
            None => Store::open_default(),
        }
        .map_err(e)?;
        let migration = match st.migration() {
            Some(Ok(m)) => json!({
                "notes": m.notes, "folders": m.folders, "attachments": m.attachments, "locked": m.locked,
                "tags": m.tags, "seconds": m.seconds, "backup": m.backup.to_string_lossy(),
            }),
            Some(Err(msg)) => json!({ "error": msg }),
            None => Value::Null,
        };
        *S.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session { st, open: HashMap::new(), facts: HashMap::new() });
        return Ok(json!({ "migration": migration }));
    }
    if m == "opening" {
        // what a slow open (the first, converting the library) is doing, while it runs
        return Ok(json!({ "message": nt_core::native::opening().message() }));
    }
    if m == "smart_parse" {
        return Ok(query_json(&smart::parse(s(a, "json"))));
    }
    if m == "uti" {
        return Ok(json!({ "uti": nt_core::store::uti_for(Path::new(s(a, "path"))) }));
    }
    if m == "smart_filters" {
        return Ok(json!({ "filters": smart::FILTERS, "ranges": smart::RANGES.iter().map(|r| r.0).collect::<Vec<_>>() }));
    }
    if m.starts_with("sync_") {
        return sync_call(m, a);
    }
    let mut g = S.lock().unwrap_or_else(|e| e.into_inner());
    let ses = g.as_mut().ok_or("no library open")?;
    let st = &mut ses.st;
    let id = s(a, "id");
    Ok(match m {
        "library" => library(ses)?,
        "note" => {
            let n = st.note(id).map_err(e)?;
            let v = note_json(&n);
            ses.open.insert(n.id.clone(), n);
            v
        }
        "close" => {
            ses.open.remove(id);
            json!({})
        }
        "save" => save(ses, a)?,
        "create" => {
            let mut body = match a.get("body") {
                Some(v) if v.is_array() => body_of(v),
                _ => Body { paragraphs: vec![Paragraph::plain(ParaStyle::Title, "")] },
            };
            let n = st.create_note(opt(a, "folder"), &mut body).map_err(e)?;
            let v = note_json(&n);
            ses.open.insert(n.id.clone(), n);
            v
        }
        "move" => {
            st.move_note(id, s(a, "folder")).map_err(e)?;
            if let Some(n) = ses.open.get_mut(id) {
                n.folder = s(a, "folder").into();
            }
            json!({})
        }
        "pin" => st.set_pinned(id, b(a, "on")).map(|_| json!({})).map_err(e)?,
        "delete" => st.delete_note(id).map(|_| json!({})).map_err(e)?,
        "recover" => st.recover_note(id, opt(a, "folder")).map(|_| json!({})).map_err(e)?,
        "erase" => {
            ses.open.remove(id);
            st.erase_note(id).map(|_| json!({})).map_err(e)?
        }
        "empty_trash" => json!({ "count": st.empty_trash().map_err(e)? }),
        // (a top-level folder in `account`, when given: a Mac's iCloud kept as its own account)
        "create_folder" => json!({
            "id": match (opt(a, "parent"), opt(a, "account")) {
                (None, Some(acct)) => st.create_folder_in(acct, s(a, "name")),
                (parent, _) => st.create_folder(s(a, "name"), parent),
            }
            .map_err(e)?
        }),
        "create_smart" => json!({ "id": st.create_smart_folder(s(a, "name"), &smart::to_json(&query_of(a))).map_err(e)? }),
        "set_smart" => st.set_smart_query(id, &smart::to_json(&query_of(a))).map(|_| json!({})).map_err(e)?,
        "rename_folder" => st.rename_folder(id, s(a, "name")).map(|_| json!({})).map_err(e)?,
        "move_folder" => st.move_folder(id, opt(a, "parent")).map(|_| json!({})).map_err(e)?,
        "folder_order" => st.set_folder_order(id, a.get("order").and_then(Value::as_i64).unwrap_or(0)).map(|_| json!({})).map_err(e)?,
        "delete_folder" => st.delete_folder(id).map(|_| json!({})).map_err(e)?,
        "search" => json!(st.search(s(a, "q")).map_err(e)?),
        "has_password" => json!({ "on": st.has_password().map_err(e)?, "hint": st.password_hint().map_err(e)? }),
        "check_password" => json!({ "ok": st.check_password(s(a, "pw")).map_err(e)? }),
        "set_password" => st.set_password(s(a, "pw"), opt(a, "hint")).map(|_| json!({})).map_err(e)?,
        "change_password" => st.change_password(s(a, "old"), s(a, "pw"), opt(a, "hint")).map(|_| json!({})).map_err(e)?,
        "lock" => {
            ses.open.remove(id);
            st.lock_note(id, s(a, "pw")).map(|_| json!({})).map_err(e)?
        }
        "unlock" => {
            let n = st.unlock_note(id, s(a, "pw")).map_err(e)?;
            let v = note_json(&n);
            ses.open.insert(n.id.clone(), n);
            v
        }
        "remove_lock" => {
            ses.open.remove(id);
            st.remove_lock(id, s(a, "pw")).map(|_| json!({})).map_err(e)?
        }
        "import" => {
            let lib = nt_core::import::read_library(Path::new(s(a, "path"))).map_err(e)?;
            let sum = st.import_library(&lib).map_err(e)?;
            ses.facts.clear();
            json!({ "summary": format!("{sum:?}") })
        }
        "export" => {
            let n = match ses.open.get(id) {
                Some(n) => n.clone(),
                None => st.note(id).map_err(e)?,
            };
            match s(a, "format") {
                "html" => json!({ "text": export::html(&n).text }),
                "md" | "markdown" => json!({ "text": export::markdown(&n).text }),
                _ => json!({ "text": export::plain_text(&n) }),
            }
        }
        "export_files" => export_files(ses, a)?,
        // Duplicate Note: its body as a new note in its folder (as the desktop's)
        "duplicate" => {
            let n = match ses.open.get(id) {
                Some(n) => n.clone(),
                None => st.note(id).map_err(e)?,
            };
            let mut body = n.body.clone();
            json!({ "id": st.create_note(Some(&n.folder), &mut body).map_err(e)?.id })
        }
        // the whole library as a .bnotes file (lossless; opens in Notes on the desktop too)
        "export_bnotes" => json!({ "notes": nt_core::native::export_bnotes(st, Path::new(s(a, "path"))).map_err(e)? }),
        "import_bnotes" => {
            let r = nt_core::native::import_bnotes(st, Path::new(s(a, "path"))).map_err(e)?;
            ses.facts.clear();
            json!({ "notes": r.notes, "skipped": r.skipped, "folders": r.folders })
        }
        // notes whose files changed outside this Store (a sync on its worker)
        "refresh" => {
            st.refresh().map_err(e)?;
            ses.facts.clear();
            json!({})
        }
        "rename_tag" => json!({ "notes": retag(ses, s(a, "from"), Some(s(a, "to")))? }),
        "delete_tag" => json!({ "notes": retag(ses, s(a, "name"), None)? }),
        "demo" => demo(st)?,
        _ => return Err(format!("unknown call {m}")),
    })
}

/// The Export sheet (the desktop's File > Export…): the notes `ids` written into `dir`, each its
/// own file (+ `Attachments/`), with nt-core's options: `format` text / html / md, `sections`
/// expand / collapse / shown, `data` (Markdown's front matter, with each note's folder path).
/// Answers the files written; `format` pdf instead answers the notes (`notes`, as `note`
/// answers them) with their collapsed sections left out for "shown", for the app to lay out on
/// pages. Locked notes not open here, and deleted ones, are left out.
fn export_files(ses: &mut Session, a: &Value) -> R {
    use export::{Format, Options, Sections};
    let ids: Vec<&str> = a.get("ids").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let mut notes = Vec::new();
    for id in ids {
        let n = match ses.open.get(id) {
            Some(n) => n.clone(),
            None => ses.st.note(id).map_err(e)?,
        };
        if !n.deleted && (!n.locked || ses.open.contains_key(id)) {
            notes.push(n);
        }
    }
    let folders = ses.st.library_meta().map_err(e)?.folders;
    let folder_of = |n: &Note| {
        let mut parts = Vec::new();
        let mut at = folders.iter().find(|f| f.id == n.folder);
        while let Some(f) = at {
            if parts.len() > 64 {
                break;
            }
            parts.push(f.name.clone());
            at = f.parent.as_deref().and_then(|p| folders.iter().find(|f| f.id == p));
        }
        parts.reverse();
        parts.join("/")
    };
    let sections = match s(a, "sections") {
        "collapse" => Sections::CollapseAll,
        "shown" => Sections::AsShown,
        _ => Sections::ExpandAll,
    };
    if s(a, "format") == "pdf" {
        // (the app lays out the pages)
        return Ok(json!({ "notes": notes.iter().map(|n| note_json(&Note { body: export::shown_body(&n.body, sections), ..n.clone() })).collect::<Vec<_>>() }));
    }
    let format = match s(a, "format") {
        "html" => Format::Html,
        "md" | "markdown" => Format::Markdown,
        _ => Format::PlainText,
    };
    let written = export::write_notes_with(&notes.iter().collect::<Vec<_>>(), Path::new(s(a, "dir")), format, &Options { sections, data: b(a, "data") }, &folder_of).map_err(e)?;
    Ok(json!({ "files": written.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>() }))
}

// ---------------------------------------------------------------- the library

fn library(ses: &mut Session) -> R {
    let lib = ses.st.library_meta().map_err(e)?;
    let folders: Vec<Value> = lib
        .folders
        .iter()
        .map(|f| {
            json!({
                "id": f.id, "account": f.account, "name": f.name, "parent": f.parent,
                "kind": match f.kind { FolderKind::Trash => "trash", FolderKind::Smart => "smart", FolderKind::Normal => if f.smart.is_some() { "smart" } else { "normal" } },
                "default": f.default, "order": f.sort_order, "query": f.smart,
            })
        })
        .collect();
    let notes: Vec<Value> = lib
        .notes
        .iter()
        .map(|n| {
            json!({
                "id": n.id, "folder": n.folder, "title": n.title, "snippet": n.snippet,
                "created": n.created, "modified": n.modified, "pinned": n.pinned, "locked": n.locked,
                "deleted": n.deleted, "nd": n.not_downloaded,
                "thumb": n.thumbnail().map(|p| p.to_string_lossy().into_owned()),
                "tags": n.tags(),
            })
        })
        .collect();
    // Smart Folders' notes (as nt-app's smart_matches)
    let now = nt_core::body::now_unix();
    let mut smart_notes = Map::new();
    for f in lib.folders.iter().filter(|f| f.smart.is_some()) {
        let q = smart::parse(f.smart.as_deref().unwrap_or(""));
        let needs_body = q.rules.iter().any(|(k, _)| *k == smart::CHECKLISTS || *k == smart::ATTACHMENTS);
        let mut ids = Vec::new();
        for n in lib.notes.iter().filter(|n| !n.deleted) {
            let tags = n.tags();
            let (mut has_attachment, mut has_checklist) = (n.thumbnail().is_some(), false);
            if needs_body && !n.locked {
                let hit = ses.facts.get(&n.id).filter(|c| c.0 == n.modified).copied();
                let (_, a, c) = match hit {
                    Some(c) => c,
                    None => {
                        let full = ses.st.note(&n.id).ok();
                        let a = full.as_ref().is_some_and(|f| !f.attachments.is_empty());
                        let c = full.as_ref().is_some_and(|f| f.body.paragraphs.iter().any(|p| p.style == ParaStyle::Checklist));
                        ses.facts.insert(n.id.clone(), (n.modified, a, c));
                        (n.modified, a, c)
                    }
                };
                has_attachment |= a;
                has_checklist = c;
            }
            let quick_note = lib.folder(&n.folder).is_some_and(|f| f.name == "Quick Notes" && f.parent.is_none());
            let facts = smart::Facts { tags: &tags, quick_note, has_attachment, has_checklist };
            if smart::matches(&q, n, &facts, now) {
                ids.push(Value::from(n.id.clone()));
            }
        }
        smart_notes.insert(f.id.clone(), Value::Array(ids));
    }
    let accounts: Vec<Value> = lib.accounts.iter().map(|a| json!({ "id": a.id, "name": a.name, "kind": a.kind })).collect();
    let tags: Vec<Value> = lib.tags.iter().map(|t| json!(t.name)).collect();
    Ok(json!({ "accounts": accounts, "folders": folders, "notes": notes, "smart": smart_notes, "tags": tags }))
}

/// Renames a tag in every note that has it (`to`), or takes it off them (None: the tag's text
/// stays, as plain text). Locked notes keep theirs. Answers how many notes changed.
fn retag(ses: &mut Session, from: &str, to: Option<&str>) -> Result<usize, String> {
    let from = from.trim().trim_start_matches('#').to_lowercase();
    let to = to.map(|t| t.trim().trim_start_matches('#').to_string()).filter(|t| !t.is_empty());
    if from.is_empty() {
        return Ok(0);
    }
    let lib = ses.st.library_meta().map_err(e)?;
    let mut changed = 0;
    for n in lib.notes.iter().filter(|n| !n.locked && n.tags().iter().any(|t| t.to_lowercase() == from)) {
        let mut note = ses.st.note(&n.id).map_err(e)?;
        let mut hit = false;
        for p in &mut note.body.paragraphs {
            for sp in &mut p.spans {
                let Some(t) = &mut sp.token else { continue };
                if t.uti != UTI_HASHTAG || t.alt.trim_start_matches('#').to_lowercase() != from {
                    continue;
                }
                hit = true;
                match &to {
                    Some(name) => {
                        t.alt = format!("#{name}");
                        t.content = name.to_uppercase();
                    }
                    None => {
                        sp.text = t.alt.clone();
                        sp.token = None;
                    }
                }
            }
        }
        if hit {
            ses.st.update_note(&n.id, &mut note.body).map_err(e)?;
            ses.open.remove(&n.id);
            changed += 1;
        }
    }
    Ok(changed)
}

fn query_of(a: &Value) -> smart::Query {
    let rules = a
        .get("rules")
        .and_then(Value::as_array)
        .map(|rs| {
            rs.iter()
                .filter_map(|r| Some((r.get(0)?.as_u64()? as u8, r.get(1).and_then(Value::as_str).unwrap_or("").to_string())))
                .collect()
        })
        .unwrap_or_default();
    smart::Query { any: b(a, "any"), rules }
}

fn query_json(q: &smart::Query) -> Value {
    json!({ "any": q.any, "rules": q.rules.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>() })
}

// ---------------------------------------------------------------- notes as JSON

const STYLES: [(ParaStyle, &str); 9] = [
    (ParaStyle::Title, "title"),
    (ParaStyle::Heading, "heading"),
    (ParaStyle::Subheading, "subheading"),
    (ParaStyle::Body, "body"),
    (ParaStyle::Monostyled, "mono"),
    (ParaStyle::Bulleted, "bullet"),
    (ParaStyle::Dashed, "dash"),
    (ParaStyle::Numbered, "number"),
    (ParaStyle::Checklist, "check"),
];

const ALIGNS: [Align; 5] = [Align::Natural, Align::Left, Align::Center, Align::Right, Align::Justified];

const INKS: [(InkKind, &str); 8] = [
    (InkKind::Pen, "pen"),
    (InkKind::Pencil, "pencil"),
    (InkKind::Marker, "marker"),
    (InkKind::Monoline, "monoline"),
    (InkKind::Fountain, "fountain"),
    (InkKind::Watercolor, "watercolor"),
    (InkKind::Crayon, "crayon"),
    (InkKind::Reed, "reed"),
];

fn span_json(sp: &Span) -> Value {
    let mut o = Map::new();
    o.insert("t".into(), json!(sp.text));
    let mut put = |k: &str, on: bool, v: Value| {
        if on {
            o.insert(k.into(), v);
        }
    };
    put("b", sp.bold, json!(true));
    put("i", sp.italic, json!(true));
    put("u", sp.underline, json!(true));
    put("s", sp.strike, json!(true));
    put("sc", sp.script != 0, json!(sp.script));
    put("ln", sp.link.is_some(), json!(sp.link));
    put("hl", sp.highlight.is_some(), json!(sp.highlight));
    put("c", sp.color.is_some(), json!(sp.color));
    if let Some(f) = &sp.font {
        put("f", true, json!({ "name": f.name, "size": f.size, "hints": f.hints }));
    }
    if let Some(t) = &sp.token {
        put("tk", true, json!({ "id": t.id, "uti": t.uti, "alt": t.alt, "content": t.content }));
    }
    Value::Object(o)
}

fn para_json(p: &Paragraph) -> Value {
    let st = STYLES.iter().find(|s| s.0 == p.style).map(|s| s.1).unwrap_or("body");
    let mut o = Map::new();
    o.insert("st".into(), json!(st));
    o.insert("sp".into(), Value::Array(p.spans.iter().map(span_json).collect()));
    let mut put = |k: &str, on: bool, v: Value| {
        if on {
            o.insert(k.into(), v);
        }
    };
    put("ind", p.indent != 0, json!(p.indent));
    put("al", p.align != Align::Natural, json!(ALIGNS.iter().position(|a| *a == p.align).unwrap_or(0)));
    put("q", p.block_quote, json!(true));
    put("ck", p.checked, json!(true));
    put("cid", !p.check_id.is_empty(), json!(p.check_id));
    put("sn", p.start_number.is_some(), json!(p.start_number));
    put("id", !p.id.is_empty(), json!(p.id));
    put("col", p.collapsed, json!(true));
    put("dir", p.direction.is_some(), json!(p.direction));
    Value::Object(o)
}

fn f32s(v: Option<&Value>) -> Vec<f32> {
    v.and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default()
}

fn rgba(v: Option<&Value>) -> Option<[f32; 4]> {
    let f = f32s(v);
    (f.len() == 4).then(|| [f[0], f[1], f[2], f[3]])
}

fn span_of(v: &Value) -> Span {
    let font = v.get("f").map(|f| Font {
        name: opt(f, "name").map(String::from),
        size: f.get("size").and_then(Value::as_f64).map(|x| x as f32),
        hints: f.get("hints").and_then(Value::as_u64).map(|x| x as u32),
    });
    let token = v.get("tk").map(|t| Token { id: s(t, "id").into(), uti: s(t, "uti").into(), alt: s(t, "alt").into(), content: s(t, "content").into() });
    Span {
        text: if token.is_some() { OBJ.to_string() } else { s(v, "t").to_string() },
        bold: b(v, "b"),
        italic: b(v, "i"),
        underline: b(v, "u"),
        strike: b(v, "s"),
        script: v.get("sc").and_then(Value::as_i64).unwrap_or(0).clamp(-1, 1) as i8,
        link: opt(v, "ln").map(String::from),
        highlight: v.get("hl").and_then(Value::as_u64).map(|x| x.clamp(1, 5) as u8),
        color: rgba(v.get("c")),
        font,
        token,
    }
}

fn para_of(v: &Value) -> Paragraph {
    let st = s(v, "st");
    Paragraph {
        style: STYLES.iter().find(|x| x.1 == st).map(|x| x.0).unwrap_or_default(),
        indent: v.get("ind").and_then(Value::as_u64).unwrap_or(0).min(20) as u8,
        align: ALIGNS[v.get("al").and_then(Value::as_u64).unwrap_or(0).min(4) as usize],
        block_quote: b(v, "q"),
        checked: b(v, "ck"),
        check_id: s(v, "cid").into(),
        start_number: v.get("sn").and_then(Value::as_u64).map(|x| x as u32),
        id: s(v, "id").into(),
        collapsed: b(v, "col"),
        direction: v.get("dir").and_then(Value::as_u64).map(|x| x as u32),
        spans: v.get("sp").and_then(Value::as_array).map(|a| a.iter().map(span_of).filter(|s| !s.text.is_empty()).collect()).unwrap_or_default(),
    }
}

fn body_of(v: &Value) -> Body {
    let mut paragraphs: Vec<Paragraph> = v.as_array().map(|a| a.iter().map(para_of).collect()).unwrap_or_default();
    if paragraphs.is_empty() {
        paragraphs.push(Paragraph::plain(ParaStyle::Title, ""));
    }
    Body { paragraphs }
}

fn path_json(p: &Option<PathBuf>) -> Value {
    json!(p.as_ref().map(|p| p.to_string_lossy().into_owned()))
}

fn stroke_json(k: &Stroke) -> Value {
    let pts: Vec<f32> = k.points.iter().flat_map(|p| [p.x, p.y, p.t, p.size, p.force, p.azimuth, p.altitude, p.opacity]).collect();
    json!({
        "id": k.id, "ink": INKS.iter().find(|i| i.0 == k.ink).map(|i| i.1).unwrap_or("pen"),
        "color": k.color, "width": k.width, "pts": pts, "tf": k.transform,
        "mask": k.mask.as_ref().map(|m| m.iter().flat_map(|p| *p).collect::<Vec<f32>>()),
    })
}

fn stroke_of(v: &Value) -> Stroke {
    let pts = f32s(v.get("pts"));
    let tf = f32s(v.get("tf"));
    let ink = s(v, "ink");
    Stroke {
        id: s(v, "id").into(),
        ink: INKS.iter().find(|i| i.1 == ink).map(|i| i.0).unwrap_or_default(),
        color: rgba(v.get("color")).unwrap_or([0.0, 0.0, 0.0, 1.0]),
        width: v.get("width").and_then(Value::as_f64).unwrap_or(3.0) as f32,
        points: pts
            .chunks_exact(8)
            .map(|c| InkPoint { x: c[0], y: c[1], t: c[2], size: c[3], force: c[4], azimuth: c[5], altitude: c[6], opacity: c[7] })
            .collect(),
        transform: if tf.len() == 6 { [tf[0], tf[1], tf[2], tf[3], tf[4], tf[5]] } else { [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] },
        mask: v.get("mask").filter(|m| m.is_array()).map(|m| f32s(Some(m)).chunks_exact(2).map(|c| [c[0], c[1]]).collect()),
    }
}

fn attachment_json(a: &Attachment) -> Value {
    let kind = match a.kind {
        AttachmentKind::Image => "image",
        AttachmentKind::Pdf => "pdf",
        AttachmentKind::Drawing => "drawing",
        AttachmentKind::Table => "table",
        AttachmentKind::Url => "url",
        AttachmentKind::Audio => "audio",
        AttachmentKind::Scan => "scan",
        AttachmentKind::File => "file",
    };
    json!({
        "id": a.id, "uti": a.uti, "kind": kind, "name": a.name,
        "file": path_json(&a.file), "fallback": path_json(&a.fallback), "preview": path_json(&a.preview),
        "w": a.width, "h": a.height, "url": a.url, "summary": a.summary,
        "table": a.table.as_ref().map(|t| json!({ "rows": t.text_grid(), "rtl": t.rtl })),
        "drawing": a.drawing.as_ref().map(|d| json!({ "w": d.width, "h": d.height, "strokes": d.strokes.iter().map(stroke_json).collect::<Vec<_>>(), "shapes": d.shapes.len() })),
        "parts": a.parts.iter().map(attachment_json).collect::<Vec<_>>(),
        "pages": a.pages.len(),
        "view": match a.view_size { ViewSize::Large => 0, ViewSize::Small => 1, ViewSize::Medium => 2 },
        "nd": a.not_downloaded,
    })
}

fn note_json(n: &Note) -> Value {
    json!({
        "id": n.id, "folder": n.folder, "title": n.title, "snippet": n.snippet,
        "created": n.created, "modified": n.modified, "pinned": n.pinned,
        "locked": n.locked, "unlocked": n.lock.is_some(), "deleted": n.deleted, "nd": n.not_downloaded,
        "body": n.body.paragraphs.iter().map(para_json).collect::<Vec<_>>(),
        "attachments": n.attachments.iter().map(attachment_json).collect::<Vec<_>>(),
    })
}

/// A table's grid from rows of cell text, keeping the cells whose text didn't change (they're
/// rich text: their styles stay).
fn table_of(old: Option<&Table>, v: &Value) -> Table {
    let mut t = old.cloned().unwrap_or_default();
    let rows: Vec<Vec<String>> = v
        .get("rows")
        .and_then(Value::as_array)
        .map(|r| r.iter().map(|row| row.as_array().map(|c| c.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default()).collect())
        .unwrap_or_default();
    // where each row and column was before (rows or columns dragged to new places, added: null),
    // so a moved cell keeps its own formatting; without them, the same place
    let from = |k: &str, n: usize| -> Vec<Option<usize>> {
        match v.get(k).and_then(Value::as_array) {
            Some(a) => a.iter().map(|x| x.as_u64().map(|i| i as usize)).collect(),
            None => (0..n).map(Some).collect(),
        }
    };
    let ncols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let (row_from, col_from) = (from("rowFrom", rows.len()), from("colFrom", ncols));
    let old_rows = std::mem::take(&mut t.rows);
    t.rows = rows
        .iter()
        .enumerate()
        .map(|(r, row)| {
            row.iter()
                .enumerate()
                .map(|(c, text)| match row_from.get(r).copied().flatten().zip(col_from.get(c).copied().flatten()).and_then(|(r, c)| old_rows.get(r).and_then(|x| x.get(c))) {
                    Some(cell) if cell.text() == *text => cell.clone(),
                    _ => Body { paragraphs: text.split('\n').map(|l| Paragraph::plain(ParaStyle::Body, l)).collect() },
                })
                .collect()
        })
        .collect();
    t
}

fn drawing_of(old: Option<&Drawing>, v: &Value) -> Drawing {
    let mut d = old.cloned().unwrap_or_default();
    d.width = v.get("w").and_then(Value::as_f64).map(|x| x as f32).unwrap_or(d.width);
    d.height = v.get("h").and_then(Value::as_f64).map(|x| x as f32).unwrap_or(d.height);
    d.strokes = v.get("strokes").and_then(Value::as_array).map(|a| a.iter().map(stroke_of).collect()).unwrap_or_default();
    d
}

/// `{id, body, tables: {att: {rows}}, drawings: {att: {w, h, strokes, fallback}}, views: {att: n},
/// add: [{id, file?, uti?, name?, drawing?, fallback?, table?, pages?}]}` -> the note as saved.
fn save(ses: &mut Session, a: &Value) -> R {
    let id = s(a, "id");
    let mut n = match ses.open.remove(id) {
        Some(n) => n,
        None => ses.st.note(id).map_err(e)?,
    };
    if let Some(v) = a.get("body") {
        n.body = body_of(v);
    }
    let edits = |k: &str| a.get(k).and_then(Value::as_object).cloned().unwrap_or_default();
    for (aid, v) in edits("tables") {
        if let Some(at) = n.attachments.iter_mut().find(|x| x.id == aid) {
            at.table = Some(table_of(at.table.as_ref(), &v));
        }
    }
    for (aid, v) in edits("drawings") {
        if let Some(at) = n.attachments.iter_mut().find(|x| x.id == aid) {
            at.drawing = Some(drawing_of(at.drawing.as_ref(), &v));
            if let Some(f) = opt(&v, "fallback") {
                at.fallback = Some(PathBuf::from(f));
            }
        }
    }
    for (aid, v) in edits("views") {
        if let Some(at) = n.attachments.iter_mut().find(|x| x.id == aid) {
            at.view_size = match v.as_u64() {
                Some(1) => ViewSize::Small,
                Some(2) => ViewSize::Medium,
                _ => ViewSize::Large,
            };
        }
    }
    for v in a.get("add").and_then(Value::as_array).cloned().unwrap_or_default() {
        let file = opt(&v, "file").map(PathBuf::from);
        let mut uti = s(&v, "uti").to_string();
        if uti.is_empty() {
            uti = file.as_deref().map(nt_core::store::uti_for).unwrap_or("public.data").to_string();
        }
        let mut at = Attachment {
            id: s(&v, "id").into(),
            kind: AttachmentKind::from_uti(&uti),
            uti,
            name: s(&v, "name").into(),
            file,
            fallback: opt(&v, "fallback").map(PathBuf::from),
            width: v.get("w").and_then(Value::as_f64).unwrap_or(0.0),
            height: v.get("h").and_then(Value::as_f64).unwrap_or(0.0),
            created: nt_core::body::now_unix(),
            modified: nt_core::body::now_unix(),
            ..Default::default()
        };
        // a PDF's pages as the front end read them ([w, h] points each; nt-core would ask pdfinfo)
        if let Some(ps) = v.get("pages").and_then(Value::as_array) {
            at.pages = ps
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    let f = f32s(Some(p));
                    (f.len() >= 2).then(|| PdfPage { index: i as u32, width: f[0], height: f[1], crop: [0.0, 0.0, f[0], f[1]] })
                })
                .collect();
            if let Some(p) = at.pages.first() {
                at.width = p.width as f64;
                at.height = p.height as f64;
            }
        }
        if let Some(d) = v.get("drawing") {
            at.drawing = Some(drawing_of(None, d));
        }
        if let Some(t) = v.get("table") {
            at.table = Some(table_of(None, t));
        }
        if at.kind == AttachmentKind::Table && at.table.is_none() {
            at.table = Some(Table::from_text(&[&["", ""], &["", ""]]));
        }
        n.attachments.push(at);
    }
    let r = ses.st.save_note(&mut n);
    let v = note_json(&n);
    ses.open.insert(n.id.clone(), n);
    r.map_err(e)?;
    Ok(v)
}

// ---------------------------------------------------------------- sample notes (test builds)

/// A few notes for test builds and snapshots, made only into an empty library.
fn demo(st: &mut Store) -> R {
    if st.library_meta().map(|l| !l.notes.is_empty()).unwrap_or(true) {
        return Ok(json!({ "made": 0 }));
    }
    let recipes = st.create_folder("Recipes", None).map_err(e)?;
    let travel = st.create_folder("Travel", None).map_err(e)?;
    st.create_folder("Japan 2026", Some(&travel)).map_err(e)?;
    st.create_smart_folder("Pinned", &smart::to_json(&smart::Query { any: false, rules: vec![(smart::PINNED, String::new())] })).map_err(e)?;
    let para = |style: ParaStyle, text: &str| Paragraph::plain(style, text);
    let check = |text: &str, done: bool| Paragraph { checked: done, ..Paragraph::plain(ParaStyle::Checklist, text) };
    let mut made = 0;
    let mut add = |st: &mut Store, folder: Option<&str>, paragraphs: Vec<Paragraph>, pinned: bool| -> Result<(), String> {
        let n = st.create_note(folder, &mut Body { paragraphs }).map_err(e)?;
        if pinned {
            st.set_pinned(&n.id, true).map_err(e)?;
        }
        made += 1;
        Ok(())
    };
    add(st, Some(&recipes), vec![para(ParaStyle::Title, "Sourdough"), para(ParaStyle::Heading, "Ingredients"), para(ParaStyle::Bulleted, "500 g flour"), para(ParaStyle::Bulleted, "350 g water"), para(ParaStyle::Bulleted, "100 g starter"), para(ParaStyle::Bulleted, "10 g salt"), para(ParaStyle::Heading, "Method"), para(ParaStyle::Numbered, "Mix and rest an hour"), para(ParaStyle::Numbered, "Stretch and fold four times"), para(ParaStyle::Numbered, "Shape, proof overnight, bake at 250°")], false)?;
    add(st, None, vec![para(ParaStyle::Title, "Groceries"), check("Milk", true), check("Eggs", false), check("Coffee beans", false), check("Apples", false)], true)?;
    let mut bold = Span::plain("Bold");
    bold.bold = true;
    let mut hl = Span::plain("highlighted");
    hl.highlight = Some(4);
    add(st, None, vec![para(ParaStyle::Title, "Meeting notes"), para(ParaStyle::Subheading, "Tuesday"), Paragraph { spans: vec![bold, Span::plain(" points and "), hl, Span::plain(" ones.")], ..para(ParaStyle::Body, "") }, para(ParaStyle::Dashed, "Budget for next year"), para(ParaStyle::Dashed, "Who books the hall"), para(ParaStyle::Monostyled, "room 204, 9:30")], false)?;
    add(st, Some(&travel), vec![para(ParaStyle::Title, "Packing list"), para(ParaStyle::Body, "Passport, chargers, the good walking shoes.")], false)?;
    Ok(json!({ "made": made }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call_s(m: &str, a: Value) -> Value {
        let (m, a) = (CString::new(m).unwrap(), CString::new(a.to_string()).unwrap());
        let p = nt_call(m.as_ptr(), a.as_ptr());
        let v = serde_json::from_str(&unsafe { CStr::from_ptr(p) }.to_string_lossy()).unwrap();
        nt_free_str(p);
        v
    }

    #[test]
    fn moved_rows_and_columns_keep_their_cells() {
        // a heading cell (rich text) at row 0, column 1
        let cell = |st, t: &str| Body { paragraphs: vec![Paragraph::plain(st, t)] };
        let old = Table { rows: vec![vec![cell(ParaStyle::Body, "a"), cell(ParaStyle::Title, "B")], vec![cell(ParaStyle::Body, "c"), cell(ParaStyle::Body, "d")]], rtl: false };
        // the rows swapped and the columns swapped: "B" is now at row 1, column 0
        let t = table_of(Some(&old), &json!({ "rows": [["d", "c"], ["B", "a"]], "rowFrom": [1, 0], "colFrom": [1, 0] }));
        assert_eq!(t.text_grid(), vec![vec!["d".to_string(), "c".into()], vec!["B".into(), "a".into()]]);
        assert_eq!(t.rows[1][0].paragraphs[0].style, ParaStyle::Title);
        // a new row (null) is plain; without the orders, cells match by place as before
        let t = table_of(Some(&old), &json!({ "rows": [["", ""], ["a", "B"]], "rowFrom": [null, 0] }));
        assert_eq!(t.rows[1][1].paragraphs[0].style, ParaStyle::Title);
        let t = table_of(Some(&old), &json!({ "rows": [["a", "B"], ["c", "d"]] }));
        assert_eq!(t.rows[0][1].paragraphs[0].style, ParaStyle::Title);
    }

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("nt-ffi-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(call_s("open", json!({ "dir": dir })).get("error").is_none());
        assert_eq!(call_s("demo", json!({}))["made"], 4);
        let lib = call_s("library", json!({}));
        assert_eq!(lib["notes"].as_array().unwrap().len(), 4);
        let pinned = lib["folders"].as_array().unwrap().iter().find(|f| f["name"] == "Pinned").unwrap()["id"].as_str().unwrap().to_string();
        assert_eq!(lib["smart"][&pinned].as_array().unwrap().len(), 1);
        let id = lib["notes"].as_array().unwrap().iter().find(|n| n["title"] == "Meeting notes").unwrap()["id"].as_str().unwrap().to_string();
        let n = call_s("note", json!({ "id": id }));
        assert_eq!(n["body"][2]["sp"][0]["b"], true);
        assert_eq!(n["body"][2]["sp"][2]["hl"], 4);
        let mut body = n["body"].clone();
        body[0]["sp"][0]["t"] = json!("Team meeting");
        let saved = call_s("save", json!({ "id": id, "body": body, "add": [{ "id": "9B1F0000-0000-4000-8000-000000000001", "uti": "com.apple.notes.table" }] }));
        assert_eq!(saved["title"], "Team meeting");
        assert!(saved["attachments"][0]["table"].is_object());
        let n2 = call_s("note", json!({ "id": id }));
        assert_eq!(n2["body"][4]["st"], "dash");
        assert_eq!(call_s("search", json!({ "q": "hall" })).as_array().unwrap().len(), 1);
        call_s("set_password", json!({ "pw": "pw1", "hint": "h" }));
        assert!(call_s("lock", json!({ "id": id, "pw": "pw1" })).get("error").is_none());
        assert!(call_s("unlock", json!({ "id": id, "pw": "nope" })).get("error").is_some());
        let u = call_s("unlock", json!({ "id": id, "pw": "pw1" }));
        assert_eq!(u["unlocked"], true);
        assert_eq!(u["title"], "Team meeting");
        assert_eq!(u["body"][0]["sp"][0]["t"], "Team meeting", "unlocked body: {}", u["body"]);
        // and again with the note already open (the editor's way)
        let u2 = call_s("unlock", json!({ "id": id, "pw": "pw1" }));
        assert_eq!(u2["body"].as_array().map(|a| a.len()), n2["body"].as_array().map(|a| a.len()));
        // edits to an opened locked note are saved and stay locked
        let mut body = u2["body"].clone();
        body[0]["sp"][0]["t"] = json!("Secret meeting");
        let saved = call_s("save", json!({ "id": id, "body": body }));
        assert!(saved.get("error").is_none(), "{saved}");
        call_s("close", json!({ "id": id }));
        let plain = call_s("note", json!({ "id": id }));
        assert_eq!(plain["locked"], true);
        assert_eq!(plain["body"].as_array().map(|a| a.len()).unwrap_or(0) <= 1, true);
        let again = call_s("unlock", json!({ "id": id, "pw": "pw1" }));
        assert_eq!(again["body"][0]["sp"][0]["t"], "Secret meeting");
        // tags renamed and taken off
        let tagged = call_s("create", json!({ "body": [{ "st": "title", "sp": [{ "t": "Tagged" }] }, { "st": "body", "sp": [{ "t": "see " }, { "t": "\u{FFFC}", "tk": { "id": "", "uti": UTI_HASHTAG, "alt": "#Trip", "content": "" } }] }] }));
        let tid = tagged["id"].as_str().unwrap().to_string();
        call_s("close", json!({ "id": tid }));
        assert_eq!(call_s("rename_tag", json!({ "from": "trip", "to": "Travel" }))["notes"], 1);
        assert_eq!(call_s("note", json!({ "id": tid }))["body"][1]["sp"][1]["tk"]["alt"], "#Travel");
        assert_eq!(call_s("delete_tag", json!({ "name": "#travel" }))["notes"], 1);
        let plain = call_s("note", json!({ "id": tid }));
        assert!(plain["body"][1]["sp"].to_string().contains("#Travel") && plain["body"][1]["sp"][1].get("tk").is_none(), "{}", plain["body"]);
        let q = call_s("smart_parse", json!({ "json": smart::to_json(&smart::Query { any: true, rules: vec![(smart::TAGS, "work".into())] }) }));
        assert_eq!(q["any"], true);
        // the Export sheet: each note its own file, Markdown's front matter with the folder's path
        let out = dir.join("export-out");
        let ids = json!([id, tid]);
        let r = call_s("export_files", json!({ "ids": ids, "format": "md", "data": true, "dir": out }));
        let files = r["files"].as_array().unwrap();
        assert_eq!(files.len(), 2, "{r}");
        let md = std::fs::read_to_string(files[0].as_str().unwrap()).unwrap();
        assert!(md.starts_with("---\ntitle: \"Secret meeting\"\n") && md.contains("folder: \"Notes\"\n"), "{md}");
        let r = call_s("export_files", json!({ "ids": ids, "format": "html", "sections": "collapse", "dir": dir.join("export-html") }));
        assert!(r["files"][0].as_str().unwrap().ends_with("Secret meeting.html"));
        let r = call_s("export_files", json!({ "ids": ids, "format": "pdf", "sections": "shown" }));
        assert_eq!(r["notes"][1]["title"], "Tagged");
        // the whole library as a .bnotes, into another library
        let notes = call_s("library", json!({}))["notes"].as_array().unwrap().len();
        let bn = dir.join("all.bnotes");
        assert_eq!(call_s("export_bnotes", json!({ "path": bn }))["notes"], notes);
        let other = dir.join("other");
        assert!(call_s("open", json!({ "dir": other }))["migration"].is_null());
        let r = call_s("import_bnotes", json!({ "path": bn }));
        assert_eq!(r["notes"], notes, "{r}");
        assert_eq!(call_s("library", json!({}))["notes"].as_array().unwrap().len(), notes);
        assert!(call_s("refresh", json!({})).get("error").is_none());
        // an old-layout library (the database alone) converts on opening, with a backup
        let old = dir.join("old");
        {
            let mut st = Store::open(&old).unwrap();
            st.create_note(None, &mut Body { paragraphs: vec![Paragraph::plain(ParaStyle::Title, "Old one")] }).unwrap();
        }
        let r = call_s("open", json!({ "dir": old }));
        assert_eq!(r["migration"]["notes"], 1, "{r}");
        assert!(Path::new(r["migration"]["backup"].as_str().unwrap()).exists());
        assert_eq!(call_s("library", json!({}))["notes"][0]["title"], "Old one");
        assert!(call_s("open", json!({ "dir": old }))["migration"].is_null());
        // a Mac's iCloud kept as an account: a top-level folder made in it stays in it
        let two = dir.join("two");
        {
            let mut st = Store::open_library(&two).unwrap();
            let f = |id: &str, acct: &str, name: &str| Folder { id: id.into(), account: acct.into(), name: name.into(), default: id.starts_with("DefaultFolder-"), ..Default::default() };
            let lib = Library {
                accounts: vec![Account { id: "ICLOUD-1".into(), name: "iCloud".into(), kind: 1 }],
                folders: vec![f("DefaultFolder-ICLOUD-1", "ICLOUD-1", "Notes"), f("WORK", "ICLOUD-1", "Work")],
                ..Default::default()
            };
            st.import_library(&lib).unwrap();
        }
        call_s("open", json!({ "dir": two }));
        let lib = call_s("library", json!({}));
        let icloud = lib["accounts"].as_array().unwrap().iter().find(|a| a["name"] == "iCloud").unwrap()["id"].as_str().unwrap().to_string();
        let made = call_s("create_folder", json!({ "name": "Later", "account": icloud }))["id"].as_str().unwrap().to_string();
        let mine = call_s("create_folder", json!({ "name": "Mine" }))["id"].as_str().unwrap().to_string();
        let lib = call_s("library", json!({}));
        let acct_of = |id: &str| lib["folders"].as_array().unwrap().iter().find(|f| f["id"] == id).unwrap()["account"].as_str().unwrap().to_string();
        assert_eq!(acct_of(&made), icloud);
        assert_ne!(acct_of(&mine), icloud);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
