//! Table attachments (CRDT mergeable data) against the research's 19 Table sample.

mod common;

use nt_core::model::{Body, ParaStyle, Paragraph, Span, Table};
use nt_core::table;

#[test]
fn sample_table_grid() {
    let r = research_or_skip!();
    let blob = std::fs::read(r.join("sample-files/minimal/19_Table.table.pb.gz")).unwrap();
    let t = table::decode_table(&blob).unwrap();
    assert_eq!(t.text_grid(), vec![vec!["A1", "B1"], vec!["A2", ""]]);
    assert!(!t.rtl);
    // Notes' ICAttachment.summary for it
    assert_eq!(t.summary(), "A1\nB1\nA2\n\n");
    // and back
    let again = table::decode_table(&table::encode_table(&t)).unwrap();
    assert_eq!(again, t);
}

#[test]
fn new_tables_round_trip() {
    let mut t = Table::from_text(&[&["Name", "Qty", "Note"], &["Apples", "3", ""], &["Pears", "", "ripe\nsoft"]]);
    t.rows[1][0].paragraphs[0].spans[0].bold = true;
    t.rows[2][2].paragraphs[1].style = ParaStyle::Bulleted;
    let blob = table::encode_table(&t);
    let back = table::decode_table(&blob).unwrap();
    assert_eq!(back.text_grid(), t.text_grid());
    assert!(back.rows[1][0].paragraphs[0].spans[0].bold);
    assert_eq!(back.rows[2][2].paragraphs[1].style, ParaStyle::Bulleted);
    let rtl = Table { rtl: true, rows: vec![vec![Body { paragraphs: vec![Paragraph { spans: vec![Span::plain("א")], ..Default::default() }] }]] };
    assert!(table::decode_table(&table::encode_table(&rtl)).unwrap().rtl);
    // a 0x0 table still decodes
    assert_eq!(table::decode_table(&table::encode_table(&Table::default())).unwrap().rows.len(), 0);
}
