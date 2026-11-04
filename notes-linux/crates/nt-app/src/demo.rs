//! The built-in demo library (until nt-core reads the app's own library): the NO1 research's
//! Bramble Research folder as it was on the Mac (data/demo.txt, made by data/make_demo.py from
//! the decoded notes), in an On My Mac account, plus an iCloud account with a few folders so
//! the sidebar shows accounts, subfolders, a Smart Folder and Quick Notes.

use nt_core::model::{Account, Body, Folder, Library, Note, ParaStyle, Paragraph, Span};

use crate::app::Thumb;

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn folder(id: &str, account: &str, name: &str, parent: Option<&str>, smart: Option<&str>) -> Folder {
    let kind = if smart.is_some() { nt_core::model::FolderKind::Smart } else { nt_core::model::FolderKind::Normal };
    Folder { id: id.into(), account: account.into(), name: name.into(), parent: parent.map(String::from), smart: smart.map(String::from), kind, default: name == "Notes" && parent.is_none(), ..Default::default() }
}

/// The demo library and its notes' thumbnails (note id, thumb).
pub fn library() -> (Library, Vec<(String, Thumb)>) {
    let mut lib = Library {
        accounts: vec![Account { id: "icloud".into(), name: "iCloud".into(), kind: 1 }, Account { id: "local".into(), name: "On My Mac".into(), kind: 3 }],
        folders: vec![
            folder("icloud-quick", "icloud", "Quick Notes", None, None),
            folder("icloud-notes", "icloud", "Notes", None, None),
            folder("icloud-recipes", "icloud", "Recipes", None, None),
            folder("icloud-travel", "icloud", "Travel", None, None),
            folder("icloud-japan", "icloud", "Japan 2026", Some("icloud-travel"), None),
            folder("icloud-tagged", "icloud", "Tagged", None, Some(r#"{"tags":["brambletest"]}"#)),
            folder("local-notes", "local", "Notes", None, None),
            folder("local-bramble", "local", "Bramble Research", None, None),
            folder("local-bsub", "local", "B-Sub", Some("local-bramble"), None),
            folder("local-smart", "local", "B-Smart Pinned", None, Some(r#"{"pinned":true}"#)),
        ],
        ..Default::default()
    };
    let mut thumbs = Vec::new();
    let gradient: std::sync::Arc<[u8]> = include_bytes!("../data/bramble-gradient.png").as_slice().into();
    let mut cur: Option<Note> = None;
    for line in include_str!("../data/demo.txt").lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first() {
            Some(&"N") if f.len() >= 9 => {
                lib.notes.extend(cur.take());
                let folder = match unesc(f[3]).as_str() {
                    "B-Sub" => "local-bsub",
                    _ => "local-bramble",
                };
                let mut snippet = unesc(f[7]);
                let title = unesc(f[2]);
                match f[8] {
                    "image" => thumbs.push((f[1].to_string(), Thumb::Image(gradient.clone()))),
                    "pdf" => thumbs.push((f[1].to_string(), Thumb::Pdf)),
                    "drawing" => {
                        thumbs.push((f[1].to_string(), Thumb::Drawing));
                        if snippet.is_empty() {
                            snippet = "Handwritten note".into();
                        }
                    }
                    _ => {}
                }
                if snippet == title {
                    snippet.clear();
                }
                cur = Some(Note {
                    id: f[1].into(),
                    folder: folder.into(),
                    title,
                    snippet,
                    created: f[4].parse().unwrap_or(0.0),
                    modified: f[5].parse().unwrap_or(0.0),
                    pinned: f[6] == "1",
                    ..Default::default()
                });
            }
            Some(&"P") if f.len() >= 6 => {
                let Some(n) = cur.as_mut() else { continue };
                let style = match f[1] {
                    "Title" => ParaStyle::Title,
                    "Heading" => ParaStyle::Heading,
                    "Subheading" => ParaStyle::Subheading,
                    "Monostyled" => ParaStyle::Monostyled,
                    "Bulleted" => ParaStyle::Bulleted,
                    "Dashed" => ParaStyle::Dashed,
                    "Numbered" => ParaStyle::Numbered,
                    "Checklist" => ParaStyle::Checklist,
                    _ => ParaStyle::Body,
                };
                n.body.paragraphs.push(Paragraph { style, indent: f[2].parse().unwrap_or(0), checked: f[3] == "1", block_quote: f[4] == "1", ..Default::default() });
            }
            Some(&"S") if f.len() >= 5 => {
                let Some(p) = cur.as_mut().and_then(|n| n.body.paragraphs.last_mut()) else { continue };
                let fl = f[1];
                p.spans.push(Span {
                    text: unesc(f[4]),
                    bold: fl.contains('b'),
                    italic: fl.contains('i'),
                    underline: fl.contains('u'),
                    strike: fl.contains('s'),
                    script: if fl.contains('^') { 1 } else if fl.contains('_') { -1 } else { 0 },
                    link: Some(unesc(f[3])).filter(|l| !l.is_empty()),
                    highlight: f[2].parse().ok(),
                    ..Default::default()
                });
            }
            _ => {}
        }
    }
    lib.notes.extend(cur.take());
    // the iCloud side: a few notes of its own (dates relative to now)
    let mk = |id: &str, folder: &str, title: &str, snippet: &str, days_ago: i64, h: u32, m: u32| {
        let t = crate::dates::today_at(h, m, days_ago);
        let body = Body {
            paragraphs: vec![
                Paragraph { style: ParaStyle::Title, spans: vec![Span { text: title.into(), ..Default::default() }], ..Default::default() },
                Paragraph { spans: vec![Span { text: snippet.into(), ..Default::default() }], ..Default::default() },
            ],
        };
        Note { id: id.into(), folder: folder.into(), title: title.into(), snippet: snippet.into(), created: t, modified: t, body, ..Default::default() }
    };
    lib.notes.push(mk("demo-welcome", "icloud-notes", "Welcome to Notes", "Write, sketch and keep lists in one place.", 0, 9, 12));
    lib.notes.push(mk("demo-groceries", "icloud-notes", "Groceries", "Milk, eggs, basil #shopping", 1, 18, 40));
    lib.notes.push(mk("demo-ideas", "icloud-notes", "Ideas", "A Linux home for Apple's notes", 4, 11, 5));
    lib.notes.push(mk("demo-month", "icloud-notes", "Book list", "The Left Hand of Darkness", 20, 8, 30));
    lib.notes.push(mk("demo-old", "icloud-notes", "Moving checklist", "Boxes, tape, labels", 400, 10, 0));
    lib.notes.push(mk("demo-bread", "icloud-recipes", "Sourdough", "500 g flour, 350 g water, 100 g starter", 9, 16, 20));
    lib.notes.push(mk("demo-kyoto", "icloud-japan", "Kyoto", "Fushimi Inari early, before the crowds", 2, 7, 45));
    lib.notes.push(mk("demo-quick", "icloud-quick", "Call back about the boiler", "Thursday after 3", 0, 8, 2));
    (lib, thumbs)
}
