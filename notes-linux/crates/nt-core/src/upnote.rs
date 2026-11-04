//! Importing an UpNote library (File > Import > UpNote Library…): its notes, with their styles,
//! lists, checklists, tables, pictures and tags, into a folder "UpNote" on this device.
//!
//! UpNote keeps its notes in `~/.config/UpNote/upnote.sqlite3` (table `notes`: `html`, `title`,
//! `pinned`, `trashed`, `createdAt`/`updatedAt` in ms, `tagLinks`) and pictures in `images/`,
//! served to its editor as `http://localhost:9425/images/<name>`. Its daily backups
//! (`UpNote Backup/<user>/data/*.upnx`) hold the same records, gzipped, one JSON object per line.
//! Both are only read (the database from a copy: UpNote may be running).
//!
//! Each note gets an id made from its UpNote id, and the ids taken in are remembered in the library
//! (`upnote-import.json`), so importing again only adds what's new. Trashed notes go to Recently
//! Deleted. Pictures that aren't on this computer, and attached files (UpNote keeps those online),
//! become link cards.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::Digest;

use crate::model::*;
use crate::store::{Store, ACCOUNT_ID, TRASH_FOLDER};
use crate::{Error, Result};

/// The folder UpNote's notes go in.
pub const FOLDER: &str = "UpNote";

/// One UpNote note, as read.
#[derive(Clone, Debug, Default)]
pub struct UpNoteNote {
    pub id: String,
    pub html: String,
    /// UpNote's own plain text of it (for checking what came in)
    pub text: String,
    pub title: String,
    pub pinned: bool,
    pub trashed: bool,
    /// ms since 1970
    pub created: f64,
    pub updated: f64,
    pub tags: Vec<String>,
}

/// An UpNote library, as read.
#[derive(Clone, Debug, Default)]
pub struct UpNote {
    pub notes: Vec<UpNoteNote>,
    /// its `images/` folder, when found
    pub images: Option<PathBuf>,
    /// file id -> (name, where UpNote keeps it online)
    pub files: HashMap<String, (String, String)>,
}

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpNoteImport {
    pub notes: usize,
    /// of them, into Recently Deleted (trashed in UpNote)
    pub trashed: usize,
    /// already imported before
    pub skipped: usize,
    pub pictures: usize,
    /// pictures and files not on this computer: link cards
    pub links: usize,
    pub tables: usize,
}

// ------------------------------------------------------------------ reading

/// Read an UpNote library: its folder (`~/.config/UpNote`), its `upnote.sqlite3`, or a `.upnx`
/// backup.
pub fn read(path: &Path) -> Result<UpNote> {
    let (db, upnx) = if path.is_dir() {
        (Some(path.join("upnote.sqlite3")).filter(|p| p.is_file()), None)
    } else if path.extension().is_some_and(|e| e == "upnx") {
        (None, Some(path.to_path_buf()))
    } else {
        (Some(path.to_path_buf()), None)
    };
    let mut up = match (db, upnx) {
        (Some(db), _) => read_db(&db)?,
        (_, Some(x)) => read_upnx(&x)?,
        _ => return Err(Error::NotFound("an UpNote library (upnote.sqlite3) in that folder".into())),
    };
    // the pictures: images/ beside the database, or (a backup) beside "UpNote Backup"
    up.images = path.ancestors().map(|a| a.join("images")).find(|p| p.is_dir());
    Ok(up)
}

fn read_db(db: &Path) -> Result<UpNote> {
    // a copy, with its write-ahead log: UpNote may be writing the original
    let tmp = std::env::temp_dir().join(format!("bramble-upnote-{}-{}", std::process::id(), crate::pb::new_uuid()));
    std::fs::create_dir_all(&tmp)?;
    let result = (|| -> Result<UpNote> {
        for ext in ["", "-wal", "-shm"] {
            let from = PathBuf::from(format!("{}{ext}", db.display()));
            if from.is_file() {
                std::fs::copy(&from, tmp.join(format!("upnote.sqlite3{ext}")))?;
            }
        }
        let c = rusqlite::Connection::open(tmp.join("upnote.sqlite3"))?;
        let mut up = UpNote::default();
        let mut st = c.prepare(
            "SELECT id, IFNULL(html, ''), IFNULL(title, ''), IFNULL(pinned, 0), IFNULL(trashed, 0), IFNULL(deleted, 0),
                    IFNULL(createdAt, 0), IFNULL(updatedAt, 0), IFNULL(tagLinks, '[]'), IFNULL(text, '') FROM notes",
        )?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)? != 0,
                r.get::<_, i64>(4)? != 0,
                r.get::<_, i64>(5)? != 0,
                r.get::<_, f64>(6)?,
                r.get::<_, f64>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
            ))
        })?;
        for row in rows {
            let (id, html, title, pinned, trashed, deleted, created, updated, tags, text) = row?;
            if deleted {
                continue;
            }
            let tags = serde_json::from_str::<Vec<String>>(&tags).unwrap_or_default();
            up.notes.push(UpNoteNote { id, html, text, title, pinned, trashed, created, updated, tags });
        }
        let mut st = c.prepare("SELECT id, IFNULL(name, ''), IFNULL(downloadURL, '') FROM files WHERE IFNULL(deleted, 0) = 0")?;
        for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
            let (id, name, url) = r?;
            up.files.insert(id, (name, url));
        }
        Ok(up)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn read_upnx(path: &Path) -> Result<UpNote> {
    let mut text = String::new();
    std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(std::fs::File::open(path)?), &mut text)?;
    let mut up = UpNote::default();
    for line in text.lines().skip(1) {
        let Ok(rec) = serde_json::from_str::<Value>(line) else { continue };
        let d = &rec["data"];
        let flag = |k: &str| d[k].as_bool().unwrap_or(false) || d[k].as_i64().unwrap_or(0) != 0;
        let s = |k: &str| d[k].as_str().unwrap_or_default().to_string();
        match rec["type"].as_str() {
            Some("notes") if !flag("deleted") => {
                let tags = match &d["tagLinks"] {
                    Value::String(t) => serde_json::from_str::<Vec<String>>(t).unwrap_or_default(),
                    Value::Array(a) => a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect(),
                    _ => Vec::new(),
                };
                up.notes.push(UpNoteNote {
                    id: s("id"),
                    html: s("html"),
                    text: s("text"),
                    title: s("title"),
                    pinned: flag("pinned"),
                    trashed: flag("trashed"),
                    created: d["createdAt"].as_f64().unwrap_or(0.0),
                    updated: d["updatedAt"].as_f64().unwrap_or(0.0),
                    tags,
                });
            }
            Some("files") if !flag("deleted") => {
                up.files.insert(s("id"), (s("name"), s("downloadURL")));
            }
            _ => {}
        }
    }
    Ok(up)
}

// ------------------------------------------------------------------ HTML

/// One piece of UpNote's HTML.
#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Open(String, Vec<(String, String)>),
    Close(String),
    Text(String),
}

fn entity(name: &str) -> Option<char> {
    Some(match name {
        "nbsp" => ' ',
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "#39" => '\'',
        _ => {
            let n = name.strip_prefix('#')?;
            let code = match n.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok()?,
                None => n.parse().ok()?,
            };
            return char::from_u32(code).map(|c| if c == '\u{a0}' { ' ' } else { c });
        }
    })
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        match rest.find(';').filter(|j| *j <= 10).and_then(|j| entity(&rest[1..j]).map(|c| (c, j))) {
            Some((c, j)) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out.replace('\u{a0}', " ")
}

/// Tags, their attributes and the text between, forgivingly (UpNote's editor writes tidy HTML).
fn pieces(html: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            out.push(Piece::Text(unescape(rest)));
            break;
        };
        if lt > 0 {
            out.push(Piece::Text(unescape(&rest[..lt])));
        }
        rest = &rest[lt..];
        if rest.starts_with("<!--") {
            rest = rest.find("-->").map(|e| &rest[e + 3..]).unwrap_or("");
            continue;
        }
        // the tag's end: the first '>' outside quotes
        let mut quote = None;
        let mut end = None;
        for (i, ch) in rest.char_indices().skip(1) {
            match (quote, ch) {
                (None, '"' | '\'') => quote = Some(ch),
                (Some(q), c) if c == q => quote = None,
                (None, '>') => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            out.push(Piece::Text(unescape(rest)));
            break;
        };
        let inner = &rest[1..end];
        rest = &rest[end + 1..];
        if let Some(name) = inner.strip_prefix('/') {
            out.push(Piece::Close(name.trim().to_ascii_lowercase()));
            continue;
        }
        let inner = inner.trim_end_matches('/');
        let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
        let name = inner[..name_end].to_ascii_lowercase();
        let mut attrs = Vec::new();
        let mut a = inner[name_end..].trim_start();
        while !a.is_empty() {
            let k_end = a.find(|c: char| c == '=' || c.is_whitespace()).unwrap_or(a.len());
            let key = a[..k_end].to_ascii_lowercase();
            a = a[k_end..].trim_start();
            let mut value = String::new();
            if let Some(v) = a.strip_prefix('=') {
                let v = v.trim_start();
                let (val, after) = match v.chars().next() {
                    Some(q @ ('"' | '\'')) => match v[1..].find(q) {
                        Some(e) => (&v[1..1 + e], &v[2 + e..]),
                        None => (&v[1..], ""),
                    },
                    _ => {
                        let e = v.find(char::is_whitespace).unwrap_or(v.len());
                        (&v[..e], &v[e..])
                    }
                };
                value = unescape(val);
                a = after.trim_start();
            }
            if !key.is_empty() {
                attrs.push((key, value));
            }
        }
        out.push(Piece::Open(name, attrs));
    }
    out
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn has_class(attrs: &[(String, String)], class: &str) -> bool {
    attr(attrs, "class").is_some_and(|c| c.split_whitespace().any(|x| x == class))
}

/// The colours UpNote offers, as Notes' text colours / highlights.
fn text_color(name: &str) -> Option<[f32; 4]> {
    Some(match name {
        "red" => [0.93, 0.26, 0.21, 1.0],
        "orange" => [0.96, 0.56, 0.13, 1.0],
        "yellow" => [0.85, 0.68, 0.0, 1.0],
        "green" => [0.2, 0.66, 0.33, 1.0],
        "blue" => [0.16, 0.47, 0.96, 1.0],
        "purple" => [0.6, 0.33, 0.83, 1.0],
        "gray" | "grey" => [0.55, 0.55, 0.57, 1.0],
        _ => return None,
    })
}

/// Notes' highlights: 1 purple, 2 pink, 3 orange, 4 mint, 5 blue.
fn highlight(name: &str) -> u8 {
    match name {
        "purple" => 1,
        "red" | "pink" => 2,
        "green" => 4,
        "blue" => 5,
        _ => 3,
    }
}

/// The character style in effect.
#[derive(Clone, Debug, Default)]
struct Inline {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    mono: bool,
    link: Option<String>,
    color: Option<[f32; 4]>,
    highlight: Option<u8>,
}

/// What an `<a>` turns into.
#[derive(Clone, Debug)]
enum Anchor {
    Link,
    /// a tag: its text becomes one hashtag
    Tag(String),
    /// a link to another UpNote note (its UpNote id)
    Note(String),
    /// an attached file (its file id, and the link)
    File(String, String),
}

/// The blocks a paragraph is in.
#[derive(Clone, Debug)]
enum Block {
    /// div, p, section parts: nothing of their own
    Plain,
    Heading(u8),
    List { numbered: bool, start: Option<u32> },
    Item { checked: Option<bool> },
    Quote,
    Pre,
}

/// Builds one body (a note's, or a table cell's).
#[derive(Default)]
struct Builder {
    paragraphs: Vec<Paragraph>,
    spans: Vec<Span>,
    blocks: Vec<Block>,
    /// a table cell's body: no title
    cell: bool,
    /// the next heading starts collapsed (an UpNote collapsed section)
    collapse_next: bool,
}

impl Builder {
    fn push_text(&mut self, text: &str, st: &Inline) {
        if text.is_empty() {
            return;
        }
        let span = Span {
            text: text.to_string(),
            bold: st.bold,
            italic: st.italic,
            underline: st.underline,
            strike: st.strike,
            link: st.link.clone(),
            highlight: st.highlight,
            color: st.color,
            font: st.mono.then(|| Font { name: Some("Menlo-Regular".into()), ..Default::default() }),
            ..Default::default()
        };
        match self.spans.last_mut() {
            Some(last) if last.same_style(&span) => last.text.push_str(text),
            _ => self.spans.push(span),
        }
    }

    fn push_token(&mut self, span: Span) {
        self.spans.push(span);
    }

    /// The paragraph so far ends (`empty`: even when it has nothing, as a `<br>` does).
    fn end_paragraph(&mut self, empty: bool) {
        if self.spans.is_empty() && !empty {
            return;
        }
        // (a paragraph of only spaces: an empty line)
        if self.spans.iter().all(|s| s.token.is_none() && s.text.trim().is_empty()) {
            self.spans.clear();
        }
        let mut p = Paragraph { spans: std::mem::take(&mut self.spans), ..Default::default() };
        let lists = self.blocks.iter().filter(|b| matches!(b, Block::List { .. })).count();
        let item = self.blocks.iter().rev().find_map(|b| match b {
            Block::Item { checked } => Some(*checked),
            _ => None,
        });
        let list = self.blocks.iter().rev().find_map(|b| match b {
            Block::List { numbered, start } => Some((*numbered, *start)),
            _ => None,
        });
        p.block_quote = self.blocks.iter().any(|b| matches!(b, Block::Quote));
        if self.blocks.iter().any(|b| matches!(b, Block::Pre)) {
            p.style = ParaStyle::Monostyled;
        } else if let Some(level) = self.blocks.iter().rev().find_map(|b| match b {
            Block::Heading(l) => Some(*l),
            _ => None,
        }) {
            p.style = if level <= 2 { ParaStyle::Heading } else { ParaStyle::Subheading };
            if self.collapse_next {
                p.collapsed = true;
                self.collapse_next = false;
            }
        } else if let (Some(checked), Some((numbered, start))) = (item, list) {
            match checked {
                Some(c) => {
                    p.style = ParaStyle::Checklist;
                    p.checked = c;
                }
                None if numbered => {
                    p.style = ParaStyle::Numbered;
                    p.start_number = start;
                }
                None => p.style = ParaStyle::Bulleted,
            }
            p.indent = lists.saturating_sub(1).min(8) as u8;
            // (only a list's first item keeps its start number)
            if let Some(Block::List { start, .. }) = self.blocks.iter_mut().rev().find(|b| matches!(b, Block::List { .. })) {
                *start = None;
            }
        }
        // the note's first paragraph is its title (UpNote's title is its first line too)
        if !self.cell && self.paragraphs.iter().all(|q| q.spans.is_empty()) && !p.spans.is_empty() {
            self.paragraphs.clear();
            p.style = ParaStyle::Title;
            p.collapsed = false;
        }
        if !self.cell && self.paragraphs.is_empty() && p.spans.is_empty() {
            // no leading empty lines
            return;
        }
        self.paragraphs.push(p);
    }

    fn finish(mut self) -> Body {
        self.end_paragraph(false);
        // no trailing empty lines
        while self.paragraphs.last().is_some_and(|p| p.spans.is_empty()) {
            self.paragraphs.pop();
        }
        Body { paragraphs: self.paragraphs }
    }
}

/// A table being read.
#[derive(Default)]
struct TableIn {
    rows: Vec<Vec<Body>>,
}

/// What a note's HTML refers to, for the import to take in.
pub(crate) struct Converted {
    pub body: Body,
    pub attachments: Vec<Attachment>,
    pub pictures: usize,
    pub links: usize,
    pub tables: usize,
}

/// UpNote's HTML as a Notes body. `images`: UpNote's pictures folder; `files`: its file records;
/// `note_id`: the id an UpNote note gets here (for links between notes).
pub(crate) fn convert(html: &str, images: Option<&Path>, files: &HashMap<String, (String, String)>, note_id: &dyn Fn(&str) -> String) -> Converted {
    let mut out = Converted { body: Body::default(), attachments: Vec::new(), pictures: 0, links: 0, tables: 0 };
    // builders: the note's, and a table cell's on top while in one
    let mut builders: Vec<Builder> = vec![Builder::default()];
    let mut tables: Vec<TableIn> = Vec::new();
    // each open element's effect, popped at its close
    enum Open {
        Inline(Inline),
        Block,
        Anchor(Anchor, Inline),
        Cell,
        Row,
        Table,
        Other,
    }
    let mut open: Vec<(String, Open)> = Vec::new();
    let mut style = Inline::default();
    // an anchor's text while it's read (tags, note links and files become one token)
    let mut captured: Option<String> = None;
    let link_card = |out: &mut Converted, name: &str, url: &str| -> Span {
        let id = crate::pb::new_uuid();
        out.attachments.push(Attachment {
            id: id.clone(),
            uti: "public.url".into(),
            kind: AttachmentKind::Url,
            name: name.to_string(),
            url: Some(url.to_string()),
            ..Default::default()
        });
        out.links += 1;
        Span::token(id, "public.url")
    };
    const BLOCKS: [&str; 15] = ["div", "p", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "blockquote", "pre", "section", "article"];
    for piece in pieces(html) {
        let b = builders.last_mut().unwrap();
        match piece {
            Piece::Text(t) => {
                let t = t.replace(['\r', '\n'], " ");
                if let Some(c) = captured.as_mut() {
                    c.push_str(&t);
                } else if !(b.spans.is_empty() && t.trim().is_empty()) {
                    // (whitespace between blocks is dropped; spaces within a line are kept)
                    b.push_text(&t, &style);
                }
            }
            Piece::Open(name, attrs) => match name.as_str() {
                "br" => b.end_paragraph(true),
                "img" => {
                    let src = attr(&attrs, "src").unwrap_or_default();
                    let file = src.rsplit('/').next().unwrap_or_default().split('?').next().unwrap_or_default().to_string();
                    let local = images.map(|d| d.join(&file)).filter(|p| !file.is_empty() && p.is_file());
                    b.end_paragraph(false);
                    let span = match local {
                        Some(p) => {
                            let id = crate::pb::new_uuid();
                            let uti = crate::store::uti_for(&p).to_string();
                            out.attachments.push(Attachment {
                                id: id.clone(),
                                kind: AttachmentKind::from_uti(&uti),
                                uti: uti.clone(),
                                name: file.clone(),
                                file: Some(p),
                                ..Default::default()
                            });
                            out.pictures += 1;
                            Span::token(id, uti)
                        }
                        None => {
                            // not here: a card linking where UpNote keeps it
                            let stem = file.rsplit_once('.').map(|(s, e)| format!("{s}__{}", e.to_ascii_lowercase())).unwrap_or_default();
                            let url = files.get(&stem).map(|f| f.1.clone()).filter(|u| !u.is_empty()).unwrap_or_else(|| src.to_string());
                            link_card(&mut out, &file, &url)
                        }
                    };
                    let b = builders.last_mut().unwrap();
                    b.push_token(span);
                    b.end_paragraph(false);
                }
                "hr" => {
                    b.end_paragraph(false);
                    let mut t = Span::token("", UTI_DIVIDER);
                    t.token.as_mut().unwrap().alt = "---".into();
                    b.push_token(t);
                    b.end_paragraph(false);
                }
                "table" => {
                    b.end_paragraph(false);
                    tables.push(TableIn::default());
                    open.push((name, Open::Table));
                }
                "tr" if !tables.is_empty() => {
                    tables.last_mut().unwrap().rows.push(Vec::new());
                    open.push((name, Open::Row));
                }
                "td" | "th" if !tables.is_empty() => {
                    builders.push(Builder { cell: true, ..Default::default() });
                    let mut st = style.clone();
                    st.bold |= name == "th";
                    open.push((name, Open::Inline(std::mem::replace(&mut style, st))));
                    open.push(("cell".into(), Open::Cell));
                }
                "a" => {
                    let href = attr(&attrs, "href").unwrap_or_default().to_string();
                    let anchor = if attr(&attrs, "data-upnote-tag").is_some() || href.starts_with("upnote://x-callback-url/tag") {
                        Anchor::Tag(attr(&attrs, "data-upnote-tag").or(attr(&attrs, "title")).unwrap_or_default().to_string())
                    } else if let Some(n) = attr(&attrs, "data-note-id") {
                        Anchor::Note(n.to_string())
                    } else if let Some(f) = attr(&attrs, "data-file-id") {
                        Anchor::File(f.to_string(), href.clone())
                    } else {
                        Anchor::Link
                    };
                    let saved = style.clone();
                    match &anchor {
                        Anchor::Link if !href.is_empty() && !href.starts_with("upnote:") => style.link = Some(href),
                        Anchor::Link => {}
                        _ => captured = Some(String::new()),
                    }
                    open.push((name, Open::Anchor(anchor, saved)));
                }
                _ if BLOCKS.contains(&name.as_str()) => {
                    b.end_paragraph(false);
                    let block = match name.as_str() {
                        "h1" | "h2" => Block::Heading(2),
                        "h3" | "h4" | "h5" | "h6" => Block::Heading(3),
                        "ul" | "ol" => Block::List { numbered: name == "ol", start: attr(&attrs, "start").and_then(|s| s.parse().ok()) },
                        "li" => Block::Item { checked: attr(&attrs, "data-checked").map(|c| c == "true") },
                        "blockquote" => Block::Quote,
                        "pre" => Block::Pre,
                        _ => {
                            if has_class(&attrs, "shine-collapsible-section") && has_class(&attrs, "shine-section-collapsed") {
                                b.collapse_next = true;
                            }
                            Block::Plain
                        }
                    };
                    b.blocks.push(block);
                    open.push((name, Open::Block));
                }
                _ => {
                    let mut st = style.clone();
                    match name.as_str() {
                        "b" | "strong" => st.bold = true,
                        "i" | "em" => st.italic = true,
                        "u" | "ins" => st.underline = true,
                        "s" | "strike" | "del" => st.strike = true,
                        "code" | "tt" | "kbd" => st.mono = true,
                        "sup" | "sub" | "mark" | "span" | "font" => {}
                        // (col, colgroup, tbody, thead…: nothing of their own)
                        _ => {
                            open.push((name, Open::Other));
                            continue;
                        }
                    }
                    if name == "mark" {
                        st.highlight = Some(3);
                    }
                    for class in attr(&attrs, "class").unwrap_or_default().split_whitespace() {
                        if let Some(c) = class.strip_prefix("shine-text-") {
                            st.color = text_color(c).or(st.color);
                        } else if class == "shine-highlight" {
                            st.highlight = Some(3);
                        } else if let Some(c) = class.strip_prefix("shine-highlight-") {
                            st.highlight = Some(highlight(c));
                        }
                    }
                    open.push((name, Open::Inline(std::mem::replace(&mut style, st))));
                }
            },
            Piece::Close(name) => {
                // up to the matching open element (a missing close closes what's inside it)
                let Some(at) = open.iter().rposition(|(n, _)| *n == name || (name == "td" || name == "th") && n == "cell") else { continue };
                while open.len() > at {
                    let (_, what) = open.pop().unwrap();
                    match what {
                        Open::Inline(saved) => style = saved,
                        Open::Block => {
                            let b = builders.last_mut().unwrap();
                            b.end_paragraph(false);
                            b.blocks.pop();
                        }
                        Open::Anchor(anchor, saved) => {
                            style = saved;
                            let text = captured.take().unwrap_or_default();
                            let b = builders.last_mut().unwrap();
                            match anchor {
                                Anchor::Link => {}
                                Anchor::Tag(name) => {
                                    let tag = text.trim().trim_start_matches('#').to_string();
                                    let tag = if tag.is_empty() { name.trim_start_matches('#').to_string() } else { tag };
                                    if !tag.is_empty() {
                                        let mut t = Span::token("", UTI_HASHTAG);
                                        t.token.as_mut().unwrap().alt = format!("#{tag}");
                                        b.push_token(t);
                                    }
                                }
                                Anchor::Note(n) => {
                                    let mut t = Span::token("", UTI_NOTE_LINK);
                                    let tok = t.token.as_mut().unwrap();
                                    tok.alt = text.trim().to_string();
                                    tok.content = Token::note_link_url(&note_id(&n), None);
                                    b.push_token(t);
                                }
                                Anchor::File(id, href) => {
                                    let (name, url) = files.get(&id).cloned().unwrap_or_else(|| (text.trim().to_string(), href));
                                    let name = if name.is_empty() { text.trim().to_string() } else { name };
                                    b.end_paragraph(false);
                                    let span = link_card(&mut out, &name, &url);
                                    let b = builders.last_mut().unwrap();
                                    b.push_token(span);
                                    b.end_paragraph(false);
                                }
                            }
                        }
                        Open::Cell => {
                            if builders.len() > 1 {
                                let cell = builders.pop().unwrap().finish();
                                if let Some(row) = tables.last_mut().and_then(|t| t.rows.last_mut()) {
                                    row.push(cell);
                                }
                            }
                        }
                        Open::Row => {}
                        Open::Table => {
                            let t = tables.pop().unwrap_or_default();
                            let cols = t.rows.iter().map(Vec::len).max().unwrap_or(0);
                            if cols > 0 {
                                let mut rows = t.rows;
                                rows.retain(|r| !r.is_empty());
                                for r in &mut rows {
                                    r.resize(cols, Body::default());
                                }
                                let table = Table { rows, rtl: false };
                                let id = crate::pb::new_uuid();
                                out.attachments.push(Attachment {
                                    id: id.clone(),
                                    uti: UTI_TABLE.into(),
                                    kind: AttachmentKind::Table,
                                    summary: table.summary(),
                                    table: Some(table),
                                    ..Default::default()
                                });
                                out.tables += 1;
                                let b = builders.last_mut().unwrap();
                                b.end_paragraph(false);
                                b.push_token(Span::token(id, UTI_TABLE));
                                b.end_paragraph(false);
                            }
                        }
                        Open::Other => {}
                    }
                }
            }
        }
    }
    // (cells left open: closed)
    while builders.len() > 1 {
        builders.pop();
    }
    out.body = builders.pop().unwrap().finish();
    out
}

// ------------------------------------------------------------------ importing

/// The id an UpNote note gets here: made from its UpNote id (the same every time).
pub fn note_id(upnote_id: &str) -> String {
    let h = sha2::Sha256::digest(format!("upnote:{upnote_id}").as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&h[..16]);
    // (a version-4-shaped UUID, as Notes' are)
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(b).hyphenated().encode_upper(&mut uuid::Uuid::encode_buffer()).to_string()
}

/// The UpNote ids taken in before (`upnote-import.json` in the library folder).
fn imported(store: &Store) -> HashSet<String> {
    std::fs::read(store.dir().join("upnote-import.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

/// The notes here that came from UpNote (by their ids).
pub fn imported_note_ids(store: &Store) -> HashSet<String> {
    imported(store).iter().map(|i| note_id(i)).collect()
}

/// The note an UpNote note becomes (in `folder`).
pub(crate) fn to_note(n: &UpNoteNote, up: &UpNote, folder: &str) -> (Note, Converted) {
    let mut c = convert(&n.html, up.images.as_deref(), &up.files, &note_id);
    if c.body.paragraphs.is_empty() && !n.title.is_empty() {
        c.body.paragraphs.push(Paragraph::plain(ParaStyle::Title, &n.title));
    }
    // its tags not written in its text: at the end, as Notes keeps them
    let have: HashSet<String> = c.body.tokens().filter(|t| t.uti == UTI_HASHTAG).map(|t| t.alt.trim_start_matches('#').to_lowercase()).collect();
    let missing: Vec<&String> = n.tags.iter().filter(|t| !t.trim().is_empty() && !have.contains(&t.trim().trim_start_matches('#').to_lowercase())).collect();
    if !missing.is_empty() {
        let mut p = Paragraph::default();
        for (i, t) in missing.iter().enumerate() {
            if i > 0 {
                p.spans.push(Span::plain(" "));
            }
            let mut s = Span::token("", UTI_HASHTAG);
            s.token.as_mut().unwrap().alt = format!("#{}", t.trim().trim_start_matches('#').replace(' ', "-"));
            p.spans.push(s);
        }
        c.body.paragraphs.push(p);
    }
    let title = c.body.paragraphs.first().map(|p| p.text().replace(OBJ, "").trim().to_string()).unwrap_or_default();
    let note = Note {
        id: note_id(&n.id),
        folder: if n.trashed { TRASH_FOLDER.into() } else { folder.to_string() },
        title,
        created: n.created / 1000.0,
        modified: n.updated.max(n.created) / 1000.0,
        pinned: n.pinned,
        deleted: n.trashed,
        trashed_at: n.trashed.then_some(n.updated / 1000.0),
        body: std::mem::take(&mut c.body),
        attachments: std::mem::take(&mut c.attachments),
        ..Default::default()
    };
    (note, c)
}

/// The folder "UpNote" on this device (made when missing).
fn folder(store: &mut Store) -> Result<String> {
    let lib = store.library_meta()?;
    if let Some(f) = lib.folders.iter().find(|f| f.name == FOLDER && f.account == ACCOUNT_ID && f.parent.is_none() && f.smart.is_none()) {
        return Ok(f.id.clone());
    }
    store.create_folder_in(ACCOUNT_ID, FOLDER)
}

/// Import an UpNote library (see [`read`] for `path`) into folder "UpNote"; notes imported before
/// are left as they are.
pub fn import(store: &mut Store, path: &Path) -> Result<UpNoteImport> {
    let up = read(path)?;
    import_from(store, &up)
}

pub fn import_from(store: &mut Store, up: &UpNote) -> Result<UpNoteImport> {
    let mut rep = UpNoteImport::default();
    let mut done = imported(store);
    let here = store.change_keys()?;
    let todo: Vec<&UpNoteNote> = up.notes.iter().filter(|n| !done.contains(&n.id) && !here.contains_key(&note_id(&n.id))).collect();
    rep.skipped = up.notes.len() - todo.len();
    if todo.is_empty() {
        return Ok(rep);
    }
    let folder = folder(store)?;
    let mut notes = Vec::new();
    for n in &todo {
        let (note, c) = to_note(n, up, &folder);
        rep.pictures += c.pictures;
        rep.links += c.links;
        rep.tables += c.tables;
        rep.trashed += usize::from(note.deleted);
        notes.push(note);
    }
    rep.notes = notes.len();
    store.put_synced_notes(&notes)?;
    done.extend(todo.iter().map(|n| n.id.clone()));
    let mut list: Vec<&String> = done.iter().collect();
    list.sort();
    crate::native::write_atomic(&store.dir().join("upnote-import.json"), serde_json::to_string(&list).unwrap_or_default().as_bytes())?;
    Ok(rep)
}

/// How much of each UpNote note's own plain text came in, by words in order (0…1), for the notes
/// here from `up`: a check of the import that reads no note to anyone.
pub fn fidelity(store: &Store, up: &UpNote) -> Result<Vec<(String, f64)>> {
    let mut out = Vec::new();
    for n in up.notes.iter().filter(|n| !n.text.trim().is_empty()) {
        let Ok(here) = store.note(&note_id(&n.id)) else { continue };
        let mut vocab: HashMap<String, u32> = HashMap::new();
        let mut ids = |w: Vec<String>| -> Vec<u32> {
            w.into_iter().map(|w| {
                let k = vocab.len() as u32;
                *vocab.entry(w).or_insert(k)
            }).collect()
        };
        let theirs = ids(n.text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect());
        let mine = ids(words(&here));
        out.push((n.id.clone(), if theirs.is_empty() { 1.0 } else { common(&theirs, &mine) as f64 / theirs.len() as f64 }));
    }
    Ok(out)
}

// ------------------------------------------------------------------ duplicates

/// How alike two notes must be (by their words, in order) to count as the same note.
pub const SIMILAR: f64 = 0.9;

/// A note here (not from UpNote) that an UpNote note repeats.
#[derive(Clone, Debug, PartialEq)]
pub struct Duplicate {
    pub note: String,
    pub title: String,
    /// the UpNote note it repeats
    pub upnote: String,
    /// 0…1 by words in order (1: the same words)
    pub similarity: f64,
    /// None: the UpNote note has all of it (safe to move to Recently Deleted); Some: why it's kept
    pub keep: Option<String>,
}

/// A note's words for comparing: lower case, no punctuation, no attachment placeholders; its
/// tags, links to notes, tables' cells and link cards' names count as words.
fn words(n: &Note) -> Vec<String> {
    let mut text = String::new();
    for p in &n.body.paragraphs {
        for s in &p.spans {
            match &s.token {
                // tags and links to notes by their text, tables by their cells, link cards by their name
                Some(t) if t.is_inline() => {
                    text.push(' ');
                    text.push_str(&t.alt);
                    text.push(' ');
                }
                Some(t) => match n.attachment(&t.id) {
                    Some(a) if a.table.is_some() => {
                        text.push(' ');
                        text.push_str(&a.table.as_ref().unwrap().summary());
                    }
                    Some(a) if a.kind == AttachmentKind::Url => {
                        text.push(' ');
                        text.push_str(&a.name);
                        text.push(' ');
                    }
                    _ => {}
                },
                None => text.push_str(&s.text),
            }
        }
        text.push('\n');
    }
    text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// The longest run of `a`'s words found in `b` in order (not necessarily side by side).
fn common(a: &[u32], b: &[u32]) -> usize {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let mut prev = vec![0u32; b.len() + 1];
    let mut cur = vec![0u32; b.len() + 1];
    for x in a {
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = if x == y { prev[j] + 1 } else { cur[j].max(prev[j + 1]) };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()] as usize
}

/// What's in a note that isn't text: pictures, and the kinds UpNote can't hold (drawings, scans,
/// PDFs, audio, other files).
fn media(n: &Note) -> (usize, usize) {
    let kinds: Vec<AttachmentKind> = n.body.tokens().filter(|t| !t.is_inline()).filter_map(|t| n.attachment(&t.id)).map(|a| a.kind).collect();
    let pictures = kinds.iter().filter(|k| **k == AttachmentKind::Image).count();
    let other = kinds.iter().filter(|k| matches!(k, AttachmentKind::Drawing | AttachmentKind::Scan | AttachmentKind::Pdf | AttachmentKind::Audio | AttachmentKind::File)).count();
    (pictures, other)
}

/// The notes outside `upnote` (and Recently Deleted) that one of the `upnote` notes repeats,
/// ≥ [`SIMILAR`] alike. A note is safe to move to Recently Deleted only when the UpNote note has
/// all of its words, in order, and at least its pictures, and it has no drawings, scans or files.
pub fn duplicates(store: &Store, upnote: &HashSet<String>) -> Result<Vec<Duplicate>> {
    let lib = store.library_meta()?;
    let live = |n: &Note| !n.deleted && !n.locked && n.folder != TRASH_FOLDER;
    // every word as a number
    let mut vocab: HashMap<String, u32> = HashMap::new();
    let mut ids = |w: Vec<String>| -> Vec<u32> {
        w.into_iter().map(|w| {
            let n = vocab.len() as u32;
            *vocab.entry(w).or_insert(n)
        }).collect()
    };
    struct Side {
        note: Note,
        words: Vec<u32>,
        set: HashSet<u32>,
        title: String,
    }
    let norm_title = |t: &str| t.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    let mut ups = Vec::new();
    let mut macs = Vec::new();
    for light in lib.notes.iter().filter(|n| live(n)) {
        let n = store.note(&light.id)?;
        let w = ids(words(&n));
        let side = Side { set: w.iter().copied().collect(), words: w, title: norm_title(&n.title), note: n };
        if upnote.contains(&light.id) {
            ups.push(side);
        } else {
            macs.push(side);
        }
    }
    let mut out = Vec::new();
    for m in &macs {
        if m.words.is_empty() {
            continue;
        }
        // the closest UpNote note: first by shared words (cheap), then by words in order
        let mut best: Option<(f64, &Side)> = None;
        for u in &ups {
            let inter = m.set.intersection(&u.set).count();
            let union = m.set.len() + u.set.len() - inter;
            let jaccard = if union == 0 { 0.0 } else { inter as f64 / union as f64 };
            if jaccard < 0.6 && !(m.title == u.title && !m.title.is_empty() && jaccard >= 0.3) {
                continue;
            }
            let lcs = common(&m.words, &u.words);
            let sim = 2.0 * lcs as f64 / (m.words.len() + u.words.len()) as f64;
            if best.is_none_or(|(s, _)| sim > s) {
                best = Some((sim, u));
            }
        }
        let Some((sim, u)) = best.filter(|(s, _)| *s >= SIMILAR) else { continue };
        let lcs = common(&m.words, &u.words);
        let (mp, mo) = media(&m.note);
        let (up, _) = media(&u.note);
        let keep = if lcs < m.words.len() {
            let extra = m.words.len() - lcs;
            Some(if m.note.modified > u.note.modified {
                format!("newer here, with {extra} words the UpNote copy doesn't have")
            } else {
                format!("{extra} words the UpNote copy doesn't have")
            })
        } else if mo > 0 {
            Some(format!("{mo} drawings, scans or files the UpNote copy can't hold"))
        } else if mp > up {
            Some(format!("{} more pictures than the UpNote copy", mp - up))
        } else {
            None
        };
        out.push(Duplicate { note: m.note.id.clone(), title: m.note.title.clone(), upnote: u.note.id.clone(), similarity: sim, keep });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(html: &str) -> Body {
        convert(html, None, &HashMap::new(), &|i| format!("ID-{i}")).body
    }

    fn lines(b: &Body) -> Vec<(ParaStyle, String)> {
        b.paragraphs.iter().map(|p| (p.style, p.text())).collect()
    }

    #[test]
    fn lines_headings_and_lists() {
        let b = body("<h2>Shopping</h2><div>Milk &amp; bread</div><div><br></div><ul><li data-checked=\"true\">eggs</li><li data-checked=\"false\">jam</li></ul><ol start=\"3\"><li>one</li><li>two<ul><li>deep</li></ul></li></ol><h3>After</h3><div>a<br>b</div>");
        assert_eq!(
            lines(&b),
            vec![
                (ParaStyle::Title, "Shopping".into()),
                (ParaStyle::Body, "Milk & bread".into()),
                (ParaStyle::Body, "".into()),
                (ParaStyle::Checklist, "eggs".into()),
                (ParaStyle::Checklist, "jam".into()),
                (ParaStyle::Numbered, "one".into()),
                (ParaStyle::Numbered, "two".into()),
                (ParaStyle::Bulleted, "deep".into()),
                (ParaStyle::Subheading, "After".into()),
                (ParaStyle::Body, "a".into()),
                (ParaStyle::Body, "b".into()),
            ]
        );
        assert!(b.paragraphs[3].checked && !b.paragraphs[4].checked);
        assert_eq!(b.paragraphs[5].start_number, Some(3));
        assert_eq!(b.paragraphs[6].start_number, None);
        assert_eq!(b.paragraphs[7].indent, 1);
    }

    #[test]
    fn styles_links_tags_and_quotes() {
        let b = body("<div>Title</div><div><b>bold</b> <i>it</i> <u>u</u> <strike>s</strike> <a href=\"https://x.org\">link</a> <code spellcheck=\"false\">c</code> <span class=\"shine-text-red\">red</span> <span class=\"shine-highlight-blue\">hi</span></div><div><a title=\"#food\" data-upnote-tag=\"food\" href=\"upnote://x-callback-url/tag/view?tag=food\">#food</a></div><blockquote><div>quoted</div></blockquote><div><a href=\"upnote://x-callback-url/openNote?noteId=N1\" data-note-id=\"N1\">Other</a></div>");
        let p = &b.paragraphs[1];
        let span = |t: &str| p.spans.iter().find(|s| s.text == t).unwrap();
        assert!(span("bold").bold && span("it").italic && span("u").underline && span("s").strike);
        assert_eq!(span("link").link.as_deref(), Some("https://x.org"));
        assert!(span("c").font.is_some());
        assert!(span("red").color.is_some());
        assert_eq!(span("hi").highlight, Some(5));
        let tag = b.paragraphs[2].spans[0].token.as_ref().unwrap();
        assert_eq!((tag.uti.as_str(), tag.alt.as_str()), (UTI_HASHTAG, "#food"));
        assert!(b.paragraphs[3].block_quote);
        let link = b.paragraphs[4].spans[0].token.as_ref().unwrap();
        assert_eq!(link.uti, UTI_NOTE_LINK);
        assert_eq!(link.content, Token::note_link_url("ID-N1", None));
        assert_eq!(link.alt, "Other");
    }

    #[test]
    fn tables_pictures_files_and_sections() {
        let dir = std::env::temp_dir().join(format!("upnote-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A.png"), b"\x89PNG").unwrap();
        let files = HashMap::from([("B__png".to_string(), ("B.png".to_string(), "https://store/B".to_string()))]);
        let c = convert(
            "<h2>T</h2><div class=\"shine-table-wrapper\"><table><colgroup><col style=\"width:1px\"></colgroup><tbody><tr><td>a</td><td><b>b</b></td></tr><tr><td>c</td></tr></tbody></table></div><img src=\"http://localhost:9425/images/A.png\" width=\"10\"><img src=\"http://localhost:9425/images/B.png\"><a data-file-id=\"F__mp3\" href=\"http://localhost:9425/files/F.mp3\">song.mp3</a><div class=\"shine-collapsible-section shine-section-collapsed\"><div class=\"shine-section-title\"><h3>Folded</h3></div><div class=\"shine-section-content\"><div>inside</div></div></div>",
            Some(&dir),
            &files,
            &|i| i.to_string(),
        );
        assert_eq!((c.tables, c.pictures, c.links), (1, 1, 2));
        let t = c.attachments.iter().find(|a| a.kind == AttachmentKind::Table).unwrap().table.as_ref().unwrap();
        assert_eq!(t.text_grid(), vec![vec!["a".to_string(), "b".into()], vec!["c".into(), "".into()]]);
        assert!(c.attachments.iter().any(|a| a.url.as_deref() == Some("https://store/B")));
        assert!(c.attachments.iter().any(|a| a.name == "song.mp3" && a.kind == AttachmentKind::Url));
        let folded = c.body.paragraphs.iter().find(|p| p.text() == "Folded").unwrap();
        assert!(folded.collapsed && folded.style == ParaStyle::Subheading);
        assert_eq!(c.body.paragraphs.last().unwrap().text(), "inside");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ids_stay_the_same() {
        assert_eq!(note_id("abc"), note_id("abc"));
        assert_ne!(note_id("abc"), note_id("abd"));
        assert_eq!(note_id("abc").len(), 36);
    }

    #[test]
    fn words_in_order() {
        assert_eq!(common(&[1, 2, 3, 4], &[1, 3, 4, 5]), 3);
        assert_eq!(common(&[], &[1]), 0);
    }
}
