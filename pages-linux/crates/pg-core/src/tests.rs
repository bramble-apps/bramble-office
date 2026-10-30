use super::*;

pub(crate) fn samples_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../pages-research-src/pages-research/sample-files/minimal")
}

pub(crate) fn sample(name: &str) -> Pages {
    Pages::open(samples_dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

pub(crate) fn all_samples() -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(samples_dir()).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pages")).collect();
    v.sort();
    v
}

pub(crate) fn out_dir() -> std::path::PathBuf {
    let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/pg-edits");
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Save a test result after checking it has no unreferenced objects or data (Pages' own
/// files have none; the Mac's P1 check).
pub(crate) fn save(p: &mut Pages, name: &str) -> std::path::PathBuf {
    let path = out_dir().join(name);
    p.save_as(&path).unwrap();
    // read back (password files can't be: they were checked before encryption)
    if p.doc.password.is_none() {
        let back = Pages::open(&path).unwrap();
        let (o, d) = gc::orphans(back.file());
        let kinds: Vec<&str> = o.iter().map(|id| back.file().type_name(*id)).collect();
        assert!(o.is_empty() && d.is_empty(), "{name}: orphans {o:?} {kinds:?}, data {d:?}");
        assert_eq!(dangling(back.file()), Vec::new(), "{name}: dangling references after saving");
        assert_eq!(dangling_payload(back.file()), Vec::new(), "{name}: references to missing objects after saving");
    }
    path
}

/// Declared references to objects that aren't in the file: (from, to). (Pages' own stylesheets
/// can name unsaved styles in their payload, never in the declarations.)
pub(crate) fn dangling(k: &kn_format::KeyFile) -> Vec<(ObjId, ObjId)> {
    let ids: std::collections::HashSet<ObjId> = k.archives().map(|(_, a)| a.id()).collect();
    let mut out = Vec::new();
    for (_, a) in k.archives() {
        for i in 0..a.payloads.len() {
            for r in a.object_references(i) {
                if !ids.contains(&r) {
                    out.push((a.id(), r));
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Payload references to objects that aren't in the file, leaving out the stylesheet (Pages'
/// own name styles it doesn't save).
pub(crate) fn dangling_payload(k: &kn_format::KeyFile) -> Vec<(ObjId, ObjId)> {
    let ids: std::collections::HashSet<ObjId> = k.archives().map(|(_, a)| a.id()).collect();
    let sheet = k.stylesheet_id();
    let mut out = Vec::new();
    for (_, a) in k.archives() {
        if Some(a.id()) == sheet {
            continue;
        }
        for pl in &a.payloads {
            let mut v = Vec::new();
            pl.collect_refs(&mut v);
            out.extend(v.into_iter().filter(|r| !ids.contains(r)).map(|r| (a.id(), r)));
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

pub(crate) fn lay(p: &Pages) -> flow::Layout {
    let mut fonts = kn_render::fonts::Fonts::new();
    let mut cache = flow::Cache::default();
    flow::layout(p, &mut fonts, &mut cache, &flow::Options::default())
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn every_sample_opens_and_resaves_losslessly() {
    let files = all_samples();
    assert!(files.len() >= 45, "samples missing: {}", files.len());
    for f in files {
        let p = Pages::open(&f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        assert!(p.root().is_some(), "{}: no TP.DocumentArchive", f.display());
        let zip = p.file().to_zip().unwrap();
        let again = Pages::from_bytes(&zip).unwrap();
        // every object survives with the same encoding
        for (name, a) in p.file().archives() {
            let b = again.file().archive(a.id()).unwrap_or_else(|| panic!("{}: object {} lost", f.display(), a.id()));
            for (x, y) in a.payloads.iter().zip(&b.payloads) {
                assert_eq!(x.encode(), y.encode(), "{}: {} {} changed", f.display(), name, a.id());
            }
        }
    }
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn every_sample_lays_out() {
    for f in all_samples() {
        let p = Pages::open(&f).unwrap();
        let l = lay(&p);
        assert!(!l.pages.is_empty(), "{}", f.display());
    }
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn setup_and_modes() {
    let p = sample("00-blank.pages");
    let s = p.setup();
    assert_eq!((s.width, s.height, s.left, s.top), (612.0, 792.0, 72.0, 72.0));
    assert!(s.body);
    assert!(!sample("00-blank-layout.pages").setup().body);
    let l = sample("14-landscape.pages").setup();
    assert!(l.landscape && l.width > l.height);
    assert!(sample("09-facing-pages.pages").setup().facing_pages);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn paragraphs_and_breaks() {
    let p = sample("02-paragraphs.pages");
    let l = lay(&p);
    let b = l.body.as_ref().unwrap();
    assert_eq!(b.tb.paragraphs.len(), 3);
    assert_eq!(&b.tb.text[b.tb.paragraphs[1].start..b.tb.paragraphs[1].end], "Para two");
    // page break: two pages
    let p = sample("10-page-break.pages");
    let l = lay(&p);
    assert_eq!(l.pages.len(), 2, "page break gives two pages");
    // section break: two sections, second page in section 1
    let p = sample("26-section-break.pages");
    assert_eq!(p.sections().len(), 2);
    let l = lay(&p);
    assert_eq!(l.pages.len(), 2);
    assert_eq!(l.pages[1].section, 1);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn column_break_two_columns() {
    let p = sample("48-column-break-2col.pages");
    let l = lay(&p);
    assert_eq!(l.columns.len(), 2);
    assert_eq!(l.pages.len(), 1);
    let cols: Vec<usize> = l.pages[0].lines.iter().map(|x| x.column).collect();
    assert!(cols.contains(&0) && cols.contains(&1), "{cols:?}");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn header_footer_and_page_number() {
    let p = sample("24-header-footer.pages");
    let l = lay(&p);
    assert!(l.pages[0].headers.iter().any(|h| !h.piece.tl.glyphs.is_empty()), "header text shown");
    let p = sample("25-page-number-field.pages");
    let l = lay(&p);
    let b = l.body.as_ref().unwrap();
    assert!(b.tb.text.starts_with("Page "), "{:?}", b.tb.text);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn footnote_shown_at_page_bottom() {
    let p = sample("18-footnote.pages");
    let l = lay(&p);
    let b = l.body.as_ref().unwrap();
    assert!(b.tb.text.starts_with("Text with a footnote1"), "{:?}", b.tb.text);
    assert_eq!(l.pages[0].footnotes.len(), 1);
    assert!(l.pages[0].footnotes[0].dy > 600.0);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn caret_and_hit_round_trip() {
    let p = sample("02-paragraphs.pages");
    let l = lay(&p);
    let b = l.body.as_ref().unwrap();
    for s in [0, 3, b.tb.paragraphs[1].start, b.tb.paragraphs[2].end] {
        let (page, x, top, h) = l.caret(s).unwrap();
        let back = l.hit(page, x + 0.1, top + h / 2.0).unwrap();
        assert_eq!(back, s, "offset {s}");
    }
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn long_text_flows_to_more_pages() {
    let mut p = sample("00-blank.pages");
    let body = p.body_storage().unwrap();
    let para = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.\n";
    let mut h = kn_model::edit::History::default();
    p.doc.begin(&mut h, "Typing");
    p.doc.replace_text(&mut h, body, 0, 0, &para.repeat(120));
    p.doc.end(&mut h);
    let l = lay(&p);
    assert!(l.pages.len() >= 4, "{} pages", l.pages.len());
    let placed: usize = l.pages.iter().map(|pg| pg.lines.len()).sum();
    assert!(placed >= 120);
    for w in l.pages.windows(2) {
        assert!(w[0].start <= w[1].start);
    }
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn toc_and_drop_cap_and_lists() {
    let l = lay(&sample("19-toc.pages"));
    let toc: Vec<_> = l.pages.iter().flat_map(|p| p.toc.iter()).collect();
    assert_eq!(toc.len(), 1);
    assert_eq!(toc[0].entries.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["Chapter One", "Chapter Two"]);
    assert_eq!(toc[0].entries[0].page, 2);
    let l = lay(&sample("33-drop-cap.pages"));
    assert!(l.body.as_ref().unwrap().extra[0].drop_cap.is_some());
    let l = lay(&sample("16-bullets.pages"));
    assert!(l.body.as_ref().unwrap().tb.paragraphs.iter().any(|p| p.label.is_some()));
}

fn body_text_of(p: &Pages) -> String {
    p.body_text()
}

#[test]
fn edits_write_valid_files() {
    use kn_model::edit::History;
    let out = out_dir();
    // typing, a page break, a section of bold, a heading, a bullet list, a level change
    let mut p = Pages::new_blank(false);
    let b = p.body_storage().unwrap();
    let mut h = History::default();
    p.doc.begin(&mut h, "Typing");
    edit::replace(&mut p.doc, &mut h, b, 0, 0, "Title line\nSome body text here.\nOne\nTwo");
    p.doc.end(&mut h);
    assert_eq!(body_text_of(&p), "Title line\nSome body text here.\nOne\nTwo");
    p.doc.begin(&mut h, "Page Break");
    let at = "Title line\nSome body text here.".len();
    edit::replace(&mut p.doc, &mut h, b, at, at + 1, "\u{5}");
    p.doc.end(&mut h);
    let heading = edit::para_style_named(&p.doc, "Heading").expect("Heading style");
    p.doc.begin(&mut h, "Style");
    assert!(edit::set_para_style(&mut p.doc, &mut h, b, 0, 0, heading));
    p.doc.end(&mut h);
    p.doc.begin(&mut h, "Bold");
    assert!(edit::set_char_props(&mut p.doc, &mut h, b, 11, 15, &|c| c.set_bool("bold", true)));
    p.doc.end(&mut h);
    let bullet = edit::list_styles(&p.doc).into_iter().find(|(_, n)| n == "Bullet").map(|x| x.0).expect("Bullet list");
    let t = p.body_text();
    let one = t.find("One").unwrap();
    p.doc.begin(&mut h, "List");
    assert!(edit::set_list_style(&mut p.doc, &mut h, b, one, t.len(), Some(bullet)));
    assert!(edit::change_level(&mut p.doc, &mut h, b, t.len(), t.len(), 1));
    p.doc.end(&mut h);
    p.doc.begin(&mut h, "Spacing");
    assert!(edit::set_para_props(&mut p.doc, &mut h, b, 12, 12, &|pp| pp.set_f32("space_after", 12.0)));
    p.doc.end(&mut h);
    let l = lay(&p);
    assert_eq!(l.pages.len(), 2);
    let body = l.body.as_ref().unwrap();
    assert!(body.tb.paragraphs[0].runs[0].style.size > 14.0, "heading is bigger");
    assert!(body.tb.paragraphs[1].runs.iter().any(|r| r.style.bold), "bold run");
    assert!(body.tb.paragraphs[2].label.is_some() && body.tb.paragraphs[3].level == 1, "list and level");
    let path = save(&mut p, "E1-typing-break-style-list.pages");
    let again = Pages::open(&path).unwrap();
    assert_eq!(again.body_text(), p.body_text());
    // undo everything back to the blank text
    while p.doc.undo(&mut h) {}
    assert_eq!(body_text_of(&p), "");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn deleting_across_breaks_and_footnotes() {
    use kn_model::edit::History;
    let mut p = sample("18-footnote.pages");
    let b = p.body_storage().unwrap();
    let mut h = History::default();
    p.doc.begin(&mut h, "Delete");
    let t = p.body_text();
    edit::replace(&mut p.doc, &mut h, b, t.len() - '\u{E}'.len_utf8(), t.len(), "");
    p.doc.end(&mut h);
    let l = lay(&p);
    assert!(l.pages[0].footnotes.is_empty());
    let st = p.file().obj(b).unwrap();
    assert_eq!(st.msg("table_footnote").map(|t| t.count("entries")).unwrap_or(0), 0);
    // section break deleted: one section again
    let mut p = sample("26-section-break.pages");
    let b = p.body_storage().unwrap();
    let t = p.body_text();
    let at = t.find('\u{4}').unwrap();
    p.doc.begin(&mut h, "Delete");
    edit::replace(&mut p.doc, &mut h, b, at, at + 1, "");
    p.doc.end(&mut h);
    assert_eq!(p.sections().len(), 1);
    assert_eq!(lay(&p).pages.len(), 1);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn objects_floating_and_inline() {
    use kn_model::edit::History;
    use objects::{NewObject, Placement};
    let mut p = Pages::new_blank(false);
    let b = p.body_storage().unwrap();
    let mut h = History::default();
    p.doc.begin(&mut h, "Typing");
    edit::replace(&mut p.doc, &mut h, b, 0, 0, "Intro\n\nAfter the table.");
    p.doc.end(&mut h);
    let kinds = [NewObject::TextBox, NewObject::Shape(kn_model::insert::BasicShape::Star), NewObject::Line, NewObject::Chart, NewObject::Table];
    for (i, k) in kinds.iter().enumerate() {
        p.doc.begin(&mut h, "Insert");
        p.insert(&mut h, *k, Placement::Floating { page: 0, x: 72.0 + i as f32 * 20.0, y: 300.0 + i as f32 * 30.0 }).unwrap_or_else(|e| panic!("{k:?}: {e}"));
        p.doc.end(&mut h);
    }
    // an inline table on the empty second paragraph, an inline picture after "Intro"
    p.doc.begin(&mut h, "Insert");
    let t = p.insert(&mut h, NewObject::Table, Placement::Inline { at: "Intro\n".len() }).unwrap();
    p.doc.end(&mut h);
    let png = std::fs::read(samples_dir().join("../synthetic-400x300.png")).unwrap();
    p.doc.begin(&mut h, "Insert");
    let img = p.insert_image(&mut h, png, "pic.png", Placement::Floating { page: 0, x: 300.0, y: 100.0 }, 468.0).unwrap();
    p.doc.end(&mut h);
    assert_eq!(p.floating().len(), 6);
    assert!(p.inline_attachment(t).is_some());
    let l = lay(&p);
    let objs = &l.pages[0].objects;
    assert!(objs.iter().any(|o| o.id == t && o.inline), "inline table placed");
    assert!(objs.iter().any(|o| o.id == img));
    // the text after the inline table sits below it
    let p2 = l.body.as_ref().unwrap().tb.paragraphs[2].start;
    let after = l.pages[0].lines.iter().find(|x| x.range().0 >= p2 && x.para == 2).unwrap();
    let tab = objs.iter().find(|o| o.id == t).unwrap();
    assert!(after.band().0 >= tab.y + tab.h - 1.0);
    save(&mut p, "E2-objects.pages");
    // move the image inline, then delete it and a floating shape
    p.doc.begin(&mut h, "Placement");
    p.set_placement(&mut h, img, Placement::Inline { at: 0 });
    p.doc.end(&mut h);
    assert!(p.inline_attachment(img).is_some() && p.floating_page(img).is_none());
    let shape = p.floating()[1].1;
    p.doc.begin(&mut h, "Delete");
    p.delete_objects(&mut h, &[img, shape]);
    p.doc.end(&mut h);
    assert!(p.inline_attachment(img).is_none());
    assert_eq!(p.floating().len(), 4);
    save(&mut p, "E3-objects-deleted.pages");
    let again = Pages::open(out_dir().join("E3-objects-deleted.pages")).unwrap();
    assert_eq!(again.floating().len(), 4);
}

#[test]
fn layout_pages_add_and_delete() {
    use kn_model::edit::History;
    use objects::{NewObject, Placement};
    let mut p = Pages::new_blank(true);
    let mut h = History::default();
    p.doc.begin(&mut h, "Add Page");
    p.add_layout_page(&mut h, 0);
    p.add_layout_page(&mut h, 1);
    p.doc.end(&mut h);
    p.doc.begin(&mut h, "Insert");
    p.insert(&mut h, NewObject::TextBox, Placement::Floating { page: 2, x: 100.0, y: 100.0 }).unwrap();
    p.doc.end(&mut h);
    assert_eq!(lay(&p).pages.len(), 3);
    p.doc.begin(&mut h, "Delete Page");
    p.delete_layout_page(&mut h, 1);
    p.doc.end(&mut h);
    let l = lay(&p);
    assert_eq!(l.pages.len(), 2);
    assert_eq!(l.pages[1].objects.len(), 1);
    save(&mut p, "E4-layout-pages.pages");
}

#[test]
fn comments_and_tracked_changes() {
    use kn_model::edit::History;
    let mut p = Pages::new_blank(false);
    let b = p.body_storage().unwrap();
    let mut h = History::default();
    p.doc.begin(&mut h, "Typing");
    edit::replace(&mut p.doc, &mut h, b, 0, 0, "Original text stays here");
    p.doc.end(&mut h);
    p.doc.begin(&mut h, "Comment");
    p.add_text_comment(&mut h, b, 9, 13, "Look at this", "Tester").unwrap();
    p.doc.end(&mut h);
    let cs = p.text_comments(b);
    assert_eq!(cs.len(), 1);
    assert_eq!((cs[0].start, cs[0].end, cs[0].text.as_str(), cs[0].author.as_str()), (9, 13, "Look at this", "Tester"));
    p.doc.begin(&mut h, "Track");
    p.set_tracking(&mut h, true);
    p.doc.end(&mut h);
    p.doc.begin(&mut h, "Typing");
    let t = p.body_text();
    p.tracked_insert(&mut h, b, t.len(), " inserted", "Tester");
    p.doc.end(&mut h);
    p.doc.begin(&mut h, "Delete");
    p.tracked_delete(&mut h, b, 0, 9, "Tester");
    p.doc.end(&mut h);
    let ch = p.changes(b);
    assert_eq!(ch.len(), 2, "{ch:?}");
    assert!(ch[0].deletion && ch[0].text == "Original ");
    assert!(!ch[1].deletion && ch[1].text == " inserted");
    save(&mut p, "E5-comment-track-changes.pages");
    // layout shows the deletion struck through, the insertion underlined
    let l = lay(&p);
    let runs = &l.body.as_ref().unwrap().tb.paragraphs[0].runs;
    assert!(runs.iter().any(|r| r.style.strike) && runs.iter().any(|r| r.style.underline));
    // accept all: the deleted text goes, the insertion stays plain
    p.doc.begin(&mut h, "Accept All");
    assert_eq!(p.resolve_all(&mut h, b, true), 2);
    p.doc.end(&mut h);
    assert_eq!(p.body_text(), "text stays here inserted");
    assert!(p.changes(b).is_empty());
    // the comment moved with its text
    let cs = p.text_comments(b);
    assert_eq!(&p.body_text()[cs[0].start..cs[0].end], "text");
    save(&mut p, "E6-accepted.pages");
    p.doc.undo(&mut h);
    p.doc.begin(&mut h, "Reject All");
    p.resolve_all(&mut h, b, false);
    p.doc.end(&mut h);
    assert_eq!(p.body_text(), "Original text stays here");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn exports_every_format() {
    let out = out_dir().join("exports");
    std::fs::create_dir_all(&out).unwrap();
    for name in ["16-bullets", "18-footnote", "20-comment", "21-track-changes", "22-link-webpage", "28-inline-image", "08-table", "19-toc", "48-column-break-2col"] {
        let p = sample(&format!("{name}.pages"));
        for ext in ["txt", "rtf", "docx", "html", "epub", "pdf"] {
            let path = out.join(format!("{name}.{ext}"));
            export::export(&p, &path).unwrap_or_else(|e| panic!("{name}.{ext}: {e}"));
            assert!(ext == "txt" || std::fs::metadata(&path).unwrap().len() > 5, "{name}.{ext} empty");
        }
    }
    // what Pages' own exports keep
    let t = export::to_text(&sample("21-track-changes.pages"));
    assert_eq!(t, "text stays here inserted", "changes accepted in plain text");
    let t = export::to_text(&sample("18-footnote.pages"));
    assert!(t.contains("Text with a footnote") && t.contains("Footnote text"));
    let rtf = export::to_rtf(&sample("03-bold-word.pages"));
    assert!(rtf.starts_with("{\\rtf1") && rtf.contains("\\b "));
    // the docx reads back
    let back = import::from_docx(&out.join("16-bullets.docx")).unwrap();
    let texts: Vec<String> = back.iter().map(|p| p.runs.iter().map(|r| r.0.as_str()).collect()).collect();
    assert_eq!(texts, ["One", "Two", "Three"]);
    assert!(back.iter().all(|p| p.list.is_some()));
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn imports_pages_own_exports_and_markdown() {
    let ex = samples_dir().join("../../exports");
    let p = import::import_file(&ex.join("03-bold-word.docx")).unwrap();
    assert!(p.body_text().contains("bold"), "{:?}", p.body_text());
    let l = lay(&p);
    assert!(l.body.as_ref().unwrap().tb.paragraphs.iter().flat_map(|q| q.runs.iter()).any(|r| r.style.bold), "bold kept");
    let p = import::import_file(&ex.join("03-bold-word.rtf")).unwrap();
    assert!(lay(&p).body.as_ref().unwrap().tb.paragraphs.iter().flat_map(|q| q.runs.iter()).any(|r| r.style.bold), "rtf bold kept");
    let p = import::import_file(&ex.join("28-inline-image.docx")).unwrap();
    assert!(!lay(&p).pages[0].objects.is_empty(), "picture imported");
    let md = "# Title here\n\nSome **bold** and *italic* text with a [link](https://example.com).\n\n- one\n- two\n\n1. first\n";
    let p = import::build(&import::from_markdown(md));
    let l = lay(&p);
    let b = l.body.as_ref().unwrap();
    assert_eq!(b.tb.paragraphs.len(), 5);
    assert!(b.tb.paragraphs[0].runs[0].style.size > 16.0, "heading");
    assert!(b.tb.paragraphs[1].runs.iter().any(|r| r.style.bold) && b.tb.paragraphs[1].runs.iter().any(|r| r.style.italic));
    assert!(b.spans.iter().any(|s| matches!(&s.kind, body::SpanKind::Link(u) if u == "https://example.com")));
    assert!(b.tb.paragraphs[2].label.is_some() && b.tb.paragraphs[4].label.is_some());
    let mut p = p;
    save(&mut p, "E7-imported-markdown.pages");
}

#[test]
fn fields_and_structure() {
    use kn_model::edit::History;
    let mut p = Pages::new_blank(false);
    let b = p.body_storage().unwrap();
    let mut h = History::default();
    p.doc.begin(&mut h, "Typing");
    edit::replace(&mut p.doc, &mut h, b, 0, 0, "Chapter One\nSee the site here.\nChapter Two\nMore text");
    p.doc.end(&mut h);
    let t = p.body_text();
    let heading = edit::para_style_named(&p.doc, "Heading").unwrap();
    p.doc.begin(&mut h, "Edits");
    edit::set_para_style(&mut p.doc, &mut h, b, 0, 0, heading);
    let two = t.find("Chapter Two").unwrap();
    edit::set_para_style(&mut p.doc, &mut h, b, two, two, heading);
    let site = t.find("site").unwrap();
    assert!(p.add_link(&mut h, b, site, site + 4, "https://example.com"));
    assert!(p.add_bookmark(&mut h, b, 0, 7));
    p.insert_footnote(&mut h, site + 4).unwrap();
    p.insert_toc(&mut h, 0).unwrap();
    let t = p.body_text();
    let two = t.find("Chapter Two").unwrap();
    p.insert_section_break(&mut h, two).unwrap();
    let n = p.insert_date(&mut h, b, p.body_text().len());
    assert!(n > 10);
    assert!(edit::insert_page_field(&mut p.doc, &mut h, b, 0, false));
    p.doc.end(&mut h);
    assert_eq!(p.bookmarks().len(), 1);
    assert!(p.link_at(b, p.body_text().find("site").unwrap() + 1).is_some());
    assert_eq!(p.sections().len(), 2);
    let l = lay(&p);
    let toc: Vec<_> = l.pages.iter().flat_map(|q| q.toc.iter()).collect();
    assert_eq!(toc.len(), 1);
    assert_eq!(toc[0].entries.len(), 2, "{:?}", toc[0].entries);
    assert!(l.pages.iter().any(|q| !q.footnotes.is_empty()));
    assert!(l.pages.len() >= 2, "section break starts a page");
    save(&mut p, "E8-fields.pages");
    // columns, section background, Convert to Page Layout and back
    p.doc.begin(&mut h, "Columns");
    assert!(p.set_columns(&mut h, 2, 0.05));
    let s0 = p.sections()[0].id;
    p.set_section_background(&mut h, s0, Some(kn_model::Color { r: 1.0, g: 0.95, b: 0.8, a: 1.0 }));
    p.doc.end(&mut h);
    assert_eq!(p.columns().0, 2);
    assert!(lay(&p).pages[0].background.is_some());
    save(&mut p, "E9-columns-background.pages");
    let pages: Vec<(usize, String)> = lay(&p).pages.iter().map(|pg| (pg.index, "text".to_string())).collect();
    p.doc.begin(&mut h, "Convert");
    p.convert_to_page_layout(&mut h, &pages).unwrap();
    p.doc.end(&mut h);
    assert!(!p.is_word_processing());
    assert!(!p.page_templates().is_empty());
    save(&mut p, "E10-converted-to-layout.pages");
    p.doc.begin(&mut h, "Convert");
    p.convert_to_word_processing(&mut h);
    p.doc.end(&mut h);
    assert!(p.is_word_processing());
}

#[test]
fn group_and_ungroup_floating() {
    use kn_model::edit::History;
    use objects::{NewObject, Placement};
    let mut p = Pages::new_blank(true);
    let mut h = History::default();
    p.doc.begin(&mut h, "Insert");
    let a = p.insert(&mut h, NewObject::Shape(kn_model::insert::BasicShape::Rectangle), Placement::Floating { page: 0, x: 100.0, y: 100.0 }).unwrap();
    let b = p.insert(&mut h, NewObject::Shape(kn_model::insert::BasicShape::Oval), Placement::Floating { page: 0, x: 300.0, y: 200.0 }).unwrap();
    p.doc.end(&mut h);
    p.doc.begin(&mut h, "Group");
    let g = p.group(&mut h, &[a, b]).unwrap();
    p.doc.end(&mut h);
    assert_eq!(p.floating().iter().map(|x| x.1).collect::<Vec<_>>(), vec![g]);
    let l = lay(&p);
    assert_eq!(l.pages[0].objects.len(), 1);
    save(&mut p, "E11-group.pages");
    p.doc.begin(&mut h, "Ungroup");
    let kids = p.ungroup(&mut h, g).unwrap();
    p.doc.end(&mut h);
    assert_eq!(kids.len(), 2);
    let fl: Vec<ObjId> = p.floating().iter().map(|x| x.1).collect();
    assert!(fl.contains(&a) && fl.contains(&b) && !fl.contains(&g));
    // positions back on the page
    let ga = p.file().obj(a).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap();
    assert!((ga.x - 100.0).abs() < 0.5 && (ga.y - 100.0).abs() < 0.5, "{ga:?}");
    assert!(p.file().obj(a).and_then(|m| m.find_super("TSD.DrawableArchive")).and_then(|d| d.ref_id("parent")).is_none());
    save(&mut p, "E12-ungroup.pages");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn package_password_and_more_imports() {
    // a .pages package (folder) opens like a single file
    let p = sample("53-package-format.pages");
    assert!(p.root().is_some());
    // password round trip
    let mut p = sample("02-paragraphs.pages");
    p.doc.password = Some(("secret".into(), "the usual".into()));
    let path = save(&mut p, "E13-password.pages");
    match Pages::open(&path) {
        Err(kn_format::Error::NeedsPassword(hint)) => assert_eq!(hint, "the usual"),
        other => panic!("expected a password prompt, got {:?}", other.map(|_| ())),
    }
    let q = Pages::open_with_password(&path, "secret", "the usual").unwrap();
    assert!(q.body_text().contains("Para two"));
    assert!(Pages::open_with_password(&path, "wrong", "").is_err());
    // HTML and plain text
    let h = import::build(&import::from_html("<html><body><h1>Big</h1><p>Some <b>bold</b> text and <a href=\"https://x.org\">a link</a>.</p><ul><li>one</li><li>two</li></ul></body></html>"));
    let l = lay(&h);
    let b = l.body.as_ref().unwrap();
    assert_eq!(b.tb.paragraphs.len(), 4, "{:?}", b.tb.text);
    assert!(b.tb.paragraphs[1].runs.iter().any(|r| r.style.bold));
    assert!(b.tb.paragraphs[2].label.is_some());
    let t = import::build(&import::from_text("line one\nline two\u{c}page two"));
    assert_eq!(lay(&t).pages.len(), 2);
    // pictures of the pages
    let dir = out_dir().join("png");
    let n = export::to_images(&sample("10-page-break.pages"), &dir, "pb").unwrap();
    assert_eq!(n, 2);
    assert!(dir.join("pb-2.png").exists());
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn samples_have_no_orphans() {
    for f in all_samples() {
        let p = Pages::open(&f).unwrap();
        assert_eq!(gc::orphans(p.file()), (Vec::new(), Vec::new()), "{}", f.display());
        assert_eq!(dangling(p.file()), Vec::new(), "{}: declared references to missing objects", f.display());
        assert_eq!(dangling_payload(p.file()), Vec::new(), "{}: references to missing objects", f.display());
        // and Pages keeps no unused style variations either (what the sweep culls)
        let mut k = p.file().clone();
        assert_eq!(gc::cull_variations(&mut k), 0, "{}", f.display());
    }
}

/// Every enum value the code writes exists in the Pages schema (a wrong name panics when set).
#[test]
fn enum_literals_exist_in_schema() {
    let schema = include_str!("../data/schema.json");
    // the values of every "enum": {"0": "kName", …} object
    let mut values = std::collections::HashSet::new();
    let mut rest = schema;
    while let Some(i) = rest.find("\"enum\"") {
        rest = &rest[i + 6..];
        let end = rest.find('}').unwrap_or(rest.len());
        for (j, part) in rest[..end].split('"').enumerate() {
            // "{", key, ": ", value, ", ", key, … : values are every fourth piece
            if j % 4 == 3 {
                values.insert(part.to_string());
            }
        }
        rest = &rest[end..];
    }
    assert!(values.contains("kTitled") && values.contains("kNoScript"));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut bad = Vec::new();
    for dir in ["pg-core/src", "pg-app/src"] {
        for e in std::fs::read_dir(root.join(dir)).unwrap().flatten() {
            let src = std::fs::read_to_string(e.path()).unwrap_or_default();
            for (k, _) in src.match_indices("set_enum(") {
                // the last string literal before the closing parenthesis is the value
                let call = &src[k..src[k..].find(");").map(|n| k + n).unwrap_or(src.len())];
                let lits: Vec<&str> = call.split('"').skip(1).step_by(2).collect();
                if let Some(v) = lits.iter().rev().find(|v| v.starts_with('k') || v.starts_with("TAT")) {
                    if !values.contains(*v) {
                        bad.push(format!("{}: {v}", e.path().display()));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "enum values not in the schema: {bad:?}");
}

/// The body's attribute tables are in order and inside the text (what Pages needs to open it).
fn check_tables(p: &Pages, what: &str) {
    let b = p.body_storage().unwrap();
    let text = p.doc.storage_text(b);
    let n16 = text.encode_utf16().count();
    let st = p.file().obj(b).unwrap();
    for t in ["table_para_style", "table_char_style", "table_list_style", "table_section", "table_footnote", "table_attachment", "table_bookmark", "table_insertion", "table_deletion", "table_highlight", "table_smartfield", "table_layout_style"] {
        let Some(tm) = st.msg(t) else { continue };
        let idx: Vec<u64> = tm.msgs("entries").map(|e| e.u64("character_index").unwrap_or(0)).collect();
        assert!(idx.windows(2).all(|w| w[0] < w[1]), "{what}: {t} out of order {idx:?}");
        assert!(idx.iter().all(|i| (*i as usize) <= n16), "{what}: {t} past the text ({n16}) {idx:?}");
        if matches!(t, "table_para_style" | "table_list_style") {
            assert_eq!(idx.first(), Some(&0), "{what}: {t} doesn't start at 0");
        }
    }
    // every attachment character has its attachment
    let atts: Vec<u64> = st.msg("table_attachment").map(|tm| tm.msgs("entries").map(|e| e.u64("character_index").unwrap_or(0)).collect()).unwrap_or_default();
    let fffc: Vec<u64> = text.encode_utf16().enumerate().filter(|(_, c)| *c == 0xFFFC).map(|(i, _)| i as u64).collect();
    assert_eq!(atts, fffc, "{what}: attachments vs U+FFFC");
}

/// Random typing, deleting and formatting (fixed seed) keep every sample consistent: the tables in
/// order, layout working, a clean save that opens again with the same text, and undo all the way back.
#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn fuzzed_edits_keep_files_valid() {
    use kn_model::edit::History;
    let mut seed: u64 = 0x5eed_c0de;
    let mut rnd = move |n: usize| -> usize {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as usize) % n.max(1)
    };
    let pieces = ["a", "word ", "é", "\n", "\t", "\u{2028}", "\u{5}", "\u{c}", " two words\n", "ü€"];
    let mut done = 0;
    for f in all_samples() {
        let mut p = Pages::open(&f).unwrap();
        if !p.setup().body || p.doc.password.is_some() {
            continue;
        }
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let b = p.body_storage().unwrap();
        let original = p.body_text();
        let heading = edit::para_style_named(&p.doc, "Heading");
        let bullet = edit::list_styles(&p.doc).into_iter().find(|(_, n)| n == "Bullet").map(|x| x.0);
        let mut h = History::default();
        for step in 0..40 {
            let text = p.doc.storage_text(b);
            let bounds: Vec<usize> = text.char_indices().map(|(i, _)| i).chain([text.len()]).collect();
            // never split an attachment, footnote or field from its character (the app doesn't either)
            let a = bounds[rnd(bounds.len())];
            let e = bounds[(bounds.iter().position(|x| *x == a).unwrap() + rnd(6)).min(bounds.len() - 1)];
            p.doc.begin(&mut h, "fuzz");
            let op = rnd(7);
            match op {
                0 | 1 => edit::replace(&mut p.doc, &mut h, b, a, a, pieces[rnd(pieces.len())]),
                2 => edit::replace(&mut p.doc, &mut h, b, a, e, ""),
                3 => {
                    edit::set_char_props(&mut p.doc, &mut h, b, a, e.max(a), &|c| c.set_bool("bold", true));
                }
                4 => {
                    if let Some(s) = heading {
                        edit::set_para_style(&mut p.doc, &mut h, b, a, e, s);
                    }
                }
                5 => {
                    edit::set_list_style(&mut p.doc, &mut h, b, a, e, if rnd(2) == 0 { bullet } else { None });
                }
                _ => edit::replace(&mut p.doc, &mut h, b, a, e, pieces[rnd(pieces.len())]),
            }
            p.doc.end(&mut h);
            check_tables(&p, &format!("{name} step {step}"));
        }
        let l = lay(&p);
        assert!(!l.pages.is_empty(), "{name}");
        let path = save(&mut p, &format!("fuzz-{name}"));
        assert_eq!(Pages::open(&path).unwrap().body_text(), p.body_text(), "{name}: text after reopening");
        while p.doc.undo(&mut h) {}
        assert_eq!(p.body_text(), original, "{name}: undo back to the start");
        check_tables(&p, &format!("{name} undone"));
        done += 1;
    }
    assert!(done >= 30, "only {done} word-processing samples");
}

/// Arrange > Section Layouts: a shape becomes the page template's layout object (shown on the
/// section's pages, under the text), saves clean, comes back to the page, and can be deleted.
#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn section_layout_objects() {
    use kn_model::edit::History;
    let mut p = sample("05-shape.pages");
    let mut h = History::default();
    let (_, shape) = p.floating()[0];
    let l = lay(&p);
    let t = p.page_template_of(&l, 0).expect("template");
    p.move_to_section_layout(&mut h, &[shape], t);
    assert!(p.floating().is_empty());
    let l = lay(&p);
    assert!(l.pages[0].objects.iter().any(|o| o.id == shape && o.layout), "shown as a layout object");
    let path = save(&mut p, "E14-section-layout-object.pages");
    let back = Pages::open(&path).unwrap();
    assert!(lay(&back).pages[0].objects.iter().any(|o| o.id == shape && o.layout));
    p.move_from_section_layout(&mut h, &[shape], 0);
    assert_eq!(p.floating(), vec![(0, shape)]);
    assert!(!lay(&p).pages[0].objects.iter().any(|o| o.layout));
    // deleting a layout object takes it out of its template too
    p.move_to_section_layout(&mut h, &[shape], t);
    p.delete_objects(&mut h, &[shape]);
    save(&mut p, "E15-section-layout-object-deleted.pages");
}

/// A merge stored the older way (two cell references and a colon, as in the user's 2023
/// booklet) reads as merged, and Unmerge removes it: saved, reopened, nothing else changed.
#[test]
fn old_style_merge_unmerge() {
    use kn_model::edit::History;
    use objects::{NewObject, Placement};
    let mut p = Pages::new_blank(false);
    let mut h = History::default();
    p.doc.begin(&mut h, "Insert");
    let t = p.insert(&mut h, NewObject::Table, Placement::Inline { at: 0 }).unwrap();
    p.doc.end(&mut h);
    p.doc.begin(&mut h, "Merge");
    p.doc.merge_cells(&mut h, t, kn_model::CellRange { row: 0, col: 0, rows: 1, cols: 3 }).unwrap();
    p.doc.end(&mut h);
    // rewrite the merge formula the old way, keeping its formula index and dependencies
    let model = p.file().obj(t).and_then(|m| m.ref_id("tableModel")).unwrap();
    p.doc.begin(&mut h, "Old style");
    p.doc.touch(&mut h, model);
    {
        let m = p.doc.file.obj_mut(model).unwrap();
        let store = m.msg_mut("merge_owner").unwrap().msg_mut("formula_store").unwrap();
        let f = store.msgs_mut("formulas").next().unwrap();
        let arr = f.msg_mut_or_create("formula").msg_mut_or_create("AST_node_array");
        let node = arr.field_def("AST_node").unwrap().num;
        arr.fields.retain(|x| x.num != node);
        for (r, c) in [(0, 0), (0, 2)] {
            let n = arr.push_new("AST_node");
            n.set_enum("AST_node_type", "CELL_REFERENCE_NODE");
            n.msg_mut_or_create("AST_row").set_i64("row", r);
            n.msg_mut_or_create("AST_column").set_i64("column", c);
        }
        arr.push_new("AST_node").set_enum("AST_node_type", "COLON_NODE");
        arr.push_new("AST_node").set_enum("AST_node_type", "FUNCTION_NODE");
    }
    p.doc.end(&mut h);
    let shape = |p: &Pages| {
        let i = p.file().obj(t).unwrap();
        let td = kn_model::table::table(p.file(), i).unwrap();
        (td.rows, td.cols, td.merges.iter().map(|m| (m.row, m.col, m.rows, m.cols)).collect::<Vec<_>>())
    };
    let path = save(&mut p, "E16-old-style-merge.pages");
    let mut back = Pages::open(&path).unwrap();
    let (rows, cols, merges) = shape(&back);
    assert_eq!(merges, vec![(0, 0, 1, 3)], "old-style merge read back");
    let mut h = History::default();
    back.doc.begin(&mut h, "Unmerge");
    assert!(back.doc.unmerge_cells(&mut h, t, 0, 1), "unmerge finds the old-style merge");
    back.doc.end(&mut h);
    let path = save(&mut back, "E17-old-style-unmerged.pages");
    let again = Pages::open(&path).unwrap();
    assert_eq!(shape(&again), (rows, cols, Vec::new()), "merge gone, size unchanged");
}

/// Bullets & Lists details and drop cap settings: changed through style variations, saved,
/// read back.
#[test]
fn list_levels_and_drop_cap_settings() {
    use kn_model::edit::History;
    let mut p = Pages::new_blank(false);
    let b = p.body_storage().unwrap();
    let mut h = History::default();
    p.doc.begin(&mut h, "Typing");
    edit::replace(&mut p.doc, &mut h, b, 0, 0, "One\nTwo\nThree");
    p.doc.end(&mut h);
    let lists = edit::list_styles(&p.doc);
    let numbered = lists.iter().find(|l| l.1 == "Numbered").map(|l| l.0).expect("a Numbered list style");
    assert_eq!(edit::list_marker(&p.doc, numbered), "1.");
    p.doc.begin(&mut h, "List");
    edit::set_list_style(&mut p.doc, &mut h, b, 0, 5, Some(numbered));
    edit::set_list_level(&mut p.doc, &mut h, b, 0, 5, 0, &|l| {
        l.number = 9;
        l.tiered = true;
        l.text_indent = 30.0;
    });
    p.doc.end(&mut h);
    let (list, level) = edit::list_at(&p.doc, b, 0);
    let l = edit::list_level(&p.doc, list.unwrap(), level);
    assert_eq!((l.kind, l.number, l.tiered, l.text_indent), (3, 9, true, 30.0));
    let styles = edit::drop_cap_styles(&p.doc);
    let first = styles.first().map(|s| s.0).expect("drop cap styles");
    p.doc.begin(&mut h, "Drop Cap");
    edit::set_drop_cap(&mut p.doc, &mut h, b, 0, 0, Some(first));
    edit::set_drop_cap_settings(&mut p.doc, &mut h, b, 0, 0, first, &|s| {
        s.lines = 2;
        s.shape = true;
        s.fill = Some(kn_model::Color { r: 1.0, g: 0.5, b: 0.0, a: 1.0 });
    });
    p.doc.end(&mut h);
    let path = save(&mut p, "E18-lists-dropcaps.pages");
    let back = Pages::open(&path).unwrap();
    let b2 = back.body_storage().unwrap();
    let (list, level) = edit::list_at(&back.doc, b2, 4);
    assert_eq!(edit::list_level(&back.doc, list.unwrap(), level).number, 9, "second paragraph keeps the list");
    let dc = edit::drop_cap_at(&back.doc, b2, 0).expect("drop cap kept");
    let s = edit::drop_cap_settings(&back.doc, dc);
    assert_eq!((s.lines, s.shape), (2, true));
    assert!(s.fill.is_some_and(|c| (c.g - 0.5).abs() < 0.01));
    assert!(edit::drop_cap_at(&back.doc, b2, 5).is_none(), "the next paragraph has none");
}

/// Media > Choose Music / Record Audio: a sound inserted on a page saves cleanly and reads back.
#[test]
fn insert_sound_and_movie() {
    use kn_model::edit::History;
    use objects::Placement;
    let mut p = Pages::new_blank(false);
    let mut h = History::default();
    // a tiny WAV (44-byte header, silence)
    let mut wav = b"RIFF\x24\x01\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x40\x1f\x00\x00\x80\x3e\x00\x00\x02\x00\x10\x00data\x00\x01\x00\x00".to_vec();
    wav.extend(std::iter::repeat_n(0u8, 256));
    let m = kn_model::insert::NewMovie { bytes: wav, name: "beep.wav".into(), poster: None, natural: None, duration: 0.1, audio_only: true };
    p.doc.begin(&mut h, "Insert");
    let id = p.insert_movie(&mut h, m, Placement::Floating { page: 0, x: 100.0, y: 120.0 }, 468.0).unwrap();
    p.doc.end(&mut h);
    assert_eq!(p.file().type_name(id), "TSD.MovieArchive");
    assert!(p.floating().iter().any(|f| f.1 == id), "on the page");
    let path = save(&mut p, "E19-sound.pages");
    let back = Pages::open(&path).unwrap();
    assert!(back.floating().iter().any(|f| f.1 == id), "kept on save");
    let l = lay(&back);
    assert!(l.pages[0].objects.iter().any(|o| o.id == id), "laid out");
}

#[test]
fn connection_line_follows_objects() {
    use kn_model::edit::History;
    use kn_model::insert::{BasicShape, Connection};
    use objects::{NewObject, Placement};
    let mut p = Pages::new_blank(false);
    let mut h = History::default();
    let geom = |p: &Pages, id| p.file().obj(id).and_then(|o| o.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap();
    p.doc.begin(&mut h, "Insert");
    let a = p.insert(&mut h, NewObject::Shape(BasicShape::Rectangle), Placement::Floating { page: 0, x: 100.0, y: 200.0 }).unwrap();
    let b = p.insert(&mut h, NewObject::Shape(BasicShape::Rectangle), Placement::Floating { page: 0, x: 400.0, y: 200.0 }).unwrap();
    let line = p.insert_connection_line(&mut h, Connection::Straight, Some((a, b)), 0, (0.0, 0.0)).unwrap();
    p.doc.end(&mut h);
    assert_eq!(p.floating_page(line), Some(0));
    let (ga, g0) = (geom(&p, a), geom(&p, line));
    assert!((g0.x - (ga.x + ga.w)).abs() < 1.0, "starts at the right edge of the first shape: {g0:?}");
    p.doc.begin(&mut h, "Move");
    p.doc.move_by(&mut h, b, 50.0, 0.0);
    p.update_connections(&mut h, &[b]);
    p.doc.end(&mut h);
    assert!((geom(&p, line).w - g0.w - 50.0).abs() < 1.0, "the end followed");
    let path = save(&mut p, "E20-connection.pages");
    let back = Pages::open(&path).unwrap();
    assert_eq!(back.file().type_name(line), "TSD.ConnectionLineArchive");
    assert!((geom(&back, line).w - g0.w - 50.0).abs() < 1.0, "kept on save: {:?}", geom(&back, line));
    let (orphans, _) = gc::orphans(back.file());
    assert!(orphans.is_empty(), "nothing left over: {orphans:?}");
    assert!(lay(&back).pages[0].objects.iter().any(|o| o.id == line), "laid out");
    // joining something that isn't on a page (inline objects, the body) is refused
    let mut p = back;
    let root = p.root_id().unwrap();
    assert!(p.insert_connection_line(&mut h, Connection::Curved, Some((root, a)), 0, (0.0, 0.0)).is_err());
}

#[test]
fn conditional_highlighting_on_new_table() {
    use kn_model::edit::History;
    use kn_model::tablecells::CellInput;
    use kn_model::tablecond::{Cmp, LOOKS};
    use objects::{NewObject, Placement};
    let mut p = Pages::new_blank(false);
    let mut h = History::default();
    p.doc.begin(&mut h, "Insert");
    let t = p.insert(&mut h, NewObject::Table, Placement::Inline { at: 0 }).unwrap();
    for (r, v) in [(1, 5.0), (2, 50.0)] {
        p.doc.set_cell(&mut h, t, r, 1, &CellInput::Number(v)).unwrap();
    }
    p.doc.end(&mut h);
    assert!(p.doc.can_highlight(t), "a table Letters inserts takes rules");
    p.doc.begin(&mut h, "Rule");
    let n = p.doc.add_highlight_rule(&mut h, t, (1, 1, 2, 1), Cmp::Greater, 10.0, &LOOKS[0]).unwrap();
    p.doc.end(&mut h);
    assert_eq!(n, 2);
    let path = save(&mut p, "E21-highlight.pages");
    let mut back = Pages::open(&path).unwrap();
    assert_eq!(back.doc.highlight_rules(t, 2, 1).len(), 1, "kept on save");
    back.doc.begin(&mut h, "Clear");
    back.doc.clear_highlight_rules(&mut h, t, (1, 1, 2, 1)).unwrap();
    back.doc.end(&mut h);
    assert!(back.doc.highlight_rules(t, 2, 1).is_empty());
}

#[test]
fn page_guides_round_trip() {
    use guides::Guide;
    use kn_model::edit::History;
    let mut p = Pages::new_blank(false);
    let mut h = History::default();
    let l = lay(&p);
    assert!(p.page_guides(&l, 0).is_empty());
    let gs = [Guide { vertical: true, pos: 120.0 }, Guide { vertical: false, pos: 300.5 }];
    p.doc.begin(&mut h, "Guide");
    assert!(p.set_page_guides(&mut h, &l, 0, &gs));
    p.doc.end(&mut h);
    assert_eq!(p.page_guides(&l, 0).len(), 2);
    let path = save(&mut p, "E22-guides.pages");
    let mut back = Pages::open(&path).unwrap();
    let l = lay(&back);
    let g = back.page_guides(&l, 0);
    assert_eq!((g[0].vertical, g[0].pos, g[1].vertical, g[1].pos), (true, 120.0, false, 300.5));
    // Clear All Guides on Page
    back.doc.begin(&mut h, "Clear Guides");
    back.set_page_guides(&mut h, &l, 0, &[]);
    back.doc.end(&mut h);
    assert!(back.page_guides(&l, 0).is_empty());
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn gallery_and_web_video_round_trip() {
    // galleries and web videos are TSA extensions of TSD.ImageArchive (fields 200 / 300)
    use kn_model::edit::History;
    use objects::Placement;
    let png = std::fs::read(samples_dir().join("../synthetic-400x300.png")).unwrap();
    let mut p = Pages::new_blank(false);
    let mut h = History::default();
    p.doc.begin(&mut h, "Insert");
    let gal = p.insert_image(&mut h, png.clone(), "a.png", Placement::Floating { page: 0, x: 72.0, y: 100.0 }, 468.0).unwrap();
    p.doc.make_gallery(&mut h, gal, vec![(png.clone(), "b.png".into()), (png.clone(), "c.png".into())]).unwrap();
    let web = p.insert_image(&mut h, png, "poster.png", Placement::Floating { page: 0, x: 72.0, y: 450.0 }, 468.0).unwrap();
    assert!(p.doc.make_web_video(&mut h, web, "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "A video", "Someone"));
    p.doc.end(&mut h);
    let path = save(&mut p, "E23-gallery-web.pages");
    let back = Pages::open(&path).unwrap();
    let l = lay(&back);
    let ds: Vec<kn_model::Drawable> = l.pages.iter().flat_map(|pg| flow::page_drawables(&back, pg)).collect();
    let image = |id| ds.iter().find_map(|d| match &d.kind {
        kn_model::DrawableKind::Image(im) if d.id == id => Some(im.clone()),
        _ => None,
    });
    assert_eq!(image(gal).unwrap().gallery.len(), 3, "the gallery's three images");
    assert_eq!(image(web).unwrap().web_video.map(|w| w.title), Some("A video".to_string()));
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn section_image_background() {
    use kn_model::edit::History;
    use kn_model::format::FillSpec;
    let png = std::fs::read(samples_dir().join("../synthetic-400x300.png")).unwrap();
    let mut p = Pages::new_blank(false);
    let mut h = History::default();
    let s0 = p.sections()[0].id;
    p.doc.begin(&mut h, "Background");
    let data = p.doc.file.add_image_data(png, "bg.png").unwrap();
    p.set_section_fill(&mut h, s0, &FillSpec::Image { data_id: data, technique: kn_model::ImageFillTechnique::ScaleToFill, size: Some((400.0, 300.0)), reference: None, tint: None });
    p.doc.end(&mut h);
    let path = save(&mut p, "E24-image-background.pages");
    let back = Pages::open(&path).unwrap();
    assert!(matches!(back.sections()[0].background, Some(kn_model::Fill::Image { .. })), "{:?}", back.sections()[0].background);
    assert!(back.file().archive(s0).unwrap().data_references(0).contains(&data), "the picture is listed with the section");
    let mut p = back;
    p.doc.begin(&mut h, "Background");
    p.set_section_fill(&mut h, s0, &FillSpec::None);
    p.doc.end(&mut h);
    assert!(p.sections()[0].background.is_none());
}

#[test]
fn tall_header_and_footer_move_the_body() {
    use kn_model::edit::History;
    let mut p = Pages::new_blank(false);
    let mut h = History::default();
    let mut s = p.setup();
    s.top = 40.0;
    s.bottom = 40.0;
    s.headers = true;
    s.footers = true;
    p.doc.begin(&mut h, "Setup");
    p.set_setup(&mut h, &s);
    let body = p.body_storage().unwrap();
    edit::replace(&mut p.doc, &mut h, body, 0, 0, "Body text");
    let sec = p.sections()[0].clone();
    let master = sec.master(0, 1).unwrap();
    let head = p.header_footer_storages(master, false)[0];
    let foot = p.header_footer_storages(master, true)[0];
    edit::replace(&mut p.doc, &mut h, head, 0, 0, "One\nTwo\nThree");
    edit::replace(&mut p.doc, &mut h, foot, 0, 0, "Footer");
    p.doc.end(&mut h);
    let l = lay(&p);
    let pg = &l.pages[0];
    let hd = pg.headers.iter().find(|x| x.storage == head).unwrap();
    let header_bottom = hd.dy + hd.piece.tl.content_h;
    let first = pg.lines.first().unwrap();
    assert!(header_bottom > 40.0, "the header reaches past the 40 pt top margin: {header_bottom}");
    assert!(first.dy >= header_bottom, "the body starts below the header: {} < {header_bottom}", first.dy);
    // with the headers empty, the margin decides again
    let mut p2 = Pages::new_blank(false);
    p2.doc.begin(&mut h, "Setup");
    p2.set_setup(&mut h, &s);
    let b2 = p2.body_storage().unwrap();
    edit::replace(&mut p2.doc, &mut h, b2, 0, 0, "Body text");
    p2.doc.end(&mut h);
    assert!((lay(&p2).pages[0].lines[0].dy - 40.0).abs() < 0.5);
}

#[test]
fn inserted_chart_uses_the_documents_preset() {
    use kn_model::edit::History;
    use objects::{NewObject, Placement};
    let mut p = Pages::new_blank(false);
    let mut h = History::default();
    let before: Vec<_> = p.file().objects_of("TSCH.ChartStylePreset").map(|(i, _)| i).collect();
    p.doc.begin(&mut h, "Insert");
    let c = p.insert(&mut h, NewObject::Chart, Placement::Floating { page: 0, x: 72.0, y: 100.0 }).unwrap();
    p.doc.end(&mut h);
    let after: Vec<_> = p.file().objects_of("TSCH.ChartStylePreset").map(|(i, _)| i).collect();
    assert_eq!(before, after, "no copied preset (Chart Styles keeps its six)");
    let preset = p.file().obj(c).and_then(|m| m.msg("unity")).and_then(|u| u.ref_id("preset")).unwrap();
    assert!(before.contains(&preset));
    let path = save(&mut p, "E25-chart-preset.pages");
    let back = Pages::open(&path).unwrap();
    assert!(lay(&back).pages[0].objects.iter().any(|o| o.id == c), "the chart still lays out");
}

#[test]
fn word_mode_keeps_the_cardstock_copy() {
    use kn_model::edit::History;
    let mut p = Pages::new_blank(false);
    let mut h = History::default();
    let body = p.body_storage().unwrap();
    p.doc.begin(&mut h, "Type");
    edit::replace(&mut p.doc, &mut h, body, 0, 0, "Kept whole");
    p.doc.end(&mut h);
    let dir = out_dir();
    // Word + Letters: the Letters copy comes back
    let hybrid = dir.join("E26-hybrid.docx");
    wordmode::save_docx(&mut p, &hybrid, true).unwrap();
    let o = wordmode::open_docx(&hybrid).unwrap();
    assert!(o.hybrid && !o.changed_elsewhere);
    assert_eq!(text_plain(&o.pages.doc.storage_text(o.pages.body_storage().unwrap())), "Kept whole");
    // another app changed the Word part: its content is read instead
    let changed = dir.join("E26-hybrid-changed.docx");
    {
        let mut z = zip::ZipArchive::new(std::fs::File::open(&hybrid).unwrap()).unwrap();
        let mut w = zip::ZipWriter::new(std::fs::File::create(&changed).unwrap());
        for i in 0..z.len() {
            let mut e = z.by_index(i).unwrap();
            let name = e.name().unwrap().to_string();
            let mut b = Vec::new();
            std::io::Read::read_to_end(&mut e, &mut b).unwrap();
            if name == "word/document.xml" {
                b = String::from_utf8(b).unwrap().replace("Kept whole", "Edited in Word").into_bytes();
            }
            w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut w, &b).unwrap();
        }
        w.finish().unwrap();
    }
    let o = wordmode::open_docx(&changed).unwrap();
    assert!(o.changed_elsewhere);
    assert!(text_plain(&o.pages.doc.storage_text(o.pages.body_storage().unwrap())).contains("Edited in Word"));
    // plain Word: no copy
    let plain = dir.join("E26-plain.docx");
    wordmode::save_docx(&mut p, &plain, false).unwrap();
    assert!(!wordmode::open_docx(&plain).unwrap().hybrid);
}

/// A .docx made of the given parts (Content_Types and the package relationships added).
fn write_docx(path: &std::path::Path, parts: &[(&str, &str)]) {
    let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let ct = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    for (name, body) in [("[Content_Types].xml", ct), ("_rels/.rels", rels)].iter().chain(parts.iter()) {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut w, body.as_bytes()).unwrap();
    }
    w.finish().unwrap();
}

/// Word rules OnlyOffice follows that a Word file relies on (the "Support Raising Toolkit"
/// differences): list indents from numbering.xml, contextual spacing, table style paragraph
/// properties, Pages-only style extras kept out, empty rows a line tall, autofit tables shrunk
/// to the text column.
#[test]
fn word_import_follows_onlyoffice_rules() {
    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let styles = format!(
        r#"<w:styles {W}><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="200" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
        <w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:pPr><w:spacing w:after="80"/></w:pPr><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/></w:rPr></w:style>
        <w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:rPr><w:b/></w:rPr></w:style>
        <w:style w:type="paragraph" w:styleId="ListBullet"><w:name w:val="List Bullet"/><w:basedOn w:val="Normal"/><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr><w:contextualSpacing/></w:pPr></w:style>
        <w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/></w:style>
        <w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:basedOn w:val="TableNormal"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:style></w:styles>"#
    );
    let numbering = format!(
        r#"<w:numbering {W}><w:abstractNum w:abstractNumId="8"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:tabs><w:tab w:val="num" w:pos="360"/></w:tabs><w:ind w:left="360" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="8"/></w:num></w:numbering>"#
    );
    let li = |t: &str| format!(r#"<w:p><w:pPr><w:pStyle w:val="ListBullet"/></w:pPr><w:r><w:t>{t}</w:t></w:r></w:p>"#);
    let cell = |w: u32, t: &str| format!(r#"<w:tc><w:tcPr><w:tcW w:type="dxa" w:w="{w}"/></w:tcPr><w:p>{}</w:p></w:tc>"#, if t.is_empty() { String::new() } else { format!("<w:r><w:t>{t}</w:t></w:r>") });
    // three columns of 4000 twips (600 pt) in a 468 pt text column
    let row = |a: &str| format!("<w:tr>{}{}{}</w:tr>", cell(4000, a), cell(4000, ""), cell(4000, ""));
    let doc = format!(
        r#"<w:document {W}><w:body><w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t>Heading</w:t></w:r></w:p>{}{}{}
        <w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:type="auto" w:w="0"/></w:tblPr><w:tblGrid><w:gridCol w:w="4000"/><w:gridCol w:w="4000"/><w:gridCol w:w="4000"/></w:tblGrid>{}{}</w:tbl><w:p/>
        <w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720"/></w:sectPr></w:body></w:document>"#,
        li("One"),
        li("Two"),
        li("Three"),
        row("Text"),
        row("")
    );
    let settings = format!(r#"<w:settings {W}><w:defaultTabStop w:val="720"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="14"/></w:compat></w:settings>"#);
    let dir = std::env::temp_dir().join("pg-word-rules");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rules.docx");
    write_docx(&path, &[("word/document.xml", &doc), ("word/styles.xml", &styles), ("word/numbering.xml", &numbering), ("word/settings.xml", &settings)]);

    let wd = docx::read(&path).unwrap();
    let paras: Vec<&docx::Para> = wd.blocks.iter().filter_map(|b| if let docx::Block::Para(p) = b { Some(p) } else { None }).collect();
    // the numbering level's hanging indent: label at the margin, text 18 pt in
    assert_eq!(paras[1].x.list_geom, Some((0.0, 18.0, 18.0)));
    // contextual spacing: no space between the list items, the last keeps its space after
    assert_eq!(paras[1].x.space_after, Some(0.0));
    assert_eq!(paras[2].x.space_before, Some(0.0));
    assert_eq!(paras[3].x.space_after, Some(4.0));
    // the table style's single line spacing in its cells (the paragraph style's space after)
    let t = wd.blocks.iter().find_map(|b| if let docx::Block::Table(t) = b { Some(t) } else { None }).unwrap();
    let cp = &t.rows[0].2[0].paras[0];
    assert_eq!(cp.x.line, Some(("kRelativeLineSpacing", 1.0)));
    assert_eq!(cp.x.space_after, Some(4.0));

    let p = docx::build(&wd);
    let body = body::build(p.file(), p.body_storage().unwrap(), None, Default::default()).unwrap();
    // Pages' Heading 3 rule and tracking stay out of a Word heading
    assert_eq!(body.extra[0].border_edges, 0);
    assert!(body.tb.paragraphs[0].runs.iter().all(|r| r.style.tracking == 0.0));
    // the bullet where Word draws it, the text at the left indent
    let lp = &body.tb.paragraphs[1];
    assert!(lp.label.is_some());
    assert!((lp.left_indent + lp.list_indent).abs() < 0.01, "label at the margin: {} {}", lp.left_indent, lp.list_indent);
    let em = lp.runs.first().map(|r| r.style.size).unwrap();
    assert!((lp.label_indent * em - 18.0).abs() < 0.01, "text 18 pt in: {}", lp.label_indent * em);

    let l = lay(&p);
    let ys: Vec<f32> = l.pages[0].lines.iter().map(|ln| ln.dy).collect();
    // list items one line apart (11 pt Calibri at 1.15: about 15.4 pt), no 4 pt gap
    let gap = ys[2] - ys[1];
    assert!((ys[3] - ys[2] - gap).abs() < 0.01 && gap < 16.0, "list line gaps {:?}", ys);
    // the autofit table shrinks to the text column plus its cell margins (468 + 10.8 pt)
    let tbl = l.pages[0].objects.iter().find(|o| p.file().type_name(o.id) == "TST.TableInfoArchive").unwrap();
    assert!((tbl.w - 478.8).abs() < 1.0, "table width {}", tbl.w);
    // an empty row is a line tall (13.4 pt + 4 pt space after), like the row with text
    assert!(tbl.h > 2.0 * 17.0, "table height {}", tbl.h);
    // saved as Word again, the list keeps its indents (not the numbering level's defaults)
    let again = dir.join("rules-again.docx");
    export::to_docx(&p, &again).unwrap();
    let wd2 = docx::read(&again).unwrap();
    let li2 = wd2.blocks.iter().filter_map(|b| if let docx::Block::Para(p) = b { Some(p) } else { None }).find(|p| p.p.list.is_some()).unwrap();
    let g = li2.x.list_geom.unwrap();
    assert!(g.0.abs() < 0.1 && (g.1 - 18.0).abs() < 0.1, "list after a save {:?}", g);
}

/// Word sections with their own page (a landscape page between portrait ones), tables kept on
/// one page, exact row heights, keep-with-next: laid out and saved again as Word does.
#[test]
fn word_sections_and_kept_tables() {
    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let sect = |w: u32, h: u32, orient: &str| format!(r#"<w:sectPr><w:pgSz w:w="{w}" w:h="{h}"{orient}/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720"/></w:sectPr>"#);
    let lines = r#"<w:tbl><w:tblPr><w:tblW w:type="auto" w:w="0"/></w:tblPr><w:tblGrid><w:gridCol w:w="9000"/></w:tblGrid>"#.to_string()
        + &(0..3).map(|_| r#"<w:tr><w:trPr><w:trHeight w:val="480" w:hRule="exact"/><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:type="dxa" w:w="9000"/></w:tcPr><w:p><w:pPr><w:keepNext/></w:pPr></w:p></w:tc></w:tr>"#).collect::<String>()
        + "</w:tbl>";
    let doc = format!(
        r#"<w:document {W}><w:body><w:p><w:pPr><w:keepNext/></w:pPr><w:r><w:t>Prompt</w:t></w:r></w:p>{lines}<w:p><w:pPr>{}</w:pPr></w:p><w:p><w:r><w:t>Wide page</w:t></w:r></w:p><w:p><w:pPr>{}</w:pPr></w:p><w:p><w:r><w:t>Tall again</w:t></w:r></w:p>{}</w:body></w:document>"#,
        sect(12240, 15840, ""),
        sect(15840, 12240, r#" w:orient="landscape""#),
        sect(12240, 15840, "")
    );
    let dir = std::env::temp_dir().join("pg-word-rules");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sections.docx");
    write_docx(&path, &[("word/document.xml", &doc)]);
    let wd = docx::read(&path).unwrap();
    assert_eq!(wd.section_setups.len(), 2);
    let t = wd.blocks.iter().find_map(|b| if let docx::Block::Table(t) = b { Some(t) } else { None }).unwrap();
    assert!(t.keep_together && t.exact_rows.iter().all(|e| *e));
    let p = docx::build(&wd);
    // the rows exactly 24 pt (no border added), the table kept whole
    let id = *p.keep_tables.iter().next().unwrap();
    let tm = kn_model::table::table(p.file(), p.file().obj(id).unwrap()).unwrap();
    assert!(tm.row_heights.iter().all(|h| (h - 24.0).abs() < 0.01), "{:?}", tm.row_heights);
    // three sections; the middle one's pages landscape
    assert_eq!(p.sections().len(), 3);
    let l = lay(&p);
    let sizes: Vec<(f32, f32)> = (0..l.pages.len()).map(|i| (l.page_setup(i).width, l.page_setup(i).height)).collect();
    assert_eq!(sizes, vec![(612.0, 792.0), (792.0, 612.0), (612.0, 792.0)]);
    // saved as Word: each section's own page size, the rows exact, the table kept together
    let again = dir.join("sections-again.docx");
    export::to_docx(&p, &again).unwrap();
    let x = {
        let mut z = zip::ZipArchive::new(std::fs::File::open(&again).unwrap()).unwrap();
        let mut s = String::new();
        std::io::Read::read_to_string(&mut z.by_name("word/document.xml").unwrap(), &mut s).unwrap();
        s
    };
    assert!(x.contains(r#"w:orient="landscape""#), "landscape section written");
    assert!(x.contains(r#"w:hRule="exact""#) && x.contains("<w:cantSplit/>") && x.contains("<w:keepNext/>"));
}

/// Word's even-page headers (settings: w:evenAndOddHeaders): on Pages' even-page template,
/// left and right pages apart; saved back as an "even" header with the setting.
#[test]
fn word_even_page_headers() {
    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
    let hdr = |t: &str| format!(r#"<w:hdr {W}><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:hdr>"#);
    let doc = format!(
        r#"<w:document {W}><w:body><w:p><w:r><w:t>One</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rH1"/><w:headerReference w:type="even" r:id="rH2"/><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720"/></w:sectPr></w:body></w:document>"#
    );
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rH1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rH2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header2.xml"/></Relationships>"#;
    let settings = format!(r#"<w:settings {W}><w:evenAndOddHeaders/></w:settings>"#);
    let dir = std::env::temp_dir().join("pg-word-rules");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("evenodd.docx");
    let (h1, h2) = (hdr("Right page"), hdr("Left page"));
    write_docx(&path, &[("word/document.xml", &doc), ("word/_rels/document.xml.rels", rels), ("word/header1.xml", &h1), ("word/header2.xml", &h2), ("word/settings.xml", &settings)]);
    let wd = docx::read(&path).unwrap();
    assert!(wd.even_odd && wd.even_header.is_some());
    let p = docx::build(&wd);
    let s = &p.sections()[0];
    assert!(s.even_odd_different);
    let text = |m: Option<kn_format::ObjId>| p.header_footer_storages(m.unwrap(), false).iter().map(|st| p.doc.storage_text(*st)).collect::<String>();
    assert!(text(s.odd).contains("Right page") && text(s.even).contains("Left page"), "{:?} / {:?}", text(s.odd), text(s.even));
    let again = dir.join("evenodd-again.docx");
    export::to_docx(&p, &again).unwrap();
    let read = |name: &str| {
        let mut z = zip::ZipArchive::new(std::fs::File::open(&again).unwrap()).unwrap();
        let mut s = String::new();
        std::io::Read::read_to_string(&mut z.by_name(name).unwrap(), &mut s).unwrap();
        s
    };
    assert!(read("word/settings.xml").contains("<w:evenAndOddHeaders/>"));
    assert!(read("word/document.xml").contains(r#"w:type="even""#));
}
