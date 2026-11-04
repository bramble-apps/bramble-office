//! Exports against Notes' own Markdown export of the research notes.

mod common;

use nt_core::export::{self, Format};
use nt_core::import;
use nt_core::model::*;

fn library(name: &str) -> Option<(std::path::PathBuf, Library)> {
    let r = common::research()?;
    let lib = import::read_library(&common::group_container(&r, name)).unwrap();
    Some((r, lib))
}

fn note<'a>(lib: &'a Library, title: &str) -> &'a Note {
    lib.notes.iter().find(|n| n.title == title && !n.deleted).unwrap_or_else(|| panic!("note {title}"))
}

#[test]
fn markdown_matches_notes_export_exactly() {
    let Some((r, lib)) = library("export-md") else { return eprintln!("skipped: research not found") };
    let dir = r.join("sample-files/exports/markdown");
    let mut checked = 0;
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "md") {
            continue;
        }
        let title = p.file_stem().unwrap().to_string_lossy().into_owned();
        let want = std::fs::read_to_string(&p).unwrap();
        let got = export::markdown(note(&lib, &title));
        assert_eq!(got.text, want, "{title}");
        // the attachment files it names, with Notes' bytes
        for (rel, src) in &got.files {
            assert_eq!(std::fs::read(src).unwrap(), std::fs::read(dir.join(rel)).unwrap(), "{title}: {rel}");
        }
        checked += 1;
    }
    assert_eq!(checked, 35);
}

#[test]
fn markdown_lists_and_styles() {
    let mut b = Body::from_lines("T", &[]);
    let p = |style, indent, text: &str| Paragraph { style, indent, spans: vec![Span::plain(text)], ..Default::default() };
    b.paragraphs.push(p(ParaStyle::Numbered, 0, "a"));
    b.paragraphs.push(p(ParaStyle::Numbered, 1, "a1"));
    b.paragraphs.push(p(ParaStyle::Numbered, 1, "a2"));
    b.paragraphs.push(p(ParaStyle::Numbered, 0, "b"));
    b.paragraphs.push(p(ParaStyle::Monostyled, 0, "x = 1"));
    b.paragraphs.push(p(ParaStyle::Monostyled, 0, "y = 2"));
    b.paragraphs.push(Paragraph {
        spans: vec![Span { text: "bi".into(), bold: true, italic: true, ..Default::default() }, Span { text: "s".into(), strike: true, link: Some("u".into()), ..Default::default() }],
        ..Default::default()
    });
    let n = Note { title: "T".into(), body: b, ..Default::default() };
    assert_eq!(export::markdown(&n).text, "# T  \n1. a  \n    1. a1  \n    2. a2  \n2. b  \n```\nx = 1\ny = 2\n```\n***bi***[~~s~~](u)  \n");
}

#[test]
fn plain_text_and_html() {
    let Some((_, lib)) = library("export-text") else { return };
    assert_eq!(export::plain_text(note(&lib, "09 Checklist")), "09 Checklist\n☐ Hello\n☑ world\n");
    assert_eq!(export::plain_text(note(&lib, "19 Table")), "19 Table\nA1\tB1\nA2\t\n");
    assert_eq!(export::plain_text(note(&lib, "21 Tag")), "21 Tag\nHello #brambletest world\n");
    assert_eq!(export::plain_text(note(&lib, "28 Image")), "28 Image\nHello\n[bramble-gradient.png]\n[bramble-gradient.png]\n");

    let h = export::html(note(&lib, "10 Bold")).text;
    assert!(h.contains("<title>10 Bold</title>") && h.contains("<h1>10 Bold</h1>") && h.contains("<p>Hello <b>world</b></p>"), "{h}");
    let h = export::html(note(&lib, "18 Nested List")).text;
    assert!(h.contains("<ul>\n<li>Hello</li>\n<ul>\n<li>world</li>\n</ul>\n</ul>"), "{h}");
    let h = export::html(note(&lib, "09 Checklist")).text;
    assert!(h.contains("<ul class=\"checklist\">\n<li>Hello</li>\n<li class=\"checked\">world</li>\n</ul>"), "{h}");
    let t = export::html(note(&lib, "19 Table")).text;
    assert!(t.contains("<table><tr><td>A1</td><td>B1</td></tr><tr><td>A2</td><td></td></tr></table>"), "{t}");
    let i = export::html(note(&lib, "28 Image"));
    assert!(i.text.contains("<img src=\"Attachments/EB5DCE00-5ED3-4664-ACDD-19C1D0C64547.png\""));
    assert_eq!(i.files.len(), 1);
    let l = export::html(note(&lib, "25 Note Link")).text;
    assert!(l.contains("<a href=\"applenotes://showNote?identifier=E522CCA8-68D9-466F-814B-AC5B4FA9DF10&amp;paragraphID=9ECD1474-B6C4-4712-BBCE-84816D403BDE\">B-Target Zqx → Zqx Section</a>"), "{l}");
    assert!(export::html(note(&lib, "34 Highlight Colors")).text.contains("<mark class=\"hl5\">blue</mark>"));
    assert!(export::html(note(&lib, "08 Block Quote")).text.contains("<blockquote><h1>Hello world</h1></blockquote>"));
    assert!(export::html(note(&lib, "16 Center")).text.contains("<p style=\"text-align:center\">Hello world</p>"));
}

#[test]
fn write_folder_like_notes() {
    let Some((r, lib)) = library("export-write") else { return };
    let out = common::scratch("export-out");
    let notes: Vec<&Note> = ["28 Image", "29 PDF", "01 Plain"].iter().map(|t| note(&lib, t)).collect();
    let files = export::write_notes(&notes, &out, Format::Markdown).unwrap();
    assert_eq!(files.len(), 3);
    assert_eq!(std::fs::read_to_string(out.join("29 PDF.md")).unwrap(), std::fs::read_to_string(r.join("sample-files/exports/markdown/29 PDF.md")).unwrap());
    assert!(out.join("Attachments/EB5DCE00-5ED3-4664-ACDD-19C1D0C64547.png").is_file());
    assert!(out.join("Attachments/0C2A7602-08D5-4697-9FF3-7B2EE9373434.pdf").is_file());
    // the same title twice gets a number
    let twice = export::write_notes(&[notes[2], notes[2]], &common::scratch("export-twice"), Format::PlainText).unwrap();
    assert_eq!(twice.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>(), ["01 Plain.txt", "01 Plain 2.txt"]);
    assert!(export::write_notes(&notes, &common::scratch("export-html"), Format::Html).unwrap()[0].ends_with("28 Image.html"));
}
