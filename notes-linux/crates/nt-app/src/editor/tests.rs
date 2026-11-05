//! Editor behaviour without a window: typing and Notes' auto-format, Return / Delete list rules,
//! styles, checklists, undo, tables, tokens, clipboard and the round trip to the library's model.

use nt_core::model::{self, ParaStyle};

use super::doc::{self, AttKind, Doc, Para};
use super::{Cell, Editor, Loc};

fn ed_with(lines: &[(&str, ParaStyle)]) -> Editor {
    let mut ed = Editor { last_highlight: 3, ..Default::default() };
    let doc = Doc { paras: lines.iter().map(|(t, s)| Para::new(t, *s)).collect(), atts: Vec::new() };
    ed.load_doc("n", doc);
    ed
}

fn at(ed: &mut Editor, para: usize, off: usize) {
    ed.set_caret(Loc { cell: None, para, off }, false);
}

fn typ(ed: &mut Editor, s: &str) {
    for c in s.chars() {
        if c == '\n' {
            ed.return_key();
        } else {
            ed.type_text(&c.to_string());
        }
    }
}

fn texts(ed: &Editor) -> Vec<String> {
    ed.doc.paras.iter().map(|p| p.text.clone()).collect()
}

fn styles(ed: &Editor) -> Vec<ParaStyle> {
    ed.doc.paras.iter().map(|p| p.style).collect()
}

#[test]
fn typing_and_title_then_body() {
    let mut ed = ed_with(&[("", ParaStyle::Title)]);
    typ(&mut ed, "Groceries\nmilk");
    assert_eq!(texts(&ed), ["Groceries", "milk"]);
    assert_eq!(styles(&ed), [ParaStyle::Title, ParaStyle::Body]);
}

#[test]
fn autoformat_lists() {
    // 27 Autoformat: "- " dashed, "* " and "• " bulleted, "1. " and "1) " numbered starting at 1
    for (typed, style, start) in [("- dash", ParaStyle::Dashed, None), ("* star", ParaStyle::Bulleted, None), ("\u{2022} bullet", ParaStyle::Bulleted, None), ("1. one", ParaStyle::Numbered, Some(1)), ("1) paren", ParaStyle::Numbered, Some(1)), ("3. three", ParaStyle::Numbered, Some(3))] {
        let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body)]);
        at(&mut ed, 1, 0);
        typ(&mut ed, typed);
        let p = &ed.doc.paras[1];
        assert_eq!(p.style, style, "{typed}");
        assert_eq!(p.start, start, "{typed}");
        assert_eq!(p.text, typed.split_once(' ').unwrap().1);
    }
    // no conversion for "[] ", "# ", "> "
    for typed in ["[] box", "# hash", "> quote"] {
        let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body)]);
        at(&mut ed, 1, 0);
        typ(&mut ed, typed);
        assert_eq!(ed.doc.paras[1].style, ParaStyle::Body);
        assert_eq!(ed.doc.paras[1].text, typed);
    }
}

#[test]
fn smart_quotes_dashes_ellipsis() {
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    typ(&mut ed, "\"quotes\" -- dash 'x' ... done ---");
    assert_eq!(ed.doc.paras[0].text, "\u{201c}quotes\u{201d} \u{2014} dash \u{2018}x\u{2019} \u{2026} done \u{2014}");
}

#[test]
fn return_continues_and_ends_lists() {
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    typ(&mut ed, "- one\ntwo\n");
    assert_eq!(texts(&ed), ["one", "two", ""]);
    assert_eq!(styles(&ed), [ParaStyle::Dashed, ParaStyle::Dashed, ParaStyle::Dashed]);
    // Return on the empty item ends the list
    ed.return_key();
    assert_eq!(styles(&ed), [ParaStyle::Dashed, ParaStyle::Dashed, ParaStyle::Body]);
    assert_eq!(ed.doc.paras.len(), 3);
}

#[test]
fn return_on_empty_nested_item_outdents() {
    let mut ed = ed_with(&[("a", ParaStyle::Bulleted), ("", ParaStyle::Bulleted)]);
    ed.doc.paras[1].indent = 1;
    at(&mut ed, 1, 0);
    ed.return_key();
    assert_eq!(ed.doc.paras[1].indent, 0);
    assert_eq!(ed.doc.paras[1].style, ParaStyle::Bulleted);
}

#[test]
fn backspace_rules() {
    // at a list item's start the label goes first, then the lines join
    let mut ed = ed_with(&[("a", ParaStyle::Body), ("b", ParaStyle::Bulleted)]);
    at(&mut ed, 1, 0);
    ed.backspace(false);
    assert_eq!(styles(&ed), [ParaStyle::Body, ParaStyle::Body]);
    ed.backspace(false);
    assert_eq!(texts(&ed), ["ab"]);
    assert_eq!(ed.head, Loc { cell: None, para: 0, off: 1 });
    // word backspace
    let mut ed = ed_with(&[("hello big world", ParaStyle::Body)]);
    at(&mut ed, 0, 15);
    ed.backspace(true);
    assert_eq!(texts(&ed), ["hello big "]);
}

#[test]
fn styles_toggle_and_menus() {
    let mut ed = ed_with(&[("a", ParaStyle::Body), ("b", ParaStyle::Body)]);
    ed.anchor = Loc { cell: None, para: 0, off: 0 };
    ed.head = Loc { cell: None, para: 1, off: 1 };
    ed.command_label(None, "Checklist");
    assert_eq!(styles(&ed), [ParaStyle::Checklist, ParaStyle::Checklist]);
    // again: back to Body
    ed.command_label(None, "Checklist");
    assert_eq!(styles(&ed), [ParaStyle::Body, ParaStyle::Body]);
    ed.command_label(None, "Heading");
    assert_eq!(styles(&ed), [ParaStyle::Heading, ParaStyle::Heading]);
    ed.command_label(None, "Block Quote");
    assert!(ed.doc.paras.iter().all(|p| p.quote));
    ed.command_label(None, "Increase");
    assert!(ed.doc.paras.iter().all(|p| p.indent == 1));
    ed.command_label(None, "Center");
    assert!(ed.doc.paras.iter().all(|p| p.align == doc::Align::Center));
    assert_eq!(ed.state().style, Some(ParaStyle::Heading));
}

#[test]
fn bold_with_and_without_selection() {
    let mut ed = ed_with(&[("Hello world", ParaStyle::Body)]);
    ed.anchor = Loc { cell: None, para: 0, off: 6 };
    ed.head = Loc { cell: None, para: 0, off: 11 };
    ed.command_label(None, "Bold");
    let runs: Vec<(String, bool)> = ed.doc.paras[0].spans().map(|(r, f)| (ed.doc.paras[0].text[r].to_string(), f.bold)).collect();
    assert_eq!(runs, [("Hello ".to_string(), false), ("world".to_string(), true)]);
    assert!(ed.state().bold);
    // nothing selected: what is typed next
    at(&mut ed, 0, 11);
    ed.command_label(None, "Bold");
    typ(&mut ed, "!");
    assert!(!ed.doc.paras[0].fmt_of(11).unwrap().bold);
    ed.command_label(None, "Italic");
    typ(&mut ed, "?");
    assert!(ed.doc.paras[0].fmt_of(12).unwrap().italic);
}

#[test]
fn highlight_colours_and_toggle() {
    let mut ed = ed_with(&[("purple", ParaStyle::Body)]);
    ed.anchor = Loc { cell: None, para: 0, off: 0 };
    ed.head = Loc { cell: None, para: 0, off: 6 };
    ed.command_label(None, "Purple");
    assert_eq!(ed.doc.paras[0].fmt_of(0).unwrap().highlight, Some(1));
    ed.command_label(None, "Mint");
    assert_eq!(ed.doc.paras[0].fmt_of(0).unwrap().highlight, Some(4));
    // Format > Highlight (⇧⌘E) takes it off again
    ed.command_label(None, "Highlight");
    assert_eq!(ed.doc.paras[0].fmt_of(0).unwrap().highlight, None);
    ed.command_label(None, "Highlight");
    assert_eq!(ed.doc.paras[0].fmt_of(0).unwrap().highlight, Some(4));
}

#[test]
fn checklist_ticks_sort_to_bottom() {
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("a", ParaStyle::Checklist), ("b", ParaStyle::Checklist), ("c", ParaStyle::Checklist), ("after", ParaStyle::Body)]);
    ed.set_checked(None, 1, true);
    assert_eq!(texts(&ed), ["T", "b", "c", "a", "after"]);
    assert!(ed.doc.paras[3].checked);
    // without sorting
    ed.settings.sort_checked = false;
    ed.set_checked(None, 1, true);
    assert_eq!(texts(&ed), ["T", "b", "c", "a", "after"]);
    assert!(ed.doc.paras[1].checked);
    at(&mut ed, 2, 0);
    ed.command_label(None, "Uncheck All");
    assert!(ed.doc.paras[1..4].iter().all(|p| !p.checked));
    ed.command_label(None, "Check All");
    assert!(ed.doc.paras[1..4].iter().all(|p| p.checked));
    ed.command_label(None, "Delete Checked");
    assert_eq!(texts(&ed), ["T", "after"]);
}

#[test]
fn return_in_checklist_gives_unchecked_item() {
    let mut ed = ed_with(&[("a", ParaStyle::Checklist)]);
    ed.doc.paras[0].checked = true;
    at(&mut ed, 0, 1);
    ed.return_key();
    assert_eq!(styles(&ed), [ParaStyle::Checklist, ParaStyle::Checklist]);
    assert!(ed.doc.paras[0].checked && !ed.doc.paras[1].checked);
}

#[test]
fn undo_merges_typing_and_redo() {
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    typ(&mut ed, "hello");
    ed.set_caret(ed.head, false);
    typ(&mut ed, " there");
    assert_eq!(texts(&ed), ["hello there"]);
    ed.undo();
    assert_eq!(texts(&ed), ["hello"]);
    ed.undo();
    assert_eq!(texts(&ed), [""]);
    ed.redo();
    assert_eq!(texts(&ed), ["hello"]);
    ed.redo();
    assert_eq!(texts(&ed), ["hello there"]);
    // a style change is its own step
    ed.command_label(None, "Title");
    ed.undo();
    assert_eq!(styles(&ed), [ParaStyle::Body]);
}

#[test]
fn tags_and_links_while_typing() {
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    typ(&mut ed, "Hello #brambletest world www.example.org ");
    let p = &ed.doc.paras[0];
    let tag = p.spans().find_map(|(_, f)| f.att.clone()).unwrap();
    assert_eq!(ed.doc.att(&tag).unwrap().kind, AttKind::Tag("#brambletest".into()));
    assert_eq!(p.text, "Hello \u{FFFC} world www.example.org ");
    let off = p.text.find("www").unwrap();
    assert_eq!(p.fmt_of(off).unwrap().link.as_deref(), Some("http://www.example.org"));
    // "#1" isn't a tag
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    typ(&mut ed, "#1 ");
    assert_eq!(ed.doc.paras[0].text, "#1 ");
}

#[test]
fn math_results() {
    use super::math::{eval_line, format};
    let vars = std::collections::HashMap::new();
    assert_eq!(eval_line("2+3*4", &vars), Some(14.0));
    assert_eq!(eval_line("2+3×4", &vars), Some(14.0));
    assert_eq!(eval_line("(1+2)^2", &vars), Some(9.0));
    assert_eq!(eval_line("5", &vars), None);
    assert_eq!(format(2.5), "2.5");
    assert_eq!(format(1.0 / 3.0), "0.3333333333");
    let mut ed = ed_with(&[("x=5", ParaStyle::Body), ("", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    typ(&mut ed, "x*2=");
    ed.update_suggestions_for_test();
    assert_eq!(ed.view.ghost.as_ref().map(|g| g.1.as_str()), Some("10"));
    ed.return_key();
    let p = &ed.doc.paras[1];
    assert_eq!(p.text, "x*2\u{FFFC}");
    let id = p.fmt_of(3).unwrap().att.clone().unwrap();
    assert_eq!(ed.doc.att(&id).unwrap().kind, AttKind::Math("\u{200e}\u{2009}=\u{2009}10".into()));
    assert_eq!(ed.doc.paras.len(), 2, "Return accepted the result instead of a new line");
    // brackets side by side multiply ("2(5)*4=" → 40)
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    typ(&mut ed, "2(5)*4=");
    ed.update_suggestions_for_test();
    assert_eq!(ed.view.ghost.as_ref().map(|g| g.1.as_str()), Some("40"));
}

#[test]
fn tables() {
    let mut ed = ed_with(&[("19 Table", ParaStyle::Title), ("", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    ed.command_label(None, "Table");
    assert_eq!(ed.head.cell, Some(Cell { para: 1, row: 0, col: 0 }));
    typ(&mut ed, "A1");
    ed.tab_key(false);
    typ(&mut ed, "B1");
    ed.tab_key(false);
    typ(&mut ed, "A2");
    let id = ed.doc.paras[1].sole_att().unwrap().to_string();
    let t = ed.doc.table(&id).unwrap();
    assert_eq!((t.rows(), t.cols()), (2, 2));
    assert_eq!([t.cell_text(0, 0), t.cell_text(0, 1), t.cell_text(1, 0), t.cell_text(1, 1)], ["A1", "B1", "A2", ""]);
    // Tab in the last cell adds a row
    ed.tab_key(false);
    ed.tab_key(false);
    assert_eq!(ed.doc.table(&id).unwrap().rows(), 3);
    ed.command_label(None, "Add Column After");
    assert_eq!(ed.doc.table(&id).unwrap().cols(), 3);
    ed.command_label(None, "Delete Row");
    assert_eq!(ed.doc.table(&id).unwrap().rows(), 2);
    ed.command_label(None, "Delete Column");
    assert_eq!(ed.doc.table(&id).unwrap().cols(), 2);
    // the menu's Table inside a table: Convert to Text
    ed.set_caret(Loc { cell: Some(Cell { para: 1, row: 0, col: 0 }), para: 0, off: 0 }, false);
    ed.command_label(None, "Table");
    assert_eq!(texts(&ed)[1..3], ["A1\tB1".to_string(), "A2\t".to_string()]);
}

#[test]
fn table_written_back_to_the_note() {
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    ed.command_label(None, "Table");
    typ(&mut ed, "A1");
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    doc::write_note(&ed.doc, &mut note);
    assert_eq!(note.attachments.len(), 1);
    let a = &note.attachments[0];
    assert_eq!(a.uti, model::UTI_TABLE);
    assert_eq!(a.table.as_ref().unwrap().text_grid(), vec![vec!["A1".to_string(), String::new()], vec![String::new(), String::new()]]);
    assert_eq!(note.body.paragraphs[1].spans[0].token.as_ref().unwrap().id, a.id);
    // read back
    let d2 = doc::from_note(&note);
    let id = d2.paras[1].sole_att().unwrap();
    assert_eq!(d2.table(id).unwrap().cell_text(0, 0), "A1");
}

#[test]
fn round_trip_research_notes() {
    // 21 Tag, 25 Note Link, 20 Divider, 26 Math as the format session reads them
    let tag = model::Token { id: "440C".into(), uti: model::UTI_HASHTAG.into(), alt: "#brambletest".into(), content: "BRAMBLETEST".into() };
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    note.body.paragraphs = vec![
        model::Paragraph::plain(ParaStyle::Title, "21 Tag"),
        model::Paragraph { spans: vec![model::Span::plain("Hello "), model::Span { token: Some(tag.clone()), ..model::Span::token("440C", model::UTI_HASHTAG) }, model::Span::plain(" world")], ..Default::default() },
    ];
    let d = doc::from_note(&note);
    assert_eq!(d.paras[1].text, "Hello \u{FFFC} world");
    let b = doc::to_body(&d);
    assert_eq!(b, note.body);
}

#[test]
fn paste_rich_and_plain() {
    let mut ed = ed_with(&[("one two", ParaStyle::Body), ("three", ParaStyle::Checklist)]);
    ed.anchor = Loc { cell: None, para: 0, off: 4 };
    ed.head = Loc { cell: None, para: 1, off: 5 };
    let (frag, plain) = ed.copy_frag().unwrap();
    assert_eq!(plain, "two\nthree");
    ed.set_caret(Loc { cell: None, para: 1, off: 5 }, false);
    ed.return_key();
    ed.paste_frag(&frag);
    assert_eq!(texts(&ed), ["one two", "three", "two", "three"]);
    assert_eq!(ed.doc.paras[3].style, ParaStyle::Checklist);
    let mut ed = ed_with(&[("", ParaStyle::Bulleted)]);
    ed.paste_plain("a\nb");
    assert_eq!(texts(&ed), ["a", "b"]);
    assert_eq!(styles(&ed), [ParaStyle::Bulleted, ParaStyle::Bulleted]);
}

#[test]
fn collapse_and_move_items() {
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("H", ParaStyle::Heading), ("x", ParaStyle::Body), ("H2", ParaStyle::Heading), ("y", ParaStyle::Body)]);
    at(&mut ed, 2, 1);
    ed.command_label(None, "Collapse Section");
    assert!(ed.doc.paras[1].collapsed);
    assert_eq!(super::layout::hidden_paras(&ed.doc.paras), [false, false, true, false, false]);
    assert_eq!(ed.head.para, 1, "the caret leaves the hidden text");
    ed.command_label(None, "Expand All Sections");
    assert!(!ed.doc.paras.iter().any(|p| p.collapsed));
    ed.command_label(None, "Collapse All Sections");
    assert_eq!(super::layout::hidden_paras(&ed.doc.paras), [false, true, true, true, true]);
    let mut ed = ed_with(&[("a", ParaStyle::Checklist), ("b", ParaStyle::Checklist), ("c", ParaStyle::Checklist)]);
    at(&mut ed, 0, 0);
    ed.command_label(None, "Move Item Down");
    assert_eq!(texts(&ed), ["b", "a", "c"]);
    assert_eq!(ed.head.para, 1);
    ed.command_label(None, "Move Item Up");
    assert_eq!(texts(&ed), ["a", "b", "c"]);
}

#[test]
fn divider_is_a_block_with_a_line_after() {
    let mut ed = ed_with(&[("Hello", ParaStyle::Body)]);
    at(&mut ed, 0, 5);
    ed.command_label(None, "Insert Divider Line");
    assert_eq!(ed.doc.paras.len(), 3);
    assert!(ed.doc.block_of(&ed.doc.paras[1]).is_some());
    assert_eq!(ed.head.para, 2);
    typ(&mut ed, "world");
    assert_eq!(ed.doc.paras[2].text, "world");
    // Backspace at the start of the line after a block removes the block
    at(&mut ed, 2, 0);
    ed.backspace(false);
    assert_eq!(texts(&ed), ["Hello", "world"]);
}

#[test]
fn delete_selection_across_paragraphs_and_typing_over() {
    let mut ed = ed_with(&[("abc", ParaStyle::Title), ("def", ParaStyle::Body), ("ghi", ParaStyle::Body)]);
    ed.anchor = Loc { cell: None, para: 0, off: 1 };
    ed.head = Loc { cell: None, para: 2, off: 2 };
    typ(&mut ed, "X");
    assert_eq!(texts(&ed), ["aXi"]);
    assert_eq!(styles(&ed), [ParaStyle::Title]);
}

#[test]
fn note_link_command_and_state() {
    let mut ed = ed_with(&[("Hello ", ParaStyle::Body)]);
    at(&mut ed, 0, 6);
    ed.command_label(None, "Note Link:B-Target Zqx|applenotes://showNote?identifier=E522");
    let p = &ed.doc.paras[0];
    let id = p.fmt_of(6).unwrap().att.clone().unwrap();
    assert!(matches!(&ed.doc.att(&id).unwrap().kind, AttKind::NoteLink { text, .. } if text == "B-Target Zqx"));
    let b = doc::to_body(&ed.doc);
    let t = b.paragraphs[0].spans.last().unwrap().token.as_ref().unwrap();
    assert_eq!(t.uti, model::UTI_NOTE_LINK);
    assert_eq!(t.alt, "B-Target Zqx");
}

impl Editor {
    fn update_suggestions_for_test(&mut self) {
        // update_suggestions runs after typing in a frame; tests call it directly
        let l = self.head;
        let t = self.para(l).text[..l.off].to_string();
        if t.ends_with('=') {
            let vars = super::math::variables(&self.doc, l);
            if let Some(v) = super::math::eval_line(&t[..t.len() - 1], &vars) {
                self.view.ghost = Some((l, super::math::format(v)));
            }
        }
    }
}

#[test]
fn text_after_a_tag_stays() {
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    typ(&mut ed, "first #tagme www.example.org and");
    assert_eq!(ed.doc.paras[0].text, "first \u{FFFC} www.example.org and");
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    typ(&mut ed, "1. first #tagme www.example.org and");
    assert_eq!(ed.doc.paras[0].text, "first \u{FFFC} www.example.org and");
}

#[test]
fn copy_paste_a_table_block() {
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    ed.command_label(None, "Table");
    typ(&mut ed, "A1");
    // select the table's paragraph and copy it
    let n = ed.doc.paras[1].len();
    ed.anchor = Loc { cell: None, para: 1, off: 0 };
    ed.head = Loc { cell: None, para: 1, off: n };
    let (frag, plain) = ed.copy_frag().unwrap();
    assert!(plain.starts_with("A1\t"));
    let last = ed.doc.paras.len() - 1;
    at(&mut ed, last, 0);
    ed.paste_frag(&frag);
    let tables: Vec<&str> = ed.doc.paras.iter().filter_map(|p| p.sole_att()).collect();
    assert_eq!(tables.len(), 2);
    assert_ne!(tables[0], tables[1], "the copy is a table of its own");
    assert_eq!(ed.doc.table(tables[1]).unwrap().cell_text(0, 0), "A1");
}

/// A long note stays quick to draw and type in (headless egui, release-ish timing printed).
#[test]
fn long_note_frames() {
    let lines: Vec<(String, ParaStyle)> = (0..3000).map(|i| (format!("Line {i} with some words to wrap and a bit more text so it is a real paragraph"), if i % 50 == 0 { ParaStyle::Heading } else if i % 7 == 0 { ParaStyle::Checklist } else { ParaStyle::Body })).collect();
    let refs: Vec<(&str, ParaStyle)> = lines.iter().map(|(t, s)| (t.as_str(), *s)).collect();
    let mut ed = ed_with(&refs);
    let ctx = egui::Context::default();
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let mut times = Vec::new();
    for i in 0..6 {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))), ..Default::default() };
        let t = std::time::Instant::now();
        let mut out = ctx.run_ui(raw, |ui| {
            if i == 4 {
                ed.type_text("x");
            }
            let _ = super::frame(&mut ed, ui, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0)), &mut note, &[]);
        });
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        out.textures_delta.clear();
    }
    eprintln!("frame ms: {times:?}");
    assert!(times[2] < 400.0 && times[5] < 400.0, "{times:?}");
}

#[test]
fn checklist_drag_reorder_and_delayed_sort() {
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("a", ParaStyle::Checklist), ("b", ParaStyle::Checklist), ("c", ParaStyle::Checklist)]);
    ed.doc.paras[2].indent = 0;
    ed.move_para(1, 4);
    assert_eq!(texts(&ed), ["T", "b", "c", "a"]);
    ed.move_para(3, 1);
    assert_eq!(texts(&ed), ["T", "a", "b", "c"]);
    // a nested item travels with its parent
    ed.doc.paras[2].indent = 1;
    ed.move_para(1, 4);
    assert_eq!(texts(&ed), ["T", "c", "a", "b"]);
    // (NO2) a tick moves the item at once to the top of the checked ones; unticking leaves it
    let mut ed = ed_with(&[("a", ParaStyle::Checklist), ("b", ParaStyle::Checklist), ("c", ParaStyle::Checklist), ("d", ParaStyle::Checklist)]);
    ed.set_checked(None, 1, true);
    assert_eq!(texts(&ed), ["a", "c", "d", "b"], "b to the bottom (no checked ones yet)");
    ed.set_checked(None, 0, true);
    assert_eq!(texts(&ed), ["c", "d", "a", "b"], "a above b: the top of the checked ones");
    assert!(!ed.moved.is_empty(), "the window is told what moved");
    ed.set_checked(None, 3, false);
    assert_eq!(texts(&ed), ["c", "d", "a", "b"], "b unticked stays where it is");
    // the caret stays in its item as it moves
    at(&mut ed, 0, 1);
    ed.set_checked(None, 0, true);
    // (b was unticked, so the checked ones start after it)
    assert_eq!(texts(&ed), ["d", "a", "b", "c"]);
    assert_eq!(texts(&ed)[ed.head.para], "c");
    // ticked text isn't greyed (Notes keeps its colour)
    let p = &ed.doc.paras[1];
    assert_eq!(super::layout::run_format(p, &p.fmt_at(0), 1.0).color, super::layout::text_color());
    // Move Checked to Bottom: each kind keeps its order (NO2 2c -> 2e); ⇧⌘U doesn't sort
    let mut ed = ed_with(&[("1", ParaStyle::Checklist), ("2", ParaStyle::Checklist), ("3", ParaStyle::Checklist), ("4", ParaStyle::Checklist)]);
    at(&mut ed, 1, 0);
    ed.command_label(None, "Mark as Checked");
    at(&mut ed, 3, 0);
    ed.command_label(None, "Mark as Checked");
    assert_eq!(texts(&ed), ["1", "2", "3", "4"], "⇧⌘U doesn't re-sort");
    ed.command_label(None, "Move Checked to Bottom");
    assert_eq!(texts(&ed), ["1", "3", "2", "4"]);
    assert_eq!(ed.slide_timing, Some((0.25, 0.317)), "the rows wait 0.25 s, then slide for 0.317 s");
}

#[test]
fn checklist_keys() {
    // Tab / Shift-Tab nest and un-nest, Backspace at the start goes back to Body
    let mut ed = ed_with(&[("a", ParaStyle::Checklist), ("b", ParaStyle::Checklist)]);
    at(&mut ed, 1, 0);
    ed.tab_key(false);
    assert_eq!(ed.doc.paras[1].indent, 1);
    ed.tab_key(true);
    assert_eq!(ed.doc.paras[1].indent, 0);
    // Backspace at the start of an item after another joins them in one step (NO2 "AlphaBeta")
    ed.backspace(false);
    assert_eq!(texts(&ed), ["ab"]);
    assert_eq!(styles(&ed), [ParaStyle::Checklist]);
    ed.undo();
    assert_eq!(texts(&ed), ["a", "b"], "one undo step");
    at(&mut ed, 0, 0);
    ed.backspace(false);
    assert_eq!(styles(&ed), [ParaStyle::Body, ParaStyle::Checklist], "the first item goes back to Body");
    ed.undo();
    // Format > Mark as Checked toggles the selected items
    at(&mut ed, 0, 1);
    ed.settings.sort_checked = false;
    ed.command_label(None, "Mark as Checked");
    assert!(ed.doc.paras[0].checked);
    ed.command_label(None, "Mark as Checked");
    assert!(!ed.doc.paras[0].checked);
}

#[test]
fn offsets_inside_emoji_are_safe() {
    let mut ed = ed_with(&[("world \u{1F44B}\u{1F3FD}", ParaStyle::Body)]);
    let ctx = egui::Context::default();
    let size_of = |_: &str| None;
    let lx = super::layout::Lx { ctx: &ctx, doc: &ed.doc, scale: 1.0, media: &size_of };
    let (_, map) = super::layout::para_job(&lx, &ed.doc.paras[0], 400.0);
    for b in 0..=ed.doc.paras[0].len() {
        let _ = map.to_char(b);
    }
    at(&mut ed, 0, 14);
    ed.backspace(false);
    assert_eq!(ed.doc.paras[0].text, "world \u{1F44B}");
}

/// Emoji runs: pictographs, VS16 sequences (❤️ ✔️ 1️⃣) and emoji-presentation symbols (☕) are
/// drawn untinted from the "emoji" family; text symbols (❤ ✔ ★) and VS15 ones stay text.
#[test]
fn emoji_runs_use_the_colour_font() {
    let ctx = egui::Context::default();
    kn_ui::theme::apply_appearance(&ctx, false);
    let text = "I \u{2764}\u{FE0F} tea \u{2615} \u{2714} \u{2605} \u{2714}\u{FE0F} 1\u{FE0F}\u{20E3} \u{2615}\u{FE0E} \u{1F44B}\u{1F3FD} \u{2764}";
    let ed = ed_with(&[(text, ParaStyle::Body)]);
    let size_of = |_: &str| None;
    let lx = super::layout::Lx { ctx: &ctx, doc: &ed.doc, scale: 1.0, media: &size_of };
    let (job, _) = super::layout::para_job(&lx, &ed.doc.paras[0], 400.0);
    let emoji: Vec<String> = job.sections.iter().filter(|s| s.format.color == egui::Color32::WHITE).map(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].to_string()).collect();
    assert_eq!(emoji, ["\u{2764}\u{FE0F}", "\u{2615}", "\u{2714}\u{FE0F}", "1\u{FE0F}\u{20E3}", "\u{1F44B}\u{1F3FD}"]);
    if kn_ui::theme::has_color_emoji() {
        for s in job.sections.iter().filter(|s| s.format.color == egui::Color32::WHITE) {
            assert_eq!(s.format.font_id.family, egui::FontFamily::Name("emoji".into()));
        }
    }
}

/// Rapid ticks in the window: the model's order is right at once and each item slides from where
/// it was drawn to its new place, eased, without jumps.
#[test]
fn ticked_items_slide_smoothly() {
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("a", ParaStyle::Checklist), ("b", ParaStyle::Checklist), ("c", ParaStyle::Checklist), ("d", ParaStyle::Checklist)]);
    let ctx = egui::Context::default();
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 500.0));
    let mut t = 0.0;
    let mut run = |ed: &mut Editor, t: f64, f: &mut dyn FnMut(&mut Editor)| {
        let raw = egui::RawInput { time: Some(t), screen_rect: Some(rect), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            f(ed);
            let _ = super::frame(ed, ui, rect, &mut note, &[]);
        });
        out.textures_delta.clear();
        let f = ed.view.flow.as_ref().unwrap();
        ed.doc.paras.iter().enumerate().map(|(i, p)| (p.text.clone(), f.boxes[i].top)).collect::<Vec<_>>()
    };
    let rest = run(&mut ed, t, &mut |_| {});
    let top_of = |v: &Vec<(String, f32)>, s: &str| v.iter().find(|x| x.0 == s).unwrap().1;
    let (a0, d0) = (top_of(&rest, "a"), top_of(&rest, "d"));
    // tick a, then b two frames later
    t += 1.0 / 60.0;
    let f1 = run(&mut ed, t, &mut |ed| {
        ed.set_checked(None, 1, true);
    });
    assert_eq!(texts(&ed), ["T", "b", "c", "d", "a"]);
    assert!((top_of(&f1, "a") - a0).abs() < 1.0, "a starts where it was drawn");
    let mut trace = vec![f1.clone()];
    for k in 0..30 {
        t += 1.0 / 60.0;
        let fr = run(&mut ed, t, &mut |ed| {
            if k == 1 {
                ed.set_checked(None, 1, true);
            }
        });
        trace.push(fr);
    }
    // (b goes to the top of the checked ones, above a: NO2)
    assert_eq!(texts(&ed), ["T", "c", "d", "b", "a"]);
    // a goes down without stalling or jumping back; b starts from where it was when ticked
    let ay: Vec<f32> = trace.iter().map(|f| top_of(f, "a")).collect();
    assert!(ay.windows(2).all(|w| w[1] >= w[0] - 0.01), "a only moves down: {ay:?}");
    let steps: Vec<f32> = ay.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(steps.iter().take(4).all(|d| *d > 0.5), "no stall at the start: {steps:?}");
    let last = trace.last().unwrap();
    let pitch = top_of(&rest, "c") - top_of(&rest, "b");
    assert!((top_of(last, "a") - d0).abs() < 0.5, "a ends at the bottom");
    assert!((top_of(last, "b") - (d0 - pitch)).abs() < 0.5, "b ends just above it");
    assert!(ed.view.slides.is_empty(), "the slides finish");
    // typing during a slide goes into the item under the caret
    let _ = top_of(&rest, "b");
}

/// The tests that point XDG_CACHE_HOME somewhere of their own run one at a time.
static CACHE_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn drawings_insert_save_and_mark_up() {
    let _env = CACHE_ENV.lock().unwrap_or_else(|e| e.into_inner());
    use super::ink::{InkKind, InkPoint, PointExt, Stroke, IDENTITY};
    // a cache of our own for the fallback and markup pictures
    let dir = std::env::temp_dir().join(format!("nt-ink-test-{}", std::process::id()));
    // SAFETY: only this test reads XDG_CACHE_HOME
    unsafe { std::env::set_var("XDG_CACHE_HOME", &dir) };
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    ed.command_label(None, "Insert Drawing");
    assert!(ed.draw.is_some(), "drawing at once");
    let line = |c: [f32; 4]| Stroke { id: super::doc::new_uuid(), ink: InkKind::Pen, color: c, width: 3.0, points: (0..20).map(|i| InkPoint::new(20.0 + i as f32 * 10.0, 100.0, i as f32 * 0.01, 3.0)).collect(), transform: IDENTITY, mask: None };
    ed.drawing_mut().unwrap().strokes.push(line([1.0, 0.0, 0.0, 1.0]));
    // while it is drawn on, the strokes are written but the picture waits for Done
    let mut open = model::Note { id: "n".into(), ..Default::default() };
    doc::write_note_drawing(&ed.doc, &mut open, ed.draw.as_ref().map(|d| d.att.as_str()));
    let a = open.attachments.iter().find(|a| a.uti == model::UTI_PAPER).unwrap();
    assert!(a.fallback.is_none() && a.drawing.as_ref().is_some_and(|d| d.strokes.len() == 1));
    ed.finish_drawing();
    assert!(ed.changed, "Done writes the note again");
    doc::write_note(&ed.doc, &mut open);
    assert!(open.attachments.iter().find(|a| a.uti == model::UTI_PAPER).unwrap().fallback.is_some(), "the picture drawn on Done");
    assert!(ed.draw.is_none());
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    doc::write_note(&ed.doc, &mut note);
    let a = note.attachments.iter().find(|a| a.uti == model::UTI_PAPER).expect("a paper attachment");
    assert_eq!((a.width, a.height), (768.0, 358.0));
    let fb = a.fallback.clone().expect("fallback picture written");
    let fb = &fb;
    assert_eq!(a.drawing.as_ref().map(|d| d.strokes.len()), Some(1), "the strokes go to nt-core");
    // reopened: strokes again, not a picture
    let d2 = doc::from_note(&note);
    let id = d2.paras.iter().find_map(|p| p.sole_att().filter(|id| d2.att(id).is_some_and(|a| matches!(a.kind, AttKind::Drawing { .. })))).unwrap();
    assert!(matches!(&d2.att(id).unwrap().kind, AttKind::Drawing { ink: Some(d), .. } if d.strokes.len() == 1));
    // writing again without a change doesn't draw the picture again
    let t0 = std::fs::metadata(fb).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    doc::write_note(&ed.doc, &mut note);
    assert_eq!(std::fs::metadata(fb).unwrap().modified().unwrap(), t0);
    let img = image::open(fb).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (1536, 716));
    assert!(img.get_pixel(200, 200)[0] > 200 && img.get_pixel(200, 200)[3] > 200, "the red line is in it");
    // Markup on a picture: flattened into a new picture, the old one kept for undo
    let pic = dir.join("pic.png");
    image::RgbaImage::from_pixel(100, 50, image::Rgba([255, 255, 255, 255])).save(&pic).unwrap();
    let att = doc::Att::new("public.png", AttKind::Image { path: Some(pic.clone()), width: 100.0, height: 50.0, view: nt_core::model::ViewSize::Large, name: String::new() });
    let old = att.id.clone();
    let mut p = Para::default();
    p.insert(0, "\u{FFFC}", &doc::Fmt { att: Some(old.clone()), ..Default::default() });
    ed.doc.atts.push(att);
    ed.doc.paras.push(p);
    let para = ed.doc.paras.len() - 1;
    at(&mut ed, para, 0);
    ed.command_label(None, "Markup");
    assert!(ed.draw.as_ref().is_some_and(|d| d.markup.is_some()));
    ed.drawing_mut().unwrap().strokes.push(Stroke { points: (0..10).map(|i| InkPoint::new(10.0 + i as f32 * 8.0, 25.0, 0.0, 4.0)).collect(), ..line([0.0, 0.0, 1.0, 1.0]) });
    ed.finish_drawing();
    let now = ed.doc.paras[para].sole_att().unwrap().to_string();
    assert_ne!(now, old);
    let AttKind::Image { path: Some(np), .. } = &ed.doc.att(&now).unwrap().kind else { panic!("a picture") };
    let img = image::open(np).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (100, 50));
    assert!(img.get_pixel(50, 25)[2] > 200 && img.get_pixel(50, 25)[0] < 80, "blue ink flattened in");
    ed.undo();
    assert_eq!(ed.doc.paras[para].sole_att(), Some(old.as_str()), "undo brings the original back");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Drawn here, saved in a library with nt-core, opened again: the same strokes, editable.
#[test]
fn drawing_round_trips_through_the_store() {
    let _env = CACHE_ENV.lock().unwrap_or_else(|e| e.into_inner());
    use super::ink::{Builder, InkKind};
    let dir = std::env::temp_dir().join(format!("nt-ink-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: only this test reads XDG_CACHE_HOME at this point
    unsafe { std::env::set_var("XDG_CACHE_HOME", dir.join("cache")) };
    let mut st = nt_core::store::Store::open(&dir.join("lib")).unwrap();
    let mut body = model::Body::from_lines("Sketch", &[""]);
    let mut note = st.create_note(None, &mut body).unwrap();
    let mut ed = Editor { last_highlight: 3, ..Default::default() };
    ed.load(&note);
    at(&mut ed, 1, 0);
    ed.command_label(None, "Insert Drawing");
    for (k, ink) in [InkKind::Pen, InkKind::Marker, InkKind::Pencil].into_iter().enumerate() {
        let y = 60.0 + k as f32 * 80.0;
        let mut b = Builder::new(ink, [0.2, 0.4, 0.9, 1.0], 3.0, egui::pos2(40.0, y), 0.0, Some(0.7));
        for i in 1..30 {
            b.add(egui::pos2(40.0 + i as f32 * 12.0, y + (i as f32 * 0.4).sin() * 20.0), i as f64 * 0.012, Some(0.5 + i as f32 * 0.01));
        }
        let s = b.finish();
        ed.drawing_mut().unwrap().strokes.push(s);
    }
    ed.finish_drawing();
    let drawn = ed.doc.atts.iter().find_map(|a| match &a.kind {
        AttKind::Drawing { ink: Some(d), .. } => Some((**d).clone()),
        _ => None,
    }).unwrap();
    doc::write_note(&ed.doc, &mut note);
    st.save_note(&mut note).unwrap();
    let back = st.note(&note.id).unwrap();
    let d2 = doc::from_note(&back);
    let again = d2.atts.iter().find_map(|a| match &a.kind {
        AttKind::Drawing { ink: Some(d), .. } => Some((**d).clone()),
        _ => None,
    }).expect("strokes, not just a picture");
    assert_eq!(again, drawn);
    assert_eq!(back.thumbnail().map(|p| p.exists()), Some(true), "the list shows its picture");
    let _ = std::fs::remove_dir_all(&dir);
}

/// While a sheet is open over the window, keys and pastes don't reach the note.
#[test]
fn no_typing_into_the_note_under_a_sheet() {
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("body", ParaStyle::Body)]);
    at(&mut ed, 1, 4);
    ed.focused = true;
    let ctx = egui::Context::default();
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
    let mut run = |ed: &mut Editor, events: Vec<egui::Event>| {
        let raw = egui::RawInput { screen_rect: Some(rect), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, &mut note, &[]);
        });
        out.textures_delta.clear();
    };
    let keys = || vec![egui::Event::Text("secret".into()), egui::Event::Paste("pasted".into()), egui::Event::Key { key: egui::Key::Backspace, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }];
    ed.blocked = true;
    run(&mut ed, keys());
    run(&mut ed, keys());
    assert_eq!(texts(&ed), ["T", "body"], "nothing typed under the sheet");
    ed.blocked = false;
    run(&mut ed, vec![egui::Event::Text("!".into())]);
    assert_eq!(texts(&ed), ["T", "body!"], "typing works again once it closes");
}

/// Drawing speed: a 500-point stroke dragged over a drawing of 50 strokes, then the 51 strokes
/// moved with the lasso; each frame's input, layout, painting and tessellation timed.
/// `cargo test --release -p nt-app --lib editor::tests::drawing_drag_speed -- --ignored --nocapture`
#[test]
#[ignore]
fn drawing_drag_speed() {
    let _env = CACHE_ENV.lock().unwrap_or_else(|e| e.into_inner());
    use super::ink::{InkKind, InkPoint, PointExt, Stroke, IDENTITY};
    let dir = std::env::temp_dir().join(format!("nt-ink-bench-{}", std::process::id()));
    // SAFETY: only this test reads XDG_CACHE_HOME while it holds CACHE_ENV
    unsafe { std::env::set_var("XDG_CACHE_HOME", &dir) };
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    ed.focused = true;
    ed.command_label(None, "Insert Drawing");
    for k in 0..50 {
        let pts = (0..300).map(|i| {
            let t = i as f32 / 300.0;
            InkPoint::new(20.0 + t * 720.0, 30.0 + k as f32 * 6.0 + (t * 40.0).sin() * 12.0, t, 3.0)
        }).collect();
        ed.drawing_mut().unwrap().strokes.push(Stroke { id: super::doc::new_uuid(), ink: if k % 3 == 1 { InkKind::Marker } else { InkKind::Pen }, color: [0.1, 0.2, 0.8, 1.0], width: 3.0, points: pts, transform: IDENTITY, mask: None });
    }
    let ctx = egui::Context::default();
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 800.0));
    let mut t = 0.0;
    let mut run = |ed: &mut Editor, events: Vec<egui::Event>| -> f64 {
        t += 1.0 / 120.0;
        let raw = egui::RawInput { screen_rect: Some(rect), events, time: Some(t), ..Default::default() };
        let t0 = std::time::Instant::now();
        let out = ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, &mut note, &[]);
        });
        let _ = ctx.tessellate(out.shapes, 2.0);
        t0.elapsed().as_secs_f64() * 1000.0
    };
    // (the palette slides in first)
    for _ in 0..40 {
        run(&mut ed, vec![]);
    }
    let para = ed.draw.as_ref().unwrap().para;
    let r = ed.block_rect(para).unwrap();
    let btn = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    let at_ = |i: usize| r.min + egui::vec2(40.0 + (i as f32 / 500.0) * (r.width() - 80.0), 40.0 + ((i as f32) * 0.05).sin() * 60.0 + 100.0);
    let mut times = Vec::new();
    run(&mut ed, vec![egui::Event::PointerMoved(at_(0))]);
    run(&mut ed, vec![btn(at_(0), true)]);
    for i in 1..500 {
        times.push(run(&mut ed, vec![egui::Event::PointerMoved(at_(i))]));
    }
    run(&mut ed, vec![btn(at_(499), false)]);
    let stats = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        format!("mean {mean:.2} ms, median {:.2}, p95 {:.2}, max {:.2}", v[v.len() / 2], v[v.len() * 95 / 100], v[v.len() - 1])
    };
    eprintln!("draw a 500-point stroke over 50: {}", stats(&mut times));
    assert_eq!(ed.drawing().unwrap().strokes.len(), 51);
    // lasso everything, then drag it
    ed.draw.as_mut().unwrap().tool = super::draw::Tool::Lasso;
    let corners = [r.min + egui::vec2(2.0, 2.0), egui::pos2(r.max.x - 2.0, r.min.y + 2.0), r.max - egui::vec2(2.0, 2.0), egui::pos2(r.min.x + 2.0, r.max.y - 2.0), r.min + egui::vec2(2.0, 3.0)];
    run(&mut ed, vec![egui::Event::PointerMoved(corners[0])]);
    run(&mut ed, vec![btn(corners[0], true)]);
    for w in corners.windows(2) {
        for k in 1..=10 {
            run(&mut ed, vec![egui::Event::PointerMoved(w[0] + (w[1] - w[0]) * (k as f32 / 10.0))]);
        }
    }
    run(&mut ed, vec![btn(corners[4], false)]);
    assert_eq!(ed.draw.as_ref().unwrap().selected.len(), 51, "lasso took all");
    let c = r.center();
    let mut moves = Vec::new();
    run(&mut ed, vec![egui::Event::PointerMoved(c)]);
    run(&mut ed, vec![btn(c, true)]);
    for i in 1..200 {
        moves.push(run(&mut ed, vec![egui::Event::PointerMoved(c + egui::vec2((i as f32 * 0.1).sin() * 30.0, i as f32 * 0.1))]));
    }
    run(&mut ed, vec![btn(c, false)]);
    eprintln!("drag 51 lassoed strokes: {}", stats(&mut moves));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Markup's objects: a turned box resized by a corner keeps the opposite corner where it was;
/// a line's end follows the pointer; a star's knob adds points; the turn handle turns.
#[test]
fn markup_shape_handles() {
    use super::ink::ShapeKind;
    use super::shapes::{self, Handle};
    let st = shapes::Style::default();
    let mut s = shapes::new_shape(ShapeKind::Rect, egui::pos2(100.0, 100.0), &st);
    s.rotation = 0.5;
    let f = shapes::frame(&s);
    let fixed = shapes::place(&s, f.left_top());
    let orig = s.clone();
    let corner = shapes::place(&s, f.right_bottom());
    shapes::drag(&mut s, &orig, Handle::Corner(2), corner, corner + egui::vec2(30.0, 10.0), false);
    let now = shapes::place(&s, shapes::frame(&s).left_top());
    assert!((now - fixed).length() < 0.01, "{fixed:?} {now:?}");
    assert!(shapes::frame(&s).width() > f.width());
    // turning
    let o = s.clone();
    let c = shapes::centre(&s);
    shapes::drag(&mut s, &o, Handle::Rotate, c - egui::vec2(0.0, 50.0), c + egui::vec2(50.0, 0.0), false);
    assert!((s.rotation - std::f32::consts::FRAC_PI_2).abs() < 0.01, "{}", s.rotation);
    // a line's end; Shift snaps it to 45°
    let mut l = shapes::new_shape(ShapeKind::Arrow, egui::pos2(100.0, 100.0), &st);
    let o = l.clone();
    shapes::drag(&mut l, &o, Handle::End(1), egui::pos2(170.0, 100.0), egui::pos2(160.0, 158.0), true);
    let (a, b) = (l.points[0], l.points[1]);
    assert!(((b[1] - a[1]) - (b[0] - a[0])).abs() < 0.5, "45°: {a:?} {b:?}");
    assert_eq!(l.arrows, 2);
    // a star's knob
    let mut star = shapes::new_shape(ShapeKind::Star, egui::pos2(100.0, 100.0), &st);
    let o = star.clone();
    shapes::drag(&mut star, &o, Handle::Knob, egui::pos2(0.0, 0.0), egui::pos2(30.0, 0.0), false);
    assert_eq!(star.sides, 7);
    // hits: a border, not the empty middle of an unfilled box; a filled one in the middle
    // (Notes' new shapes are filled grey with no stroke; here one stroked, unfilled)
    let mut r = shapes::new_shape(ShapeKind::Rect, egui::pos2(100.0, 100.0), &st);
    assert_eq!(r.fill[3], 1.0);
    assert_eq!(r.stroke[3], 0.0);
    assert_eq!((r.frame[2], r.frame[3]), (152.0, 152.0));
    r.fill = [0.0; 4];
    r.stroke = [0.0, 0.0, 0.0, 1.0];
    assert!(shapes::hits(&r, egui::pos2(r.frame[0] + 1.0, 100.0), 2.0));
    assert!(!shapes::hits(&r, egui::pos2(100.0, 100.0), 2.0));
    r.fill = [0.0, 0.0, 1.0, 1.0];
    assert!(shapes::hits(&r, egui::pos2(100.0, 100.0), 2.0));
}

/// Markup on a picture: shapes and a text box are flattened into the picture on Done, the text
/// in its colour; undo while marking up takes back the last object.
#[test]
fn markup_objects_flatten_and_undo() {
    let _env = CACHE_ENV.lock().unwrap_or_else(|e| e.into_inner());
    use super::ink::ShapeKind;
    use super::shapes;
    let dir = std::env::temp_dir().join(format!("nt-markup-test-{}", std::process::id()));
    // SAFETY: only this test reads XDG_CACHE_HOME while it holds CACHE_ENV
    unsafe { std::env::set_var("XDG_CACHE_HOME", &dir) };
    std::fs::create_dir_all(&dir).unwrap();
    // egui's fonts, for the text
    let ctx = egui::Context::default();
    ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    shapes::CTX.with_borrow_mut(|c| *c = Some(ctx.clone()));
    let pic = dir.join("pic.png");
    image::RgbaImage::from_pixel(300, 200, image::Rgba([255, 255, 255, 255])).save(&pic).unwrap();
    let mut ed = ed_with(&[("T", ParaStyle::Title)]);
    let att = doc::Att::new("public.png", AttKind::Image { path: Some(pic.clone()), width: 300.0, height: 200.0, view: nt_core::model::ViewSize::Large, name: String::new() });
    let old = att.id.clone();
    let mut p = Para::default();
    p.insert(0, "\u{FFFC}", &doc::Fmt { att: Some(old.clone()), ..Default::default() });
    ed.doc.atts.push(att);
    ed.doc.paras.push(p);
    at(&mut ed, 1, 0);
    ed.command_label(None, "Markup");
    assert_eq!(ed.draw.as_ref().unwrap().tool, super::draw::Tool::Ink(super::ink::InkKind::Monoline), "Markup opens with the Monoline (Notes)");
    // a blue-filled box on the left, red text on the right
    let mut st = shapes::Style { fill: [0.0, 0.0, 1.0, 1.0], ..Default::default() };
    let mut b = shapes::new_shape(ShapeKind::Rect, egui::pos2(0.0, 0.0), &st);
    b.frame = [20.0, 40.0, 80.0, 80.0];
    ed.draw_change();
    ed.drawing_mut().unwrap().shapes.push(b);
    st.text_color = [1.0, 0.0, 0.0, 1.0];
    st.font_size = 40.0;
    let mut t = shapes::new_shape(ShapeKind::Text, egui::pos2(0.0, 0.0), &st);
    t.text = "HH".into();
    t.frame = [150.0, 60.0, 140.0, 60.0];
    ed.draw_change();
    ed.drawing_mut().unwrap().shapes.push(t);
    // undo takes the text away, redo brings it back
    ed.undo();
    assert_eq!(ed.drawing().unwrap().shapes.len(), 1);
    ed.redo();
    assert_eq!(ed.drawing().unwrap().shapes.len(), 2);
    ed.finish_drawing();
    let now = ed.doc.paras[1].sole_att().unwrap().to_string();
    assert_ne!(now, old, "a new picture");
    let AttKind::Image { path: Some(np), .. } = &ed.doc.att(&now).unwrap().kind else { panic!("a picture") };
    let img = image::open(np).unwrap().to_rgba8();
    let px = img.get_pixel(60, 80);
    assert!(px[2] > 200 && px[0] < 60, "the box's blue fill: {px:?}");
    let red = (150..290).flat_map(|x| (60..120).map(move |y| (x, y))).filter(|&(x, y)| {
        let p = img.get_pixel(x, y);
        p[0] > 180 && p[1] < 90 && p[2] < 90
    }).count();
    assert!(red > 100, "the text's red letters are in the picture ({red} red pixels)");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Saved signatures: written to the app's own folder, read back, made into an object.
#[test]
fn signatures_are_saved_per_app() {
    let _env = CACHE_ENV.lock().unwrap_or_else(|e| e.into_inner());
    use super::ink::{InkKind, InkPoint, PointExt, Stroke, IDENTITY};
    let dir = std::env::temp_dir().join(format!("nt-sig-test-{}", std::process::id()));
    // SAFETY: only this test reads XDG_CONFIG_HOME while it holds CACHE_ENV
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
    let s = Stroke { id: "s".into(), ink: InkKind::Pen, color: [0.0, 0.0, 0.0, 1.0], width: 2.0, points: (0..20).map(|i| InkPoint::new(10.0 + i as f32 * 5.0, 50.0 + (i as f32).sin() * 10.0, 0.0, 2.0)).collect(), transform: IDENTITY, mask: None };
    let name = super::draw::save_signature(std::slice::from_ref(&s)).expect("saved");
    assert!(dir.join("bramble-notes/signatures").join(&name).exists());
    let all = super::draw::signatures();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].1.strokes[0].points.len(), 20);
    let o = super::shapes::new_signature(&all[0].1.strokes, egui::pos2(100.0, 100.0), 160.0, [0.0, 0.0, 1.0, 1.0]).unwrap();
    assert_eq!(o.frame[2], 160.0);
    assert!(o.ink[0].points.iter().all(|p| (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y)));
    super::draw::delete_signature(&name);
    assert!(super::draw::signatures().is_empty());
    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    let _ = std::fs::remove_dir_all(&dir);
}

/// The eraser: Pixel takes only what is under it (cutting a line between its points too, and
/// all along a quick sweep), Object takes every stroke and object it touches whatever the size;
/// one undo step per gesture; the choice is remembered in the app's file.
#[test]
fn eraser_pixel_and_object() {
    let _env = CACHE_ENV.lock().unwrap_or_else(|e| e.into_inner());
    use super::draw::{self, Tool};
    use super::ink::{InkKind, InkPoint, PointExt, Stroke, IDENTITY};
    // a line with only its two ends: the pixel eraser cuts it in the middle
    let two = vec![Stroke { id: "a".into(), ink: InkKind::Monoline, color: [0.0, 0.0, 0.0, 1.0], width: 2.0, points: vec![InkPoint::new(0.0, 0.0, 0.0, 2.0), InkPoint::new(200.0, 0.0, 0.0, 2.0)], transform: IDENTITY, mask: None }];
    let mut v = two.clone();
    assert!(super::ink::erase_at(&mut v, egui::pos2(100.0, 0.0), 10.0));
    assert_eq!(v.len(), 2, "cut in two");
    assert!(v[0].points.iter().all(|p| p.x < 89.0) && v[1].points.iter().all(|p| p.x > 111.0), "{:?}", v.iter().map(|s| s.points.len()).collect::<Vec<_>>());
    // in the editor: a sweep across three lines, pixel then object
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 800.0));
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let mut t = 0.0;
    let mut run = |ed: &mut Editor, events: Vec<egui::Event>| {
        t += 1.0 / 60.0;
        let raw = egui::RawInput { screen_rect: Some(rect), events, time: Some(t), ..Default::default() };
        ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, &mut note, &[]);
        }).textures_delta.clear();
    };
    let btn = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    for object in [false, true] {
        let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body)]);
        at(&mut ed, 1, 0);
        ed.focused = true;
        ed.command_label(None, "Insert Drawing");
        for k in 0..3 {
            let y = 60.0 + k as f32 * 40.0;
            ed.drawing_mut().unwrap().strokes.push(Stroke { id: format!("s{k}"), points: (0..=40).map(|i| InkPoint::new(40.0 + i as f32 * 15.0, y, 0.0, 2.0)).collect(), ..two[0].clone() });
        }
        {
            let st = ed.draw.as_mut().unwrap();
            assert_eq!((st.tool, st.eraser_size, st.eraser_object), (Tool::Ink(InkKind::Monoline), 2, false), "Notes' defaults");
            st.tool = if object { Tool::ObjectEraser } else { Tool::PixelEraser };
            st.eraser_size = 4;
        }
        // (the palette slides in first: nothing is drawn or erased until it is in place)
        for _ in 0..25 {
            run(&mut ed, vec![]);
        }
        let (r, k) = draw::canvas(&ed).unwrap();
        let at_ = |x: f32, y: f32| r.min + egui::vec2(x, y) * k;
        let undo_before = ed.draw.as_ref().unwrap().undo.len();
        // one quick sweep down through all three lines at x = 300
        run(&mut ed, vec![egui::Event::PointerMoved(at_(300.0, 30.0))]);
        run(&mut ed, vec![btn(at_(300.0, 30.0), true)]);
        run(&mut ed, vec![egui::Event::PointerMoved(at_(300.0, 170.0))]);
        run(&mut ed, vec![btn(at_(300.0, 170.0), false)]);
        let d = ed.drawing().unwrap();
        if object {
            assert!(d.strokes.is_empty(), "the object eraser takes whole strokes: {}", d.strokes.len());
        } else {
            assert_eq!(d.strokes.len(), 6, "each line cut in two by one sweep");
            assert!(d.strokes.iter().all(|s| s.points.iter().all(|p| (p.x - 300.0).abs() > 20.0)), "nothing left under the eraser");
        }
        assert_eq!(ed.draw.as_ref().unwrap().undo.len(), undo_before + 1, "one undo step for the gesture");
        ed.undo();
        assert_eq!(ed.drawing().unwrap().strokes.len(), 3, "undo brings all three back");
    }
    // remembered: written and read back from the app's own file
    let dir = std::env::temp_dir().join(format!("nt-prefs-test-{}", std::process::id()));
    // SAFETY: only this test reads XDG_CONFIG_HOME while it holds CACHE_ENV
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
    let p = draw::ToolPrefs { tool: Tool::ObjectEraser, eraser_size: 4, eraser_object: true, last_ink: InkKind::Reed, widths: vec![(InkKind::Reed, 3)], color: [0.2, 0.4, 0.6, 1.0] };
    draw::save_prefs(&p);
    assert_eq!(draw::load_prefs(), Some(p));
    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Markup palette slides up from the editor's bottom edge when drawing starts (0.3 s,
/// ease-out) and back down on Done (0.25 s, ease-in), from wherever it is; nothing is drawn
/// until it is in place. Prints the frame trace.
#[test]
fn markup_palette_slides_in_and_out() {
    use super::markup::palette_offset;
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    ed.focused = true;
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 800.0));
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let mut t = 10.0;
    let mut run = |ed: &mut Editor, events: Vec<egui::Event>| -> Option<f32> {
        t += 1.0 / 120.0;
        let raw = egui::RawInput { screen_rect: Some(rect), events, time: Some(t), ..Default::default() };
        ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, &mut note, &[]);
        }).textures_delta.clear();
        palette_offset(ed)
    };
    run(&mut ed, vec![]);
    ed.command_label(None, "Insert Drawing");
    let mut trace = Vec::new();
    // a press on the drawing while it slides in draws nothing
    let (r, _) = (super::draw::canvas(&ed).map(|c| c.0).unwrap_or(rect), 0);
    let p = r.center();
    let btn = |pressed: bool| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    trace.push(run(&mut ed, vec![egui::Event::PointerMoved(p)]).unwrap());
    trace.push(run(&mut ed, vec![btn(true)]).unwrap());
    trace.push(run(&mut ed, vec![egui::Event::PointerMoved(p + egui::vec2(40.0, 0.0))]).unwrap());
    trace.push(run(&mut ed, vec![btn(false)]).unwrap());
    assert!(ed.drawing().unwrap().strokes.is_empty(), "no stroke while the palette slides in");
    for _ in 0..40 {
        trace.push(run(&mut ed, vec![]).unwrap());
    }
    eprintln!("in (pt below its place, every 1/120 s): {:?}", trace.iter().map(|o| (o * 10.0).round() / 10.0).collect::<Vec<_>>());
    assert!(trace[0] > 60.0, "starts out of sight");
    assert!(trace.windows(2).all(|w| w[1] <= w[0]), "only rises");
    // in place after 0.3 s (36 frames at 120 Hz)
    assert_eq!(trace[37], 0.0);
    // ease-out: most of the way in a third of the time
    assert!(trace[12] < trace[0] * 0.35, "{} of {}", trace[12], trace[0]);
    // Done: slides out from there, ease-in, gone in 0.25 s
    ed.finish_drawing();
    let mut out = Vec::new();
    for _ in 0..34 {
        out.push(run(&mut ed, vec![]).unwrap_or(f32::NAN));
    }
    eprintln!("out: {:?}", out.iter().map(|o| (o * 10.0).round() / 10.0).collect::<Vec<_>>());
    assert!(out[0] < 2.0 && out[10] < out[25], "starts in place, falls faster as it goes");
    assert!(out[33].is_nan(), "gone after 0.25 s");
    // Done halfway through the slide in: no jump, it turns back from where it is
    ed.markup();
    let mut mid = Vec::new();
    for _ in 0..8 {
        mid.push(run(&mut ed, vec![]).unwrap());
    }
    let before = *mid.last().unwrap();
    ed.finish_drawing();
    let after = run(&mut ed, vec![]).unwrap();
    eprintln!("Done mid-slide: {before:.1} -> {after:.1}");
    // (one frame of the ease-in at that point: under 6 pt; the position carries on, it doesn't restart)
    assert!((after - before).abs() < 6.0 && after >= before - 0.01, "no jump: {before} -> {after}");
    for _ in 0..40 {
        run(&mut ed, vec![]);
    }
    assert!(palette_offset(&ed).is_none(), "and gone");
}

/// The bug: an arrow added in Markup, Done, then a click on the arrow selected the whole
/// drawing. Now the click opens Markup with the arrow picked up, and dragging moves it (only it);
/// a click on the drawing's empty space opens Markup ready to draw.
#[test]
fn click_on_a_shape_in_a_drawing_drags_it() {
    use super::draw::Tool;
    use super::ink::ShapeKind;
    use super::shapes;
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    ed.focused = true;
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 800.0));
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let mut t = 5.0;
    let mut run = |ed: &mut Editor, events: Vec<egui::Event>| {
        t += 1.0 / 60.0;
        let raw = egui::RawInput { screen_rect: Some(rect), events, time: Some(t), ..Default::default() };
        ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, &mut note, &[]);
        }).textures_delta.clear();
    };
    let btn = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    run(&mut ed, vec![]);
    ed.command_label(None, "Insert Drawing");
    for _ in 0..25 {
        run(&mut ed, vec![]);
    }
    let style = ed.draw.as_ref().unwrap().style.clone();
    ed.add_shape(shapes::new_shape(ShapeKind::Arrow, egui::Pos2::ZERO, &style));
    run(&mut ed, vec![]);
    ed.finish_drawing();
    for _ in 0..25 {
        run(&mut ed, vec![]);
    }
    assert!(ed.draw.is_none());
    let para = ed.doc.paras.iter().position(|p| p.sole_att().is_some_and(|id| matches!(ed.doc.att(id).map(|a| &a.kind), Some(AttKind::Drawing { .. })))).unwrap();
    let r = ed.block_rect(para).unwrap();
    let before = ed.doc.att(ed.doc.paras[para].sole_att().unwrap()).cloned().unwrap();
    let AttKind::Drawing { ink: Some(d0), .. } = &before.kind else { panic!() };
    let a0 = d0.shapes[0].clone();
    let k = r.width() / d0.width;
    let mid = egui::pos2((a0.points[0][0] + a0.points[1][0]) / 2.0, (a0.points[0][1] + a0.points[1][1]) / 2.0);
    let on = r.min + mid.to_vec2() * k;
    // click on the arrow and drag it 60 right, 40 down (screen)
    run(&mut ed, vec![egui::Event::PointerMoved(on)]);
    run(&mut ed, vec![btn(on, true)]);
    {
        let st = ed.draw.as_ref().expect("Markup open");
        assert_eq!(st.tool, Tool::Select);
        assert_eq!(st.selected_shapes, vec![a0.id.clone()], "the arrow is selected, not the drawing");
        assert!(!ed.has_sel(), "no whole-attachment selection");
    }
    for i in 1..=6 {
        run(&mut ed, vec![egui::Event::PointerMoved(on + egui::vec2(10.0, 40.0 / 6.0) * i as f32)]);
    }
    run(&mut ed, vec![btn(on + egui::vec2(60.0, 40.0), false)]);
    let d = ed.drawing().unwrap();
    let a1 = &d.shapes[0];
    for e in 0..2 {
        assert!((a1.points[e][0] - a0.points[e][0] - 60.0 / k).abs() < 0.5 && (a1.points[e][1] - a0.points[e][1] - 40.0 / k).abs() < 0.5, "moved: {:?} -> {:?}", a0.points, a1.points);
    }
    assert_eq!(d.shapes.len(), 1);
    assert_eq!(d.strokes.len(), d0.strokes.len());
    assert_eq!((a1.stroke, a1.line_width, a1.arrows), (a0.stroke, a0.line_width, a0.arrows), "nothing else changed");
    assert_eq!((d.width, d.height), (d0.width, d0.height));
    // Done, then a click on empty space: Markup ready to draw, nothing selected
    ed.finish_drawing();
    for _ in 0..25 {
        run(&mut ed, vec![]);
    }
    let empty = r.min + egui::vec2(30.0, r.height() - 30.0);
    run(&mut ed, vec![egui::Event::PointerMoved(empty)]);
    run(&mut ed, vec![btn(empty, true)]);
    run(&mut ed, vec![btn(empty, false)]);
    let st = ed.draw.as_ref().expect("Markup open");
    assert!(matches!(st.tool, Tool::Ink(_)), "ready to draw: {:?}", st.tool);
    assert!(st.selected_shapes.is_empty() && !ed.has_sel());
    assert_eq!(ed.drawing().unwrap().strokes.len(), 0, "the click itself draws nothing");
}

/// The cut-off rectangle: Markup from the line below a drawing (where Done leaves the caret)
/// made a second drawing, and the shape went half off its top. Now Markup reopens that drawing,
/// objects stay whole on it (dragged past any edge, resized past the right side; the drawing
/// grows below), strokes off the bottom grow it, and after a save and a relaunch a click on a
/// shape picks it up to edit.
#[test]
fn objects_stay_whole_in_their_own_drawing() {
    let _env = CACHE_ENV.lock().unwrap_or_else(|e| e.into_inner());
    use super::draw::Tool;
    use super::ink::ShapeKind;
    use super::shapes;
    let dir = std::env::temp_dir().join(format!("nt-whole-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: only this test reads XDG_CACHE_HOME while it holds CACHE_ENV
    unsafe { std::env::set_var("XDG_CACHE_HOME", dir.join("cache")) };
    let mut store = nt_core::store::Store::open(&dir.join("lib")).unwrap();
    let mut body = model::Body::from_lines("Shapes", &[""]);
    let mut note = store.create_note(None, &mut body).unwrap();
    let mut ed = Editor { last_highlight: 3, ..Default::default() };
    ed.load(&note);
    at(&mut ed, 1, 0);
    ed.focused = true;
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let mut t = 3.0;
    let mut run = |ed: &mut Editor, note: &mut model::Note, events: Vec<egui::Event>| {
        t += 1.0 / 60.0;
        let raw = egui::RawInput { screen_rect: Some(rect), events, time: Some(t), ..Default::default() };
        ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, note, &[]);
        }).textures_delta.clear();
    };
    let settle = |ed: &mut Editor, note: &mut model::Note, run: &mut dyn FnMut(&mut Editor, &mut model::Note, Vec<egui::Event>)| {
        for _ in 0..25 {
            run(ed, note, vec![]);
        }
    };
    let btn = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    let drawings = |ed: &Editor| ed.doc.paras.iter().filter(|p| p.sole_att().and_then(|id| ed.doc.att(id)).is_some_and(|a| matches!(a.kind, AttKind::Drawing { .. }))).count();
    let whole = |ed: &Editor| {
        let d = ed.drawing().unwrap();
        d.shapes.iter().all(|s| {
            let b = shapes::bounds(s);
            b.min.x >= -0.01 && b.min.y >= -0.01 && b.max.x <= d.width + 0.01 && b.max.y <= d.height + 0.01
        })
    };
    // one drawing, an arrow and a rectangle, Done
    ed.command_label(None, "Insert Drawing");
    settle(&mut ed, &mut note, &mut run);
    let style = ed.draw.as_ref().unwrap().style.clone();
    ed.add_shape(shapes::new_shape(ShapeKind::Arrow, egui::Pos2::ZERO, &style));
    ed.add_shape(shapes::new_shape(ShapeKind::RoundRect, egui::Pos2::ZERO, &style));
    ed.finish_drawing();
    settle(&mut ed, &mut note, &mut run);
    assert_eq!(drawings(&ed), 1);
    // Markup again from where Done left the caret: the same drawing
    ed.command_label(None, "Markup");
    settle(&mut ed, &mut note, &mut run);
    assert_eq!(drawings(&ed), 1, "no second drawing");
    assert_eq!(ed.drawing().unwrap().shapes.len(), 2, "the drawing with both objects is open");
    // scrolled so only a sliver of the canvas shows: a new rectangle is still whole
    ed.add_shape(shapes::new_shape(ShapeKind::Rect, egui::Pos2::ZERO, &style));
    assert!(whole(&ed), "new objects whole");
    let rid = ed.drawing().unwrap().shapes[2].id.clone();
    // drag it past each edge
    let (r, k) = super::draw::canvas(&ed).unwrap();
    let h0 = ed.drawing().unwrap().height;
    for target in [egui::vec2(-2000.0, 0.0), egui::vec2(3000.0, 0.0), egui::vec2(0.0, -2000.0), egui::vec2(0.0, 400.0)] {
        let s = ed.drawing().unwrap().shapes.iter().find(|s| s.id == rid).unwrap().clone();
        let c = r.min + shapes::centre(&s).to_vec2() * k;
        ed.draw.as_mut().unwrap().selected_shapes = vec![rid.clone()];
        ed.draw.as_mut().unwrap().tool = Tool::Select;
        run(&mut ed, &mut note, vec![egui::Event::PointerMoved(c)]);
        run(&mut ed, &mut note, vec![btn(c, true)]);
        for i in 1..=5 {
            run(&mut ed, &mut note, vec![egui::Event::PointerMoved(c + target * (i as f32 / 5.0))]);
        }
        run(&mut ed, &mut note, vec![btn(c + target, false)]);
        assert!(whole(&ed), "whole after a drag by {target:?}: {:?}", ed.drawing().unwrap().shapes.iter().find(|s| s.id == rid).unwrap().frame);
    }
    assert!(ed.drawing().unwrap().height > h0, "dragged down, the drawing grew");
    // resize it past the right side by its right edge handle
    let s = ed.drawing().unwrap().shapes.iter().find(|s| s.id == rid).unwrap().clone();
    let (_, hp) = shapes::handles(&s, k).into_iter().find(|h| h.0 == shapes::Handle::Edge(1)).unwrap();
    let hp = r.min + hp.to_vec2() * k;
    run(&mut ed, &mut note, vec![egui::Event::PointerMoved(hp)]);
    run(&mut ed, &mut note, vec![btn(hp, true)]);
    run(&mut ed, &mut note, vec![egui::Event::PointerMoved(hp + egui::vec2(2000.0, 0.0))]);
    run(&mut ed, &mut note, vec![btn(hp + egui::vec2(2000.0, 0.0), false)]);
    assert!(whole(&ed), "whole after a resize past the right side");
    // a stroke drawn off the bottom grows the drawing to hold it
    ed.draw.as_mut().unwrap().tool = Tool::Ink(super::ink::InkKind::Monoline);
    ed.draw.as_mut().unwrap().clear_selection();
    let (r, k) = super::draw::canvas(&ed).unwrap();
    let a = egui::pos2(r.min.x + 40.0, r.max.y - 20.0);
    run(&mut ed, &mut note, vec![egui::Event::PointerMoved(a)]);
    run(&mut ed, &mut note, vec![btn(a, true)]);
    for i in 1..=6 {
        run(&mut ed, &mut note, vec![egui::Event::PointerMoved(a + egui::vec2(10.0, 60.0) * i as f32)]);
    }
    run(&mut ed, &mut note, vec![btn(a + egui::vec2(60.0, 360.0), false)]);
    let d = ed.drawing().unwrap();
    let low = d.strokes.iter().map(|s| super::ink::StrokeExt::bounds(s).max.y).fold(0.0f32, f32::max);
    assert!(low < d.height, "the stroke is inside the grown drawing: {low} < {}", d.height);
    let _ = k;
    // Done, save, relaunch: the objects come back as objects
    ed.finish_drawing();
    settle(&mut ed, &mut note, &mut run);
    doc::write_note(&ed.doc, &mut note);
    store.save_note(&mut note).unwrap();
    drop(store);
    let store = nt_core::store::Store::open(&dir.join("lib")).unwrap();
    let mut back = store.note(&note.id).unwrap();
    let mut ed2 = Editor { last_highlight: 3, ..Default::default() };
    ed2.load(&back);
    ed2.focused = true;
    assert_eq!(drawings(&ed2), 1);
    for _ in 0..3 {
        run(&mut ed2, &mut back, vec![]);
    }
    let para = ed2.doc.paras.iter().position(|p| p.sole_att().and_then(|id| ed2.doc.att(id)).is_some_and(|a| matches!(a.kind, AttKind::Drawing { .. }))).unwrap();
    let AttKind::Drawing { ink: Some(d), .. } = &ed2.doc.att(ed2.doc.paras[para].sole_att().unwrap()).unwrap().kind else { panic!("strokes and objects, not a picture") };
    assert_eq!(d.shapes.len(), 3, "three objects after the relaunch");
    let s = d.shapes.iter().find(|s| s.id == rid).unwrap().clone();
    let r = ed2.block_rect(para).unwrap();
    let k = r.width() / d.width;
    let c = r.min + shapes::centre(&s).to_vec2() * k;
    // click it: selected; Delete takes it away
    run(&mut ed2, &mut back, vec![egui::Event::PointerMoved(c)]);
    run(&mut ed2, &mut back, vec![btn(c, true)]);
    run(&mut ed2, &mut back, vec![btn(c, false)]);
    assert_eq!(ed2.draw.as_ref().map(|d| d.selected_shapes.clone()), Some(vec![rid.clone()]), "picked up after the relaunch");
    for _ in 0..25 {
        run(&mut ed2, &mut back, vec![]);
    }
    run(&mut ed2, &mut back, vec![egui::Event::Key { key: egui::Key::Delete, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
    assert_eq!(ed2.drawing().unwrap().shapes.len(), 2, "and deleted");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Smart Links as Notes': an address becomes a link when a space, Return or punctuation is
/// typed after it (www.…, https://…, a bare domain, an email address); pasted text stays plain
/// (NO1: "pasted plain URL text stayed plain"); Undo right after takes back only the link;
/// editing inside the address moves its target; typing after a link isn't part of it.
#[test]
fn smart_links_as_you_type() {
    let link_at = |ed: &Editor, para: usize, needle: &str| {
        let p = &ed.doc.paras[para];
        let off = p.text.find(needle).unwrap();
        p.fmt_of(off).and_then(|f| f.link.clone())
    };
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    typ(&mut ed, "www.google.com ");
    assert_eq!(link_at(&ed, 0, "www"), Some("http://www.google.com".into()));
    assert_eq!(link_at(&ed, 0, " "), None, "the space isn't linked");
    // Undo: the link goes, the text and the space stay
    ed.undo();
    assert_eq!(ed.doc.paras[0].text, "www.google.com ");
    assert_eq!(link_at(&ed, 0, "www"), None);
    ed.undo();
    assert_ne!(ed.doc.paras[0].text, "www.google.com ", "the next undo takes the typing");
    // Return after a URL on its own line (the user's case)
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    typ(&mut ed, "https://x.y.com/z\nnext");
    assert_eq!(ed.doc.paras.len(), 2);
    assert_eq!(link_at(&ed, 0, "https"), Some("https://x.y.com/z".into()));
    assert_eq!(ed.doc.paras[1].text, "next");
    assert_eq!(link_at(&ed, 1, "next"), None);
    // punctuation, a bare domain, an email; a full stop at the end isn't part of it
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    typ(&mut ed, "see apple.com, mail me@example.org. Not 3.14 or file.txt ");
    assert_eq!(link_at(&ed, 0, "apple"), Some("http://apple.com".into()));
    assert_eq!(link_at(&ed, 0, "me@"), Some("mailto:me@example.org".into()));
    assert_eq!(link_at(&ed, 0, "3.14"), None);
    assert_eq!(link_at(&ed, 0, "file"), None);
    let p = &ed.doc.paras[0];
    let dot = p.text.find("org.").unwrap() + 3;
    assert!(p.fmt_of(dot).unwrap().link.is_none(), "the full stop is outside the link");
    // pasting text with two URLs: plain, as Notes leaves it
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    ed.paste_plain("Read https://a.example.com/x and www.b.org today");
    assert!(ed.doc.paras[0].runs.iter().all(|r| r.fmt.link.is_none()), "pasted URLs stay plain");
    // editing inside: deleting part of the address moves the link; typing after it doesn't extend it
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    typ(&mut ed, "www.apple.com ");
    at(&mut ed, 0, "www.apple.com".len());
    ed.backspace(false);
    assert_eq!(link_at(&ed, 0, "www"), Some("http://www.apple.co".into()), "the target follows the text");
    // typed inside the address: the link follows it
    at(&mut ed, 0, 4);
    typ(&mut ed, "x");
    assert_eq!(link_at(&ed, 0, "www"), Some("http://www.xapple.co".into()));
    let end = ed.doc.paras[0].len();
    at(&mut ed, 0, end);
    typ(&mut ed, "z");
    assert_eq!(link_at(&ed, 0, "z"), None, "typing after the space isn't linked");
    // a link chosen with Add Link (its text not an address) is left alone by edits
    let mut ed = ed_with(&[("click here", ParaStyle::Body)]);
    ed.anchor = Loc { cell: None, para: 0, off: 0 };
    ed.head = Loc { cell: None, para: 0, off: 10 };
    ed.set_link(Some("https://example.com/page".into()));
    at(&mut ed, 0, 5);
    ed.backspace(false);
    assert_eq!(link_at(&ed, 0, "clic"), Some("https://example.com/page".into()), "an Add Link link keeps its target");
}

/// A drawing erased to nothing goes from the note on Done (and Undo brings it back); the note's
/// thumbnail goes with it, after a relaunch too; a drawing deleted with Backspace takes its
/// thumbnail away; each new fallback picture is a new file, the old one removed.
#[test]
fn empty_drawings_and_their_thumbnails_go() {
    let _env = CACHE_ENV.lock().unwrap_or_else(|e| e.into_inner());
    use super::ink::{Builder, InkKind};
    let dir = std::env::temp_dir().join(format!("nt-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: only this test reads XDG_CACHE_HOME while it holds CACHE_ENV
    unsafe { std::env::set_var("XDG_CACHE_HOME", dir.join("cache")) };
    let mut store = nt_core::store::Store::open(&dir.join("lib")).unwrap();
    let mut body = model::Body::from_lines("Sketch", &[""]);
    let mut note = store.create_note(None, &mut body).unwrap();
    let mut ed = Editor { last_highlight: 3, ..Default::default() };
    ed.load(&note);
    at(&mut ed, 1, 0);
    let drawings = |ed: &Editor| ed.doc.paras.iter().filter(|p| p.sole_att().and_then(|id| ed.doc.att(id)).is_some_and(|a| matches!(a.kind, AttKind::Drawing { .. }))).count();
    let stroke = || {
        let mut b = Builder::new(InkKind::Pen, [0.0, 0.0, 0.0, 1.0], 3.0, egui::pos2(40.0, 60.0), 0.0, None);
        for i in 1..20 {
            b.add(egui::pos2(40.0 + i as f32 * 10.0, 60.0 + (i as f32).sin() * 10.0), i as f64 * 0.01, None);
        }
        b.finish()
    };
    // draw, Done: a thumbnail
    ed.command_label(None, "Insert Drawing");
    ed.draw_change();
    ed.drawing_mut().unwrap().strokes.push(stroke());
    ed.finish_drawing();
    doc::write_note(&ed.doc, &mut note);
    let first = note.thumbnail().map(|p| p.to_path_buf()).expect("a thumbnail");
    assert!(first.exists());
    // another stroke: a new picture file, the old one gone
    let para = ed.doc.paras.iter().position(|p| p.sole_att().is_some_and(|id| matches!(ed.doc.att(id).map(|a| &a.kind), Some(AttKind::Drawing { .. })))).unwrap();
    at(&mut ed, para, 0);
    ed.command_label(None, "Markup");
    ed.draw_change();
    ed.drawing_mut().unwrap().strokes.push(stroke());
    ed.finish_drawing();
    doc::write_note(&ed.doc, &mut note);
    let second = note.thumbnail().map(|p| p.to_path_buf()).unwrap();
    assert_ne!(first, second, "a new file each time");
    assert!(second.exists() && !first.exists(), "the old picture removed");
    // erase everything, Done: no drawing, no thumbnail
    at(&mut ed, para, 0);
    ed.command_label(None, "Markup");
    ed.draw_change();
    ed.drawing_mut().unwrap().strokes.clear();
    ed.finish_drawing();
    assert_eq!(drawings(&ed), 0, "the empty drawing went");
    doc::write_note(&ed.doc, &mut note);
    assert!(note.thumbnail().is_none(), "no thumbnail");
    assert!(note.attachments.iter().all(|a| a.uti != model::UTI_PAPER), "nor its attachment");
    store.save_note(&mut note).unwrap();
    drop(store);
    let store = nt_core::store::Store::open(&dir.join("lib")).unwrap();
    let back = store.note(&note.id).unwrap();
    assert!(back.thumbnail().is_none(), "still none after a relaunch");
    assert!(back.attachments.iter().all(|a| a.uti != model::UTI_PAPER));
    // Undo brings the drawing back, and its picture though the library deleted the files
    ed.undo();
    assert_eq!(drawings(&ed), 1, "undo brings it back");
    for a in &note.attachments {
        if let Some(f) = &a.fallback {
            let _ = std::fs::remove_file(f);
        }
    }
    doc::write_note(&ed.doc, &mut note);
    assert!(note.thumbnail().is_some_and(|p| p.exists()), "its picture drawn again");
    // a drawing deleted with Backspace: its thumbnail goes
    let para = ed.doc.paras.iter().position(|p| p.sole_att().is_some_and(|id| matches!(ed.doc.att(id).map(|a| &a.kind), Some(AttKind::Drawing { .. })))).unwrap();
    doc::write_note(&ed.doc, &mut note);
    assert!(note.thumbnail().is_some());
    at(&mut ed, para + 1, 0);
    ed.backspace(false);
    assert_eq!(drawings(&ed), 0);
    doc::write_note(&ed.doc, &mut note);
    assert!(note.thumbnail().is_none(), "Backspace took the drawing and its thumbnail");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Apple's own drawings (NO3 drawings/D*, PaperKit bundles read by nt-core) drawn by our ink next
/// to Apple's FallbackImage: `NT_INK_CMP=dir cargo test -p nt-app --lib editor::tests::apple_drawings_side_by_side -- --ignored`
#[test]
#[ignore]
fn apple_drawings_side_by_side() {
    let out = std::path::PathBuf::from(std::env::var("NT_INK_CMP").expect("NT_INK_CMP=dir"));
    std::fs::create_dir_all(&out).unwrap();
    let root = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("CascadeProjects/notes-research-src/no3/Notes-Research/out-NO3/drawings");
    for e in std::fs::read_dir(&root).unwrap().flatten().filter(|e| e.path().is_dir()) {
        for a in std::fs::read_dir(e.path()).unwrap().flatten().filter(|a| a.path().join("Paper.bundle").exists()) {
            let Some(d) = nt_core::paperkit::read(&a.path().join("Paper.bundle")) else {
                eprintln!("{}: not read", e.file_name().to_string_lossy());
                continue;
            };
            let fb = std::fs::read_dir(a.path()).unwrap().flatten().map(|f| f.path()).find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().contains("FallbackImage")));
            let apple = fb.as_ref().and_then(|p| image::open(p).ok()).map(|i| i.to_rgba8());
            let scale = apple.as_ref().map_or(2.0, |i| i.width() as f32 / d.width.max(1.0));
            let ours = image::load_from_memory(&super::ink::render_png(&d, scale, None)).unwrap().to_rgba8();
            let (w, h) = (ours.width(), ours.height().max(apple.as_ref().map_or(0, |i| i.height())));
            let mut img = image::RgbaImage::from_pixel(w * 2 + 20, h, image::Rgba([255, 255, 255, 255]));
            if let Some(ap) = &apple {
                image::imageops::overlay(&mut img, ap, 0, 0);
            }
            image::imageops::overlay(&mut img, &ours, (w + 20) as i64, 0);
            let name = e.file_name().to_string_lossy().into_owned();
            eprintln!("{name}: {} strokes ({:?}), {} shapes, {}x{} pt, scale {scale:.2}", d.strokes.len(), d.strokes.iter().map(|s| s.ink).collect::<std::collections::HashSet<_>>(), d.shapes.len(), d.width, d.height);
            img.save(out.join(format!("{name}.png"))).unwrap();
        }
    }
}

/// Math as Notes' Math Notes: brackets and nesting, products side by side ("(2)3", "2(3+4)",
/// "2x"), unary minus, ^ and %, functions; results in the note follow a changed variable.
#[test]
fn math_brackets_and_products() {
    use super::math::{eval_line, format};
    let mut vars = std::collections::HashMap::new();
    vars.insert("x".to_string(), 5.0);
    for (e, want) in [
        ("(2)3*3", 18.0),
        ("(2+3)*4", 20.0),
        ("((1+2)*3)^2", 81.0),
        ("2(3+4)", 14.0),
        ("(1+2)(3+4)", 21.0),
        ("2x", 10.0),
        ("3(x+1)", 18.0),
        ("x(2+1)", 15.0),
        ("-(2+3)", -5.0),
        ("2*-3", -6.0),
        ("2^10", 1024.0),
        ("50%", 0.5),
        ("sqrt(16)+1", 5.0),
        ("ln(1)", 0.0),
        ("log(1000)", 3.0),
        ("2+3*4", 14.0),
        ("x*3", 15.0),
    ] {
        assert_eq!(eval_line(e, &vars), Some(want), "{e}");
    }
    assert!((eval_line("2π", &vars).unwrap() - std::f64::consts::TAU).abs() < 1e-9);
    assert_eq!(format(eval_line("2π", &vars).unwrap()), "6.2831853072");
    assert_eq!(eval_line("x", &vars), Some(5.0));
    assert_eq!(eval_line("hello", &vars), None, "a word isn't a calculation");
    assert_eq!(eval_line("(2", &vars), None);
    // an accepted result follows its variable
    let mut ed = ed_with(&[("x=5", ParaStyle::Body), ("", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    typ(&mut ed, "3(x+1)=");
    ed.update_suggestions_for_test();
    assert_eq!(ed.view.ghost.as_ref().map(|g| g.1.as_str()), Some("18"));
    ed.return_key();
    at(&mut ed, 0, 3);
    ed.backspace(false);
    typ(&mut ed, "9");
    ed.refresh_math();
    let id = ed.doc.paras[1].runs.iter().find_map(|r| r.fmt.att.clone()).unwrap();
    assert_eq!(ed.doc.att(&id).unwrap().kind, AttKind::Math("\u{200e}\u{2009}=\u{2009}30".into()), "x = 9 now: 3(9+1) = 30");
}

/// A five-page PDF as Notes 4.13 shows it: Large with every page side by side, Show Thumbnails
/// adding the strip, a thumbnail going to its page, Medium half as tall, Small a card; sideways
/// scrolling (Shift and the wheel) pages through it without moving the note, up and down moves
/// the note and not the pages.
#[test]
fn pdf_pages_side_by_side() {
    use super::pdf::{self, Hit};
    use nt_core::model::ViewSize;
    let pdf_path = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("CascadeProjects/notes-research-src/no3-pdf/data/FallbackPDF.pdf");
    if !pdf_path.exists() || std::process::Command::new("pdfinfo").arg("-v").output().is_err() {
        eprintln!("(no research PDF or pdfinfo here: skipped)");
        return;
    }
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("before", ParaStyle::Body)]);
    for i in 0..30 {
        ed.doc.paras.push(Para::new(&format!("line {i}"), ParaStyle::Body));
    }
    at(&mut ed, 1, 6);
    ed.focused = true;
    ed.attach_file(&pdf_path);
    let id = ed.doc.paras.iter().find_map(|p| p.sole_att().filter(|id| matches!(ed.doc.att(id).map(|a| &a.kind), Some(AttKind::Pdf { .. })))).unwrap().to_string();
    let Some(AttKind::Pdf { pages, view, name, .. }) = ed.doc.att(&id).map(|a| a.kind.clone()) else { panic!() };
    assert_eq!((pages.len(), view, name.as_str()), (5, ViewSize::Large, "FallbackPDF"), "five pages, Large, its name without .pdf");
    assert_eq!(ed.doc.att(&id).unwrap().uti, model::UTI_PAPER_PDF);
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(784.0, 700.0));
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let mut t = 1.0;
    let mut run = |ed: &mut Editor, events: Vec<egui::Event>| {
        t += 1.0 / 60.0;
        let raw = egui::RawInput { screen_rect: Some(rect), events, time: Some(t), ..Default::default() };
        ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, &mut note, &[]);
        }).textures_delta.clear();
    };
    for _ in 0..3 {
        run(&mut ed, vec![]);
    }
    let para = ed.doc.paras.iter().position(|p| p.sole_att() == Some(id.as_str())).unwrap();
    let h = |ed: &Editor| ed.view.flow.as_ref().unwrap().boxes[para].height;
    let large = h(&ed);
    assert!((large - (pdf::HEADER + pdf::PAGE_LARGE + pdf::BOTTOM + super::layout::MEDIA_PAD)).abs() < 1.0, "Large: {large}");
    let hit = |ed: &Editor, want: Hit| ed.media.pdf_hits.iter().find(|h| h.0 == id && h.1 == want).map(|h| h.2.center());
    let btn = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    let click = |ed: &mut Editor, run: &mut dyn FnMut(&mut Editor, Vec<egui::Event>), p: egui::Pos2| {
        run(ed, vec![egui::Event::PointerMoved(p)]);
        run(ed, vec![btn(p, true)]);
        run(ed, vec![btn(p, false)]);
        for _ in 0..40 {
            run(ed, vec![]);
        }
    };
    // Show Thumbnails: the strip
    let p = hit(&ed, Hit::Thumbs).expect("Show Thumbnails");
    click(&mut ed, &mut run, p);
    assert!((h(&ed) - large - (pdf::THUMBS_ABOVE + pdf::THUMB_H + pdf::THUMBS_BELOW)).abs() < 1.0, "the strip adds its height");
    // thumbnail 3: page 3 comes into place
    let p = hit(&ed, Hit::Thumb(2)).expect("thumbnail 3");
    click(&mut ed, &mut run, p);
    let v = ed.media.pdf[&id].clone();
    assert_eq!(v.page, 2, "page 3 shown");
    assert!((v.x - 2.0 * (pdf::PAGE_LARGE * 612.0 / 792.0 + pdf::PAGE_GAP)).abs() < 1.0, "scrolled to page 3: {}", v.x);
    // Shift and the wheel over it: the pages move, the note doesn't
    let over = hit(&ed, Hit::Pages).unwrap();
    let note_y = ed.view.scroll.offset;
    let wheel = |dy: f32, shift: bool| egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, dy), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: egui::Modifiers { shift, ..Default::default() } };
    run(&mut ed, vec![egui::Event::PointerMoved(over)]);
    run(&mut ed, vec![wheel(-120.0, true)]);
    run(&mut ed, vec![]);
    assert!(ed.media.pdf[&id].x > v.x + 100.0, "Shift-wheel pages on");
    assert!((ed.view.scroll.offset - note_y).abs() < 0.5, "the note stayed");
    // up and down: the note moves, the pages don't
    let px = ed.media.pdf[&id].x;
    run(&mut ed, vec![wheel(-200.0, false)]);
    for _ in 0..20 {
        run(&mut ed, vec![]);
    }
    assert!(ed.view.scroll.offset > note_y + 50.0, "the note scrolled");
    assert_eq!(ed.media.pdf[&id].x, px, "the pages didn't");
    // View As Medium (half as tall, the page kept), then Small (a card)
    ed.pdf_view_as(&id, ViewSize::Medium);
    run(&mut ed, vec![]);
    let medium = h(&ed);
    assert!((medium - (pdf::HEADER + pdf::THUMBS_ABOVE + pdf::THUMB_H + pdf::THUMBS_BELOW + pdf::PAGE_MEDIUM + pdf::BOTTOM + super::layout::MEDIA_PAD)).abs() < 1.0, "Medium: {medium}");
    ed.pdf_view_as(&id, ViewSize::Small);
    run(&mut ed, vec![]);
    assert!((h(&ed) - (pdf::CARD.y + super::layout::MEDIA_PAD)).abs() < 1.0, "Small: {}", h(&ed));
    // saved as Notes keeps it
    doc::write_note(&ed.doc, &mut note);
    assert_eq!(note.attachments.iter().find(|a| a.id == id).map(|a| a.view_size), Some(ViewSize::Small));
}



/// Touch, through kn_ui::touch as the window's raw_input_hook does it: one finger dragged up
/// scrolls the note (and coasts on), selecting nothing; a tap places the caret; touch and hold
/// opens the context menu; drawing in Markup, a finger draws.
#[test]
fn touch_scrolls_taps_and_draws() {
    let lines: Vec<(String, ParaStyle)> = (0..80).map(|i| (format!("Line {i} of a long note"), ParaStyle::Body)).collect();
    let refs: Vec<(&str, ParaStyle)> = lines.iter().map(|(t, s)| (t.as_str(), *s)).collect();
    let mut ed = ed_with(&refs);
    at(&mut ed, 0, 0);
    ed.focused = true;
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 600.0));
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let mut t = 1.0;
    let mut run = |ed: &mut Editor, events: Vec<egui::Event>| {
        t += 1.0 / 60.0;
        let mut raw = egui::RawInput { screen_rect: Some(rect), events, time: Some(t), ..Default::default() };
        kn_ui::touch::raw_input(&ctx, &mut raw);
        ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, &mut note, &[]);
        }).textures_delta.clear();
    };
    let touch = |id: u64, phase: egui::TouchPhase, p: egui::Pos2| egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(id), phase, pos: p, force: None };
    for _ in 0..3 {
        run(&mut ed, vec![]);
    }
    // a one-finger drag up by 300 pt
    let a = egui::pos2(300.0, 450.0);
    run(&mut ed, vec![touch(1, egui::TouchPhase::Start, a)]);
    for k in 1..=15 {
        run(&mut ed, vec![touch(1, egui::TouchPhase::Move, a - egui::vec2(0.0, 20.0 * k as f32))]);
    }
    let during = ed.view.scroll.offset;
    run(&mut ed, vec![touch(1, egui::TouchPhase::End, a - egui::vec2(0.0, 300.0))]);
    for _ in 0..60 {
        run(&mut ed, vec![]);
    }
    let after = ed.view.scroll.offset;
    assert!(during > 200.0, "the note followed the finger: {during}");
    assert!(after > during + 20.0, "and coasted on: {during} -> {after}");
    assert!(!ed.has_sel(), "nothing selected");
    assert_eq!(ed.head, Loc { cell: None, para: 0, off: 0 }, "the caret didn't move");
    // a tap: the caret goes there
    let p = egui::pos2(120.0, 300.0);
    run(&mut ed, vec![touch(2, egui::TouchPhase::Start, p)]);
    run(&mut ed, vec![touch(2, egui::TouchPhase::End, p)]);
    for _ in 0..3 {
        run(&mut ed, vec![]);
    }
    assert!(ed.head.para > 5, "the tap placed the caret (para {})", ed.head.para);
    // touch and hold: the context menu
    run(&mut ed, vec![touch(3, egui::TouchPhase::Start, p)]);
    for _ in 0..40 {
        run(&mut ed, vec![]);
    }
    run(&mut ed, vec![touch(3, egui::TouchPhase::End, p)]);
    for _ in 0..3 {
        run(&mut ed, vec![]);
    }
    assert!(egui::Popup::is_any_open(&ctx), "touch and hold opened the menu");
    // Markup: a finger draws
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    ed.focused = true;
    ed.command_label(None, "Insert Drawing");
    for _ in 0..30 {
        run(&mut ed, vec![]);
    }
    let (r, _) = super::draw::canvas(&ed).unwrap();
    // (with the pointer events egui-winit makes of a first finger, which a grabbed area keeps)
    let a = r.min + egui::vec2(60.0, 60.0);
    let btn = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    run(&mut ed, vec![touch(4, egui::TouchPhase::Start, a), egui::Event::PointerMoved(a), btn(a, true)]);
    for k in 1..=12 {
        let q = a + egui::vec2(15.0 * k as f32, 5.0 * k as f32);
        run(&mut ed, vec![touch(4, egui::TouchPhase::Move, q), egui::Event::PointerMoved(q)]);
    }
    let end = a + egui::vec2(180.0, 60.0);
    run(&mut ed, vec![touch(4, egui::TouchPhase::End, end), btn(end, false)]);
    run(&mut ed, vec![]);
    assert_eq!(ed.drawing().unwrap().strokes.len(), 1, "the finger drew a stroke");
}

#[test]
fn not_downloaded_attachments_show_a_card() {
    use super::doc::Missing;
    let mut note = super::snapshot::not_downloaded_note();
    let before = note.attachments.clone();
    let mut ed = Editor::default();
    ed.load(&note);
    let kinds: Vec<(Missing, String)> = ed.doc.paras.iter().filter_map(|p| p.sole_att()).filter_map(|id| match &ed.doc.att(id)?.kind {
        AttKind::Missing { what, .. } => Some((*what, id.to_string())),
        _ => None,
    }).collect();
    assert_eq!(kinds.iter().map(|k| k.0).collect::<Vec<_>>(), [Missing::Scan, Missing::Table, Missing::File, Missing::Drawing, Missing::File], "each with its kind's icon");
    // laid out: a card, the drawing at its own shape across the column
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(784.0, 900.0));
    let mut t = 1.0;
    let mut run = |ed: &mut Editor, note: &mut model::Note, events: Vec<egui::Event>| {
        t += 1.0 / 60.0;
        let raw = egui::RawInput { screen_rect: Some(rect), events, time: Some(t), ..Default::default() };
        ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, note, &[]);
        }).textures_delta.clear();
    };
    for _ in 0..3 {
        run(&mut ed, &mut note, vec![]);
    }
    let para = |ed: &Editor, id: &str| ed.doc.paras.iter().position(|p| p.sole_att() == Some(id)).unwrap();
    let table = ed.block_rect(para(&ed, "ND-TABLE")).unwrap();
    assert!((table.height() - super::layout::FILE_CARD.y * ed.settings.text_scale).abs() < 2.0, "a file card's height: {table:?}");
    let draw = ed.block_rect(para(&ed, "ND-DRAW")).unwrap();
    assert!((draw.height() / draw.width() - 240.0 / 768.0).abs() < 0.01, "the drawing's own shape: {draw:?}");
    // a click: selected, and the alert says why
    let p = table.center();
    let btn = |pressed: bool| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    run(&mut ed, &mut note, vec![egui::Event::PointerMoved(p)]);
    run(&mut ed, &mut note, vec![btn(true)]);
    run(&mut ed, &mut note, vec![btn(false)]);
    let (title, text) = ed.alert.take().expect("an alert");
    assert!(title.contains("hasn’t been downloaded") && text.contains("iCloud"), "{title} / {text}");
    assert_eq!(ed.head.para, para(&ed, "ND-TABLE"));
    // saved: the rows as they were (no empty table written over the stub, no row made for the gone one)
    ed.doc.paras[0].text.push('!');
    doc::write_note(&ed.doc, &mut note);
    assert_eq!(note.attachments, before, "the stubs kept as read");
    assert!(note.body.paragraphs.iter().any(|p| p.spans.iter().any(|s| s.token.as_ref().is_some_and(|t| t.id == "ND-GONE"))), "the token without a row stays in the body");
}

/// The editor's frames on a library's largest notes: the first frame of opening each, then
/// scrolling to its end and typing at its start (NT_BENCH_LIB=<a copy of the app's library>).
#[test]
#[ignore]
fn big_notes_open_fast() {
    let Some(dir) = std::env::var_os("NT_BENCH_LIB") else { return };
    let st = nt_core::store::Store::open(std::path::Path::new(&dir)).unwrap();
    let t0 = std::time::Instant::now();
    let lib = st.library_meta().unwrap();
    eprintln!("library_meta: {} notes, {:.0} ms", lib.notes.len(), t0.elapsed().as_secs_f64() * 1e3);
    let mut notes: Vec<model::Note> = lib.notes.iter().filter(|n| !n.deleted && !n.locked).filter_map(|n| st.note(&n.id).ok()).collect();
    let weight = |n: &model::Note| n.body.paragraphs.iter().map(|p| p.text().len() + 40).sum::<usize>() + n.attachments.len() * 400;
    // (one note by its title)
    if let Ok(t) = std::env::var("NT_BENCH_TITLE") {
        notes.retain(|n| n.title == t);
        for n in &notes {
            for a in &n.attachments {
                eprintln!("  {} {:?} strokes {}", a.uti, a.kind, a.drawing.as_ref().map_or(0, |d| d.strokes.len()));
            }
        }
    }
    let top: usize = std::env::var("NT_BENCH_TOP").ok().and_then(|s| s.parse().ok()).unwrap_or(12);
    // the longest notes, then those with the most attachments
    notes.sort_by_key(|n| std::cmp::Reverse(weight(n)));
    let mut by_atts = notes.split_off(top.min(notes.len()));
    by_atts.sort_by_key(|n| std::cmp::Reverse(n.attachments.len()));
    notes.extend(by_atts.into_iter().take(top));
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(784.0, 900.0));
    let mut t = 1.0;
    let mut run = |ed: &mut Editor, note: &mut model::Note, events: Vec<egui::Event>| -> f64 {
        t += 1.0 / 60.0;
        let raw = egui::RawInput { screen_rect: Some(rect), events, time: Some(t), ..Default::default() };
        let s = std::time::Instant::now();
        ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, note, &[]);
        }).textures_delta.clear();
        s.elapsed().as_secs_f64() * 1e3
    };
    // (fonts loaded on a small note first, as the app has by then)
    let mut warm = model::Note { id: "w".into(), ..Default::default() };
    warm.body = model::Body::from_lines("Warm", &["up"]);
    let mut ed = Editor::default();
    ed.load(&warm);
    for _ in 0..3 {
        run(&mut ed, &mut warm, vec![]);
    }
    let mut worst = 0.0f64;
    for n in &notes {
        let mut note = n.clone();
        let mut ed = Editor::default();
        let s = std::time::Instant::now();
        ed.load(&note);
        let load = s.elapsed().as_secs_f64() * 1e3;
        let first = run(&mut ed, &mut note, vec![]);
        let second = run(&mut ed, &mut note, vec![]);
        // the pictures coming in (read on the workers, a few a frame)
        let mut media = 0.0f64;
        let waited = std::time::Instant::now();
        while (ed.media.busy() || ed.view.cache.estimates()) && waited.elapsed().as_secs_f64() < 5.0 {
            std::thread::sleep(std::time::Duration::from_millis(4));
            media = media.max(run(&mut ed, &mut note, vec![]));
        }
        // scrolling down through all of it
        let mut scroll = 0.0f64;
        for _ in 0..60 {
            scroll = scroll.max(run(&mut ed, &mut note, vec![egui::Event::PointerMoved(rect.center()), egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -2000.0), modifiers: egui::Modifiers::NONE, phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel }]));
        }
        // (NT_BENCH_TITLE: the drawing open too, idle and drawn on)
        if std::env::var_os("NT_BENCH_TITLE").is_some() {
            if let Some(p) = ed.doc.paras.iter().position(|p| p.sole_att().is_some_and(|id| matches!(ed.doc.att(id).map(|a| &a.kind), Some(AttKind::Drawing { .. })))) {
                at(&mut ed, p, 0);
                ed.command_label(None, "Markup");
                let mut open = Vec::new();
                for _ in 0..5 {
                    open.push(run(&mut ed, &mut note, vec![]));
                }
                eprintln!("    drawing open: frames {:?}", open.iter().map(|x| format!("{x:.1}")).collect::<Vec<_>>());
                ed.finish_drawing();
                let done = run(&mut ed, &mut note, vec![]);
                eprintln!("    Done: {done:.1} ms");
            }
        }
        // typing at the start
        at(&mut ed, 0, 0);
        let mut typing = 0.0f64;
        for c in "abcde".chars() {
            ed.type_text(&c.to_string());
            typing = typing.max(run(&mut ed, &mut note, vec![]));
        }
        let paras = n.body.paragraphs.len();
        if first > 8.0 || second > 8.0 {
            let mut l: Vec<usize> = n.body.paragraphs.iter().map(|p| p.text().chars().count()).collect();
            l.sort_unstable_by(|a, b| b.cmp(a));
            eprintln!("    longest paragraphs (chars): {:?}", &l[..l.len().min(6)]);
        }
        if load > 30.0 || first > 30.0 {
            for a in &n.attachments {
                let s = std::time::Instant::now();
                let _ = doc::block_kind(a);
                eprintln!("    {} {:?} pages {} file {} {:.1} ms", a.uti, a.kind, a.pages.len(), a.file.as_ref().map_or(0, |f| std::fs::metadata(f).map_or(0, |m| m.len())), s.elapsed().as_secs_f64() * 1e3);
            }
        }
        eprintln!("{:>5} paras {:>3} atts  load {load:6.1}  first {first:6.1}  next {second:5.1}  settling max {media:5.1} ({:.1} s)  scroll max {scroll:5.1}  typing max {typing:5.1} ms  ({})", paras, n.attachments.len(), waited.elapsed().as_secs_f64(), n.title.chars().take(30).collect::<String>());
        worst = worst.max(load + first).max(second).max(media).max(scroll).max(typing);
    }
    eprintln!("worst frame: {worst:.1} ms");
}

#[test]
fn long_notes_lay_out_in_view_and_match_a_full_layout() {
    // 400 paragraphs of mixed styles and lengths
    let mut lines: Vec<(String, ParaStyle)> = vec![("Long".into(), ParaStyle::Title)];
    for i in 0..400 {
        let style = [ParaStyle::Body, ParaStyle::Heading, ParaStyle::Checklist, ParaStyle::Bulleted, ParaStyle::Numbered, ParaStyle::Monostyled][i % 6];
        lines.push((format!("{i} {}", "word ".repeat(i % 37 * 3)), style));
    }
    let refs: Vec<(&str, ParaStyle)> = lines.iter().map(|(t, s)| (t.as_str(), *s)).collect();
    let mut ed = ed_with(&refs);
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(784.0, 700.0));
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let mut t = 1.0;
    let mut run = |ed: &mut Editor, events: Vec<egui::Event>| {
        t += 1.0 / 60.0;
        let raw = egui::RawInput { screen_rect: Some(rect), events, time: Some(t), ..Default::default() };
        ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, &mut note, &[]);
        }).textures_delta.clear();
    };
    run(&mut ed, vec![]);
    let laid = |ed: &Editor| ed.view.flow.as_ref().unwrap().boxes.iter().filter(|b| b.galley.is_some()).count();
    let first = laid(&ed);
    assert!(first < 400, "the first frame lays out what's near the view, not all of it ({first})");
    // scrolled to the middle at once; the paragraph at the view's top stays put while the rest
    // is laid out
    ed.view.scroll.offset = 6000.0;
    run(&mut ed, vec![]);
    let top_of = |ed: &Editor| {
        let f = ed.view.flow.as_ref().unwrap();
        let y = ed.view.scroll.offset - ed.view.top_pad;
        let b = f.boxes.iter().find(|b| b.bottom() > y).unwrap();
        (b.para, ed.view.origin.y + b.top)
    };
    let (para, y0) = top_of(&ed);
    for _ in 0..200 {
        if !ed.view.cache.estimates() {
            break;
        }
        run(&mut ed, vec![]);
    }
    assert!(!ed.view.cache.estimates(), "all laid out in time");
    let (para2, y1) = top_of(&ed);
    assert_eq!(para2, para, "the same paragraph at the top");
    assert!((y1 - y0).abs() < 0.5, "and in the same place: {y0} -> {y1}");
    // the same as laying out everything
    let full = |ed: &Editor, ctx: &egui::Context| {
        let sizes = std::collections::HashMap::<String, egui::Vec2>::new();
        let size_of = |id: &str| sizes.get(id).copied();
        let lx = super::layout::Lx { ctx, doc: &ed.doc, scale: ed.settings.text_scale, media: &size_of };
        let width = ed.view.content.x - 2.0 * super::layout::MARGIN_X * ed.settings.text_scale;
        super::layout::layout_flow(&lx, &ed.doc.paras, width, true)
    };
    let same = |ed: &Editor, f: &super::layout::Flow| {
        let c = ed.view.flow.as_ref().unwrap();
        assert_eq!(c.boxes.len(), f.boxes.len());
        for (a, b) in c.boxes.iter().zip(&f.boxes) {
            assert!((a.top - b.top).abs() < 0.01 && (a.height - b.height).abs() < 0.01, "para {}: {} {} vs {} {}", a.para, a.top, a.height, b.top, b.height);
            assert_eq!(a.galley.as_ref().map(|g| g.text().to_string()), b.galley.as_ref().map(|g| g.text().to_string()));
        }
    };
    same(&ed, &full(&ed, &ctx));
    // typing in view: only that paragraph again, still the same as a full layout
    let p = para + 2;
    at(&mut ed, p, 0);
    typ(&mut ed, "longer longer longer longer longer longer longer longer longer longer longer longer longer longer ");
    run(&mut ed, vec![]);
    same(&ed, &full(&ed, &ctx));
    // Return and Backspace (the paragraphs after move by one)
    ed.return_key();
    run(&mut ed, vec![]);
    same(&ed, &full(&ed, &ctx));
    ed.backspace(false);
    run(&mut ed, vec![]);
    same(&ed, &full(&ed, &ctx));
}

/// Opening a note (and drawing it a while) changes nothing in it: no edit date moves on until
/// the user edits (NT_BENCH_LIB=<a copy of the app's library>; every note).
#[test]
#[ignore]
fn opening_notes_changes_nothing() {
    let Some(dir) = std::env::var_os("NT_BENCH_LIB") else { return };
    let st = nt_core::store::Store::open(std::path::Path::new(&dir)).unwrap();
    let lib = st.library_meta().unwrap();
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(784.0, 900.0));
    let mut t = 1.0;
    let (mut changed, mut differ, mut n) = (Vec::new(), Vec::new(), 0);
    for m in lib.notes.iter().filter(|n| !n.deleted && !n.locked) {
        let Ok(orig) = st.note(&m.id) else { continue };
        let mut note = orig.clone();
        let mut ed = Editor::default();
        ed.load(&note);
        let mut any = false;
        for _ in 0..4 {
            t += 1.0 / 60.0;
            let raw = egui::RawInput { screen_rect: Some(rect), time: Some(t), ..Default::default() };
            ctx.run_ui(raw, |ui| {
                any |= super::frame(&mut ed, ui, rect, &mut note, &[]).changed;
            }).textures_delta.clear();
        }
        n += 1;
        if any {
            changed.push(orig.title.clone());
        }
        if note != orig {
            differ.push(orig.title.clone());
        }
    }
    eprintln!("{n} notes opened; reported as changed: {} {:?}; note differs: {} {:?}", changed.len(), &changed[..changed.len().min(8)], differ.len(), &differ[..differ.len().min(8)]);
    assert!(changed.is_empty() && differ.is_empty());
}

/// Random typing, keys, clicks, drags, wheel and menu commands on notes of a library copy
/// (NT_BENCH_LIB; NT_MONKEY_NOTES notes, NT_MONKEY_STEPS steps each, NT_MONKEY_SEED): no panic,
/// no frame over 50 ms, every paragraph's runs covering its text.
#[test]
#[ignore]
fn monkey_on_the_library() {
    let Some(dir) = std::env::var_os("NT_BENCH_LIB") else { return };
    // (pictures on the workers, as in the app)
    super::draw::PICTURES_AT_ONCE.store(false, std::sync::atomic::Ordering::Relaxed);
    let num = |k: &str, d: u64| std::env::var(k).ok().and_then(|s| s.parse().ok()).unwrap_or(d);
    let (n_notes, steps, mut seed) = (num("NT_MONKEY_NOTES", 40), num("NT_MONKEY_STEPS", 300), num("NT_MONKEY_SEED", 7));
    let mut rnd = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n.max(1)
    };
    let st = nt_core::store::Store::open(std::path::Path::new(&dir)).unwrap();
    let lib = st.library_meta().unwrap();
    let ids: Vec<String> = lib.notes.iter().filter(|n| !n.deleted && !n.locked).map(|n| n.id.clone()).collect();
    const LABELS: &[&str] = &[
        "Title", "Heading", "Subheading", "Body", "Monostyled", "Bulleted List", "Dashed List", "Numbered List", "Block Quote", "Checklist",
        "Mark as Checked", "Check All", "Uncheck All", "Move Checked to Bottom", "Delete Checked", "Move Item > Up", "Move Item > Down",
        "Indentation > Increase", "Indentation > Decrease", "Align Left", "Center", "Justify", "Align Right", "Table", "Convert to Text",
        "Insert Divider Line", "Bold", "Italic", "Underline", "Strikethrough", "Highlight", "Pink", "Superscript", "Subscript", "Default",
        "Undo", "Redo", "Cut", "Copy", "Paste", "Paste and Match Style", "Select All", "Delete", "Collapse Section", "Expand Section",
        "Collapse All Sections", "Expand All Sections", "Bigger", "Smaller", "Add Row Above", "Add Row Below", "Delete Row",
        "Add Column Before", "Add Column After", "Delete Column", "Copy Style", "Paste Style", "Remove Style", "Remove Link",
        "Copy as Markdown", "Transformations > Make Upper Case", "Transformations > Capitalize", "Find > Find…", "Find > Find Next",
        "Math Results > Insert Results", "Math Results > Suggest Results",
    ];
    let keys = [egui::Key::Enter, egui::Key::Backspace, egui::Key::Delete, egui::Key::Tab, egui::Key::ArrowLeft, egui::Key::ArrowRight, egui::Key::ArrowUp, egui::Key::ArrowDown, egui::Key::Home, egui::Key::End, egui::Key::PageDown, egui::Key::Escape, egui::Key::A, egui::Key::Z, egui::Key::B, egui::Key::L, egui::Key::OpenBracket, egui::Key::CloseBracket];
    let texts = ["a", "word ", "- ", "1. ", "* ", "#tag ", "www.example.com ", "2+3=", "x=5\n", "\"q\" ", "--", "...", "é", "👋🏽", " ", "\u{05e9}\u{05dc}"];
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(784.0, 800.0));
    let mut t = 1.0;
    let mut worst = (0.0f64, String::new());
    let mut slow: Vec<(f64, String)> = Vec::new();
    let mut done = 0;
    for k in 0..n_notes {
        let id = &ids[rnd(ids.len() as u64) as usize];
        let Ok(mut note) = st.note(id) else { continue };
        let title = note.title.chars().take(24).collect::<String>();
        let mut ed = Editor::default();
        ed.focused = true;
        ed.load(&note);
        for step in 0..steps {
            let mods = match rnd(6) {
                0 => egui::Modifiers::COMMAND,
                1 => egui::Modifiers::SHIFT,
                2 => egui::Modifiers { command: true, shift: true, ..Default::default() },
                3 => egui::Modifiers::ALT,
                _ => egui::Modifiers::NONE,
            };
            let p = egui::pos2(rnd(784) as f32, rnd(800) as f32);
            let btn = |pressed: bool, p: egui::Pos2, m: egui::Modifiers| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: m };
            let mut evs: Vec<Vec<egui::Event>> = Vec::new();
            let what = rnd(10);
            let mut label = None;
            match what {
                0..=2 => evs.push(vec![egui::Event::Text(texts[rnd(texts.len() as u64) as usize].replace('\n', ""))]),
                3 | 4 => {
                    let key = keys[rnd(keys.len() as u64) as usize];
                    evs.push(vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods }, egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: mods }]);
                }
                5 => {
                    evs.push(vec![egui::Event::PointerMoved(p)]);
                    evs.push(vec![btn(true, p, mods)]);
                    evs.push(vec![btn(false, p, mods)]);
                }
                6 => {
                    let q = egui::pos2(rnd(784) as f32, rnd(800) as f32);
                    evs.push(vec![egui::Event::PointerMoved(p), btn(true, p, egui::Modifiers::NONE)]);
                    evs.push(vec![egui::Event::PointerMoved(p.lerp(q, 0.5))]);
                    evs.push(vec![egui::Event::PointerMoved(q), btn(false, q, egui::Modifiers::NONE)]);
                }
                7 => evs.push(vec![egui::Event::PointerMoved(p), egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, rnd(3000) as f32 - 1500.0), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: egui::Modifiers::NONE }]),
                _ => label = Some(LABELS[rnd(LABELS.len() as u64) as usize]),
            }
            let what_s = format!("note {k} ({title}, {} paras, longest {}) step {step}: {what} {label:?} {:?}", ed.doc.paras.len(), ed.doc.paras.iter().map(|p| p.text.len()).max().unwrap_or(0), evs.first().map(|e| format!("{e:?}").chars().take(120).collect::<String>()));
            if let Some(l) = label {
                ed.command_label(Some(&ctx), l);
                evs.push(vec![]);
            }
            for e in evs {
                let e_dbg = format!("{e:?}").chars().take(160).collect::<String>();
                t += 1.0 / 60.0;
                let raw = egui::RawInput { screen_rect: Some(rect), events: e, time: Some(t), ..Default::default() };
                let s = std::time::Instant::now();
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    ctx.run_ui(raw, |ui| {
                        let _ = super::frame(&mut ed, ui, rect, &mut note, &[]);
                    }).textures_delta.clear();
                }));
                if r.is_err() {
                    panic!("panicked at {what_s}");
                }
                let ms = s.elapsed().as_secs_f64() * 1e3;
                if let Some(c) = ed.head.cell {
                    let f = ed.doc.paras.get(c.para).and_then(|p| p.sole_att()).and_then(|id| ed.doc.table(id)).and_then(|t| t.cells.get(c.row)?.get(c.col)).map(|f| f.len());
                    assert!(f.is_some_and(|n| ed.head.para < n), "head {:?} in a cell of {f:?} paragraphs after event {e_dbg} of {what_s}", ed.head);
                }
                if ms > 8.0 {
                    let strokes = ed.doc.atts.iter().filter_map(|a| match &a.kind { AttKind::Drawing { ink: Some(d), .. } => Some(d.strokes.len()), _ => None }).sum::<usize>();
                    slow.push((ms, format!("{what_s} [drawing open {} tool {:?} sel {}, markup {}, strokes {strokes}, undo {}, paras {}]", ed.draw.is_some(), ed.draw.as_ref().map(|d| d.tool), ed.draw.as_ref().map_or(0, |d| d.selected.len()), ed.draw.as_ref().is_some_and(|d| d.markup.is_some()), ed.undo.len(), ed.doc.paras.len())));
                }
                if ms > worst.0 {
                    worst = (ms, what_s.clone());
                }
            }
            // the document holds together
            for (i, p) in ed.doc.paras.iter().enumerate() {
                let sum: usize = p.runs.iter().map(|r| r.len).sum();
                assert_eq!(sum, p.text.len(), "para {i}'s runs after {what_s}");
                let mut at = 0;
                for r in &p.runs {
                    assert!(r.len > 0 && p.text.is_char_boundary(at), "para {i}: an empty run or a split character after {what_s}");
                    at += r.len;
                }
            }
            // (in a table cell, the cell's paragraphs)
            let inside = |l: Loc| match l.cell {
                None => l.para < ed.doc.paras.len() && l.off <= ed.doc.paras[l.para].len(),
                Some(c) => ed.doc.paras.get(c.para).and_then(|p| p.sole_att()).and_then(|id| ed.doc.table(id)).and_then(|t| t.cells.get(c.row)?.get(c.col)).is_some_and(|f| l.para < f.len() && l.off <= f[l.para].len()),
            };
            assert!(inside(ed.head) && inside(ed.anchor), "caret outside the note after {what_s}: head {:?} anchor {:?}", ed.head, ed.anchor);
        }
        done += 1;
    }
    slow.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (ms, w) in slow.iter().take(12) {
        eprintln!("  slow {ms:.1} ms: {w}");
    }
    eprintln!("{done} notes x {steps} steps; {} frames over 8 ms; worst frame {:.1} ms at {}", slow.len(), worst.0, worst.1);
    assert!(worst.0 < 50.0, "a frame took {:.1} ms at {}", worst.0, worst.1);
}

/// The fallback picture of the library's drawings: drawing vs PNG encoding (NT_BENCH_LIB).
#[test]
#[ignore]
fn drawing_pictures_fast() {
    let Some(dir) = std::env::var_os("NT_BENCH_LIB") else { return };
    let st = nt_core::store::Store::open(std::path::Path::new(&dir)).unwrap();
    let lib = st.library_meta().unwrap();
    let mut worst = 0.0f64;
    for m in lib.notes.iter().filter(|n| !n.deleted && !n.locked) {
        let Ok(n) = st.note(&m.id) else { continue };
        for a in &n.attachments {
            let Some(d) = a.drawing.as_ref() else { continue };
            let d = d.clone();
            let s = std::time::Instant::now();
            let png = super::ink::render_png(&d, 2.0, None);
            let ms = s.elapsed().as_secs_f64() * 1e3;
            worst = worst.max(ms);
            eprintln!("{:>5.1} ms  {} strokes {} shapes {}x{} -> {} KB", ms, d.strokes.len(), d.shapes.len(), d.width, d.height, png.len() / 1000);
        }
    }
    eprintln!("worst {worst:.1} ms");
}

#[test]
fn drawing_pictures_are_drawn_on_a_worker_and_swapped_in() {
    use super::draw::{apply_picture, fallback_later, take_pictures};
    use super::ink::{Builder, InkKind};
    let mut b = Builder::new(InkKind::Pen, [0.0, 0.0, 0.0, 1.0], 3.0, egui::pos2(40.0, 60.0), 0.0, None);
    for i in 1..30 {
        b.add(egui::pos2(40.0 + i as f32 * 10.0, 60.0 + (i as f32).sin() * 10.0), i as f64 * 0.01, None);
    }
    let mut d = super::ink::Drawing { width: 768.0, height: 300.0, ..Default::default() };
    d.strokes.push(b.finish());
    let id = format!("PIC-{}", std::process::id());
    let old = std::env::temp_dir().join(format!("{id}-old.png"));
    std::fs::write(&old, b"old").unwrap();
    let mut a = model::Attachment { id: id.clone(), drawing: Some(d.clone()), fallback: Some(old.clone()), ..Default::default() };
    // asked twice for the same strokes: drawn once
    fallback_later(None, &id, &d);
    fallback_later(None, &id, &d);
    let mut got = Vec::new();
    for _ in 0..500 {
        got.extend(take_pictures().into_iter().filter(|p| p.id == id));
        if !got.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(20));
            got.extend(take_pictures().into_iter().filter(|p| p.id == id));
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(got.len(), 1, "one picture");
    let pic = got.pop().unwrap();
    assert!(pic.path.exists() && image::open(&pic.path).is_ok(), "a whole PNG");
    assert_eq!(a.fallback.as_deref(), Some(old.as_path()), "the old picture until it's handed over");
    assert!(apply_picture(&pic, &mut a));
    assert_eq!(a.fallback.as_deref(), Some(pic.path.as_path()));
    // (the old one was outside the app's cache: left alone)
    assert!(old.exists());
    // strokes changed meanwhile: not taken
    let mut b2 = a.clone();
    b2.drawing.as_mut().unwrap().strokes.clear();
    assert!(!apply_picture(&pic, &mut b2));
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_file(&pic.path);
}

/// Every menu item the shell sends to the editor (Notes 4.13's menu bar, NO1) is one the
/// editor knows.
#[test]
fn every_editor_menu_item_is_handled() {
    let tree = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("CascadeProjects/notes-research-src/partial-3/Notes-Research/features/menus/menu-tree-raw-editing-note.txt");
    let Ok(text) = std::fs::read_to_string(&tree) else { return };
    // (as menus.rs routes them)
    let prefixes = ["Find > ", "More > ", "Move Item > ", "Font > ", "Baseline > ", "Alignment > ", "Writing Direction > ", "Indentation > ", "Math Results > ", "Attachment View > ", "Spelling and Grammar > ", "Substitutions > ", "Transformations > ", "Highlight > "];
    let plain = ["Undo", "Undo Typing", "Redo", "Cut", "Copy", "Copy as Markdown", "Paste", "Paste and Match Style", "Paste and Retain Style", "Paste as Markdown", "Select All", "Add Link…", "Insert Divider Line", "Rename Attachment…", "Title", "Heading", "Subheading", "Body", "Monostyled", "Bulleted List", "Dashed List", "Numbered List", "Block Quote", "Checklist", "Mark as Checked", "Table", "Convert to Text", "Expand Section", "Expand All Sections", "Collapse Section", "Collapse All Sections", "Markup"];
    let mut names = Vec::new();
    let mut heading = String::new();
    for line in text.lines() {
        // "Writing Direction > \tDefault": an item under the heading before it, named
        // "Writing Direction > Paragraph Default" as the app names it
        if line.contains("Writing Direction > ") {
            let f: Vec<&str> = line.split('\t').collect();
            if f[0].ends_with("> ") {
                names.push(format!("Writing Direction > {heading} {}", f.get(1).copied().unwrap_or("").trim()));
            } else if let Some(h) = f[0].rsplit(" > ").next().filter(|h| *h != "---" && line.contains("[disabled]")) {
                heading = h.trim().to_string();
            }
            continue;
        }
        let path = line.split('\t').next().unwrap_or("").trim();
        let parts: Vec<&str> = path.split(" > ").collect();
        if parts.len() < 2 || parts.last() == Some(&"---") || parts.last().is_some_and(|l| l.is_empty()) {
            continue;
        }
        let (parent, label) = (parts[parts.len() - 2], parts[parts.len() - 1]);
        let name = crate::menus::command_name(parent, label);
        let routed = plain.contains(&name.as_str()) || prefixes.iter().any(|p| name.starts_with(p)) && !matches!(name.as_str(), "More > New Smart Folder with Tag Selection" | "More > Convert Folder to Smart Folder");
        // (submenu headings themselves aren't commands)
        let heading = text.lines().any(|l| l.split('\t').next().unwrap_or("").trim().starts_with(&format!("{path} > ")));
        if routed && !heading && !names.contains(&name) {
            names.push(name);
        }
    }
    let mut missing = Vec::new();
    for n in &names {
        let mut ed = ed_with(&[("Title", ParaStyle::Title), ("one two three", ParaStyle::Checklist), ("four", ParaStyle::Body)]);
        ed.anchor = Loc { cell: None, para: 1, off: 0 };
        ed.head = Loc { cell: None, para: 1, off: 3 };
        super::UNHANDLED.with_borrow_mut(|u| u.clear());
        ed.command_label(None, n);
        if super::UNHANDLED.with_borrow(|u| !u.is_empty()) {
            missing.push(n.clone());
        }
    }
    eprintln!("{} editor menu items; not handled: {missing:?}", names.len());
    assert!(missing.is_empty(), "not handled: {missing:?}");
}

#[test]
fn paste_as_markdown() {
    use super::markdown::{frag_to_markdown, markdown_to_frag};
    let md = "# Trip\n## Packing\n- [ ] tent\n- [x] **stove** and *fuel*\n* socks\n    * wool\n- dashed\n3. third\n4. fourth\n> quoted ~~old~~ ==new==\n```\nlet x = 1;\n```\nsee [Apple](https://apple.com) or <https://example.org>\n---\n| A | B |\n| -- | -- |\n| 1 | 2 |\nsnake_case_name and 2*3*4";
    let f = markdown_to_frag(md);
    let st: Vec<(ParaStyle, &str)> = f.paras.iter().map(|p| (p.style, p.text.as_str())).collect();
    assert_eq!(&st[..4], &[(ParaStyle::Title, "Trip"), (ParaStyle::Heading, "Packing"), (ParaStyle::Checklist, "tent"), (ParaStyle::Checklist, "stove and fuel")]);
    assert!(!f.paras[2].checked && f.paras[3].checked);
    let fmt = |p: &Para, w: &str| p.fmt_of(p.text.find(w).unwrap()).unwrap().clone();
    assert!(fmt(&f.paras[3], "stove").bold && fmt(&f.paras[3], "fuel").italic && !fmt(&f.paras[3], "and").bold);
    assert_eq!((f.paras[4].style, f.paras[5].style, f.paras[5].indent), (ParaStyle::Bulleted, ParaStyle::Bulleted, 1));
    assert_eq!(f.paras[6].style, ParaStyle::Dashed);
    assert_eq!((f.paras[7].style, f.paras[7].start, f.paras[8].start), (ParaStyle::Numbered, Some(3), None));
    assert!(f.paras[9].quote && fmt(&f.paras[9], "old").strike && fmt(&f.paras[9], "new").highlight.is_some());
    assert_eq!((f.paras[10].style, f.paras[10].text.as_str()), (ParaStyle::Monostyled, "let x = 1;"));
    assert_eq!(fmt(&f.paras[11], "Apple").link.as_deref(), Some("https://apple.com"));
    assert_eq!(fmt(&f.paras[11], "https://example.org").link.as_deref(), Some("https://example.org"));
    let kinds: Vec<&AttKind> = f.atts.iter().map(|a| &a.kind).collect();
    assert!(matches!(kinds[0], AttKind::Divider) && matches!(kinds[1], AttKind::Table(t) if t.cell_text(1, 1) == "2"));
    let last = f.paras.last().unwrap();
    // (CommonMark: "_" inside a word is a character, "*" emphasises there too)
    assert_eq!(last.text, "snake_case_name and 234");
    assert!(fmt(last, "3").italic && !fmt(last, "case").italic);
    // our own Copy as Markdown reads back as it was
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("item", ParaStyle::Checklist), ("b", ParaStyle::Bulleted), ("code", ParaStyle::Monostyled), ("end", ParaStyle::Body)]);
    ed.select_all();
    let (frag, _) = ed.copy_frag().unwrap();
    let back = markdown_to_frag(&frag_to_markdown(&ed.doc, &frag));
    assert_eq!(back.paras.iter().map(|p| (p.style, p.text.clone())).collect::<Vec<_>>(), ed.doc.paras.iter().map(|p| (p.style, p.text.clone())).collect::<Vec<_>>());
    // pasted into a note
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    ed.command_label(None, "Paste as Markdown");
    assert!(ed.view.paste_md);
    ed.paste_frag(&markdown_to_frag("# Hi\n- [ ] a"));
    assert_eq!(styles(&ed), [ParaStyle::Title, ParaStyle::Checklist]);
}

#[test]
fn attachment_views_rename_direction_and_spelling() {
    use nt_core::model::ViewSize;
    let pic = std::env::temp_dir().join(format!("nt-view-{}.png", std::process::id()));
    image::RgbaImage::from_pixel(40, 30, image::Rgba([200, 30, 30, 255])).save(&pic).unwrap();
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("x", ParaStyle::Body)]);
    at(&mut ed, 1, 1);
    ed.attach_file(&pic);
    let id = ed.doc.atts.iter().find(|a| matches!(a.kind, AttKind::Image { .. })).unwrap().id.clone();
    let view = |ed: &Editor| match &ed.doc.att(&id).unwrap().kind {
        AttKind::Image { view, .. } => *view,
        _ => unreachable!(),
    };
    assert_eq!(view(&ed), ViewSize::Large);
    ed.command_label(None, "Attachment View > Set All to Small");
    assert_eq!(view(&ed), ViewSize::Small);
    ed.undo();
    assert_eq!(view(&ed), ViewSize::Large, "one undo step");
    // the picture selected: View As, Rename Attachment…
    let para = ed.doc.paras.iter().position(|p| p.sole_att() == Some(id.as_str())).unwrap();
    ed.anchor = Loc { cell: None, para, off: 0 };
    ed.head = Loc { cell: None, para, off: ed.doc.paras[para].len() };
    ed.command_label(None, "View As > Small");
    assert_eq!(view(&ed), ViewSize::Small);
    ed.command_label(None, "Rename Attachment…");
    assert!(ed.media.pdf_rename.as_ref().is_some_and(|(i, n)| *i == id && n.ends_with(".png")), "the field, with its name");
    // written to the library: View As and the name
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    if let Some(AttKind::Image { name, .. }) = ed.doc.att_mut(&id).map(|a| &mut a.kind) {
        *name = "Red square".into();
    }
    doc::write_note(&ed.doc, &mut note);
    let m = note.attachments.iter().find(|a| a.id == id).unwrap();
    assert_eq!((m.view_size, m.name.as_str()), (ViewSize::Small, "Red square"));
    let back = doc::from_note(&note);
    assert!(matches!(&back.att(&id).unwrap().kind, AttKind::Image { view: ViewSize::Small, name, .. } if name == "Red square"));
    let _ = std::fs::remove_file(&pic);
    // Writing Direction: the paragraph's field 3 (zigzag)
    let mut ed = ed_with(&[("abc", ParaStyle::Body)]);
    ed.command_label(None, "Writing Direction > Paragraph Right to Left");
    assert_eq!(ed.doc.paras[0].direction, Some(2));
    ed.command_label(None, "Writing Direction > Paragraph Default");
    assert_eq!(ed.doc.paras[0].direction, Some(1));
    // Check Document Now: the next misspelled word, selected (once the dictionaries are in)
    let t0 = std::time::Instant::now();
    while kn_ui::spell::dicts().is_none() && t0.elapsed().as_secs() < 10 {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if kn_ui::spell::dicts().is_some_and(|d| !d.is_empty()) {
        let mut ed = ed_with(&[("Fine words here", ParaStyle::Body), ("and a mistaek too", ParaStyle::Body)]);
        at(&mut ed, 0, 0);
        ed.command_label(None, "Spelling and Grammar > Check Document Now");
        assert_eq!((ed.anchor.para, &ed.doc.paras[1].text[ed.anchor.off..ed.head.off]), (1, "mistaek"));
    } else {
        eprintln!("(no dictionaries here: Check Document Now not tried)");
    }
}

#[test]
fn table_tab_adds_a_row_and_tables_copy_and_paste() {
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    ed.command_label(None, "Table");
    let (tp, id) = ed.doc.paras.iter().enumerate().find_map(|(i, p)| p.sole_att().filter(|id| ed.doc.table(id).is_some()).map(|id| (i, id.to_string()))).unwrap();
    let (rows, cols) = { let t = ed.doc.table(&id).unwrap(); (t.rows(), t.cols()) };
    ed.head = Loc { cell: Some(Cell { para: tp, row: rows - 1, col: cols - 1 }), para: 0, off: 0 };
    ed.anchor = ed.head;
    ed.tab_key(false);
    assert_eq!(ed.doc.table(&id).unwrap().rows(), rows + 1, "Tab in the last cell adds a row");
    assert_eq!(ed.head.cell.map(|c| (c.row, c.col)), Some((rows, 0)), "the caret in its first cell");
    // the table copied and pasted below: a second table, its own
    let n = ed.doc.paras[tp].len();
    ed.anchor = Loc { cell: None, para: tp, off: 0 };
    ed.head = Loc { cell: None, para: tp, off: n };
    let (frag, plain) = ed.copy_frag().unwrap();
    assert!(plain.contains('\t') || plain.lines().count() >= rows, "as text: its rows ({plain:?})");
    let last = ed.doc.paras.len() - 1;
    let end = ed.doc.paras[last].len();
    at(&mut ed, last, end);
    ed.paste_frag(&frag);
    let tables: Vec<&str> = ed.doc.paras.iter().filter_map(|p| p.sole_att()).filter(|id| ed.doc.table(id).is_some()).collect();
    assert_eq!(tables.len(), 2, "pasted as a table");
    assert_ne!(tables[0], tables[1], "with its own id");
}

#[test]
fn table_rows_and_columns_drag_to_reorder() {
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body), ("after", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    ed.command_label(None, "Table");
    let (tp, id) = ed.doc.paras.iter().enumerate().find_map(|(i, p)| p.sole_att().filter(|id| ed.doc.table(id).is_some()).map(|id| (i, id.to_string()))).unwrap();
    for r in 0..2 {
        for c in 0..2 {
            let t = ed.doc.table_mut(&id).unwrap();
            t.cells[r][c] = vec![Para::new(&format!("r{r}c{c}"), ParaStyle::Body)];
        }
    }
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(784.0, 700.0));
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let mut t = 1.0;
    let mut run = |ed: &mut Editor, events: Vec<egui::Event>| {
        t += 1.0 / 60.0;
        let raw = egui::RawInput { screen_rect: Some(rect), events, time: Some(t), ..Default::default() };
        ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, &mut note, &[]);
        }).textures_delta.clear();
    };
    ed.focused = true;
    ed.head = Loc { cell: Some(Cell { para: tp, row: 0, col: 0 }), para: 0, off: 0 };
    ed.anchor = ed.head;
    for _ in 0..3 {
        run(&mut ed, vec![]);
    }
    let btn = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    let drag = |ed: &mut Editor, run: &mut dyn FnMut(&mut Editor, Vec<egui::Event>), from: egui::Pos2, to: egui::Pos2| {
        run(ed, vec![egui::Event::PointerMoved(from)]);
        run(ed, vec![btn(from, true)]);
        for k in 1..=10 {
            run(ed, vec![egui::Event::PointerMoved(from.lerp(to, k as f32 / 10.0))]);
        }
        run(ed, vec![btn(to, false)]);
        run(ed, vec![]);
    };
    let cell = |ed: &Editor, r: usize, c: usize| ed.doc.table(&id).unwrap().cell_text(r, c);
    // the row handle of row 0 dragged down past row 1: rows swap
    let pb = ed.view.flow.as_ref().unwrap().boxes[tp].clone();
    let super::layout::Block::Table(tb) = pb.block.clone().unwrap() else { panic!() };
    let o = ed.view.origin + egui::vec2(pb.x, pb.top);
    let row_h = egui::pos2(o.x - 9.0, o.y + tb.rows[0].0 + tb.rows[0].1 / 2.0);
    let below = egui::pos2(row_h.x, o.y + tb.rows[1].0 + tb.rows[1].1 * 0.8);
    drag(&mut ed, &mut run, row_h, below);
    assert_eq!((cell(&ed, 0, 0), cell(&ed, 1, 0)), ("r1c0".to_string(), "r0c0".to_string()), "row 0 moved below row 1");
    ed.undo();
    assert_eq!(cell(&ed, 0, 0), "r0c0", "one undo step puts it back");
    // the column handle of column 0 dragged right: columns swap
    ed.head = Loc { cell: Some(Cell { para: tp, row: 0, col: 0 }), para: 0, off: 0 };
    ed.anchor = ed.head;
    ed.table_sel = None;
    run(&mut ed, vec![]);
    let col_h = egui::pos2(o.x + tb.cols[0].0 + tb.cols[0].1 / 2.0, o.y - 9.0);
    let right = egui::pos2(o.x + tb.cols[1].0 + tb.cols[1].1 * 0.8, col_h.y);
    drag(&mut ed, &mut run, col_h, right);
    assert_eq!((cell(&ed, 0, 0), cell(&ed, 0, 1)), ("r0c1".to_string(), "r0c0".to_string()), "column 0 moved right");
}

#[test]
fn move_item_with_a_selection_ending_at_a_paragraph_start() {
    let mut ed = ed_with(&[("a", ParaStyle::Body), ("b", ParaStyle::Body), ("c", ParaStyle::Body)]);
    // "a" and "b" selected, the selection ending at the start of "c"
    ed.anchor = Loc { cell: None, para: 0, off: 0 };
    ed.head = Loc { cell: None, para: 2, off: 0 };
    ed.command_label(None, "Move Item > Down");
    assert_eq!(texts(&ed), ["c", "a", "b"]);
    assert!(ed.head.para < 3 && ed.anchor.para < 3, "the caret in the note: {:?} {:?}", ed.anchor, ed.head);
    assert_eq!((ed.anchor.para, ed.head.para), (1, 0), "on the same paragraphs: a's start, c's start");
    ed.command_label(None, "Move Item > Up");
    assert!(ed.head.para < 3 && ed.anchor.para < 3);
}

#[test]
fn a_drawing_changed_after_it_was_written_is_written_again() {
    use super::ink::{Builder, InkKind};
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("", ParaStyle::Body)]);
    at(&mut ed, 1, 0);
    ed.command_label(None, "Insert Drawing");
    let stroke = |y: f32| {
        let mut b = Builder::new(InkKind::Pen, [0.0, 0.0, 0.0, 1.0], 3.0, egui::pos2(40.0, y), 0.0, None);
        for i in 1..10 {
            b.add(egui::pos2(40.0 + i as f32 * 10.0, y), i as f64 * 0.01, None);
        }
        b.finish()
    };
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let strokes = |note: &model::Note| note.attachments.iter().find_map(|a| a.drawing.as_ref()).map_or(0, |d| d.strokes.len());
    ed.draw_change();
    ed.drawing_mut().unwrap().strokes.push(stroke(60.0));
    ed.finish_drawing();
    doc::write_note(&ed.doc, &mut note);
    assert_eq!(strokes(&note), 1);
    // drawn on again (no undo step of the note's in between): the change is written
    let para = ed.doc.paras.iter().position(|p| p.sole_att().is_some_and(|id| matches!(ed.doc.att(id).map(|a| &a.kind), Some(AttKind::Drawing { .. })))).unwrap();
    at(&mut ed, para, 0);
    ed.command_label(None, "Markup");
    ed.drawing_mut().unwrap().strokes.push(stroke(90.0));
    ed.finish_drawing();
    doc::write_note(&ed.doc, &mut note);
    assert_eq!(strokes(&note), 2, "the second stroke written");
    // written again unchanged: still both
    doc::write_note(&ed.doc, &mut note);
    assert_eq!(strokes(&note), 2);
}

#[test]
fn under_lists_what_the_editor_drew() {
    let mut ed = ed_with(&[("Title here", ParaStyle::Title), ("a line of body text", ParaStyle::Body), ("another", ParaStyle::Body)]);
    ed.date_line = "November 4, 2026 at 9:00 AM".into();
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(784.0, 600.0));
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    for k in 0..3 {
        let raw = egui::RawInput { screen_rect: Some(rect), time: Some(1.0 + k as f64 / 60.0), ..Default::default() };
        ctx.run_ui(raw, |ui| {
            let _ = super::frame(&mut ed, ui, rect, &mut note, &[]);
        }).textures_delta.clear();
    }
    super::EDITOR.with_borrow_mut(|e| *e = std::mem::take(&mut ed));
    let all = super::under(rect);
    assert!(all.len() >= 4, "the date line and three lines: {all:?}");
    let title = all.iter().find(|(r, _)| r.min.y > 20.0).unwrap();
    assert!(title.0.width() > 50.0 && title.0.height() > 10.0, "a line's box: {title:?}");
    // only what's in the given strip
    let strip = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(784.0, 30.0));
    assert!(super::under(strip).iter().all(|(r, _)| r.intersects(strip)));
    super::EDITOR.with_borrow_mut(|e| *e = Editor::default());
}

#[test]
fn audio_cards_play_and_pause() {
    // half a second of 440 Hz, 16-bit mono WAV
    let tone = std::env::temp_dir().join(format!("nt-tone-{}.wav", std::process::id()));
    {
        let (rate, n) = (44100u32, 22050u32);
        let mut b: Vec<u8> = Vec::new();
        b.extend(b"RIFF");
        b.extend((36 + n * 2).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * 2).to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend((n * 2).to_le_bytes());
        for i in 0..n {
            let v = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 8000.0) as i16;
            b.extend(v.to_le_bytes());
        }
        std::fs::write(&tone, b).unwrap();
    }
    let mut ed = ed_with(&[("T", ParaStyle::Title), ("x", ParaStyle::Body)]);
    at(&mut ed, 1, 1);
    ed.attach_file(&tone);
    let att = ed.doc.atts.iter().find(|a| super::media::is_audio(&a.uti)).expect("an audio attachment").clone();
    assert!(matches!(&att.kind, AttKind::File { kind, .. } if kind == "Audio"));
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(784.0, 600.0));
    let mut note = model::Note { id: "n".into(), ..Default::default() };
    let mut t = 1.0;
    let mut run = |ed: &mut Editor, events: Vec<egui::Event>| {
        t += 1.0 / 60.0;
        let raw = egui::RawInput { screen_rect: Some(rect), events, time: Some(t), ..Default::default() };
        ctx.run_ui(raw, |ui| {
            let _ = super::frame(ed, ui, rect, &mut note, &[]);
        }).textures_delta.clear();
    };
    // its length, read on the workers
    let mut len = None;
    for _ in 0..300 {
        run(&mut ed, vec![]);
        len = ed.media.audio_hits.iter().find(|h| h.0 == att.id).and_then(|h| h.4);
        if len.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let len = len.unwrap_or_else(|| panic!("its length: hits {:?}; info {:?}", ed.media.audio_hits, crate::audio::info(&tone)));
    assert!((len - 0.5).abs() < 0.1, "{len}");
    // Play, then Pause
    let button = ed.media.audio_hits.iter().find(|h| h.0 == att.id).unwrap().1.center();
    let btn = |pressed: bool| egui::Event::PointerButton { pos: button, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    let wait = |f: &dyn Fn(&super::player::State) -> bool| {
        for _ in 0..100 {
            if f(&super::player::state()) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    };
    ed.focused = true;
    run(&mut ed, vec![egui::Event::PointerMoved(button)]);
    run(&mut ed, vec![btn(true)]);
    run(&mut ed, vec![btn(false)]);
    assert!(wait(&|s| s.playing && s.path.as_deref() == Some(tone.as_path())), "playing");
    run(&mut ed, vec![btn(true)]);
    run(&mut ed, vec![btn(false)]);
    assert!(wait(&|s| !s.playing && s.path.is_some()), "paused");
    super::player::stop();
    assert!(wait(&|s| s.path.is_none()));
    assert_eq!(super::player::clock(65.4), "1:05");
    let _ = std::fs::remove_file(&tone);
}

#[test]
fn find_and_replace() {
    let mut ed = ed_with(&[("Shopping", ParaStyle::Title), ("Apples, apples and pears", ParaStyle::Body)]);
    ed.find_command("Find and Replace");
    assert!(ed.find.open && ed.find.replace);
    ed.find.query = "apples".into();
    ed.find.with = "plums".into();
    assert_eq!(ed.find_matches().len(), 2);
    // one at a time: the first match goes
    ed.replace_one();
    assert_eq!(ed.doc.paras[1].text, "plums, apples and pears");
    // the rest at once, undone in one step
    ed.find.query = "pear".into();
    ed.find.with = "fig".into();
    ed.replace_all();
    assert_eq!(ed.doc.paras[1].text, "plums, apples and figs");
    ed.undo();
    assert_eq!(ed.doc.paras[1].text, "plums, apples and pears");
}

#[test]
fn backspace_takes_an_autocorrection_back() {
    // (the dictionaries load in the background)
    for _ in 0..200 {
        if kn_ui::spell::dicts().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if !kn_ui::spell::dicts().is_some_and(|d| !d.is_empty()) {
        return;
    }
    let mut ed = ed_with(&[("", ParaStyle::Body)]);
    ed.settings.autocorrect = true;
    typ(&mut ed, "teh ");
    assert_eq!(ed.doc.paras[0].text, "the ");
    ed.backspace(false);
    assert_eq!(ed.doc.paras[0].text, "teh");
    // the space now leaves it as typed, and so does the next time
    typ(&mut ed, " cat teh ");
    assert_eq!(ed.doc.paras[0].text, "teh cat teh ");
    // an ordinary Backspace still deletes one character
    ed.backspace(false);
    assert_eq!(ed.doc.paras[0].text, "teh cat teh");
}
