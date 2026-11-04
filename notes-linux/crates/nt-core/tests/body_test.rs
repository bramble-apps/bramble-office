//! Note body codec against every one-feature sample of the NO1 research.

mod common;

use std::path::Path;

use kn_format::{Msg, Val};
use nt_core::body;
use nt_core::model::{Align, ParaStyle};
use nt_core::pb;

/// Every minimal/<stem>.pb that has a decoded/<stem>.json.
fn samples(r: &Path) -> Vec<(String, Vec<u8>, serde_json::Value)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(r.join("format-analysis/decoded")).unwrap().flatten() {
        let p = e.path();
        let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
        let pbf = r.join("sample-files/minimal").join(format!("{stem}.pb"));
        if pbf.exists() {
            out.push((stem, std::fs::read(pbf).unwrap(), common::json(&p)));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(out.len() >= 37, "expected the 37 one-feature samples, found {}", out.len());
    out
}

/// A message with fields sorted by number (recursively for sub-messages), field 13 (run timestamp)
/// and paragraph uuids (2.9) dropped: what must survive a decode -> encode.
fn norm(m: &Msg, depth: usize) -> Msg {
    let mut fields: Vec<_> = m
        .fields
        .iter()
        .filter(|f| !(depth == 0 && f.num == 13) && !(depth == 1 && f.num == 9))
        .cloned()
        .map(|mut f| {
            if depth == 0 && (f.num == 2 || f.num == 3 || f.num == 10 || f.num == 12) {
                let sub = match &f.val {
                    Val::Bytes(b) => pb::parse(b).unwrap(),
                    Val::Msg(m) => m.clone(),
                    _ => unreachable!(),
                };
                f.val = Val::Bytes(norm(&sub, depth + 1).encode());
            }
            f
        })
        .collect();
    fields.sort_by_key(|f| f.num);
    let mut out = Msg::untyped();
    out.fields = fields;
    out
}

/// (text, normalized attributes) runs, adjacent equal runs merged.
fn runs(raw: &[u8]) -> Vec<(String, Vec<u8>)> {
    let root = pb::parse(&pb::gunzip(raw).unwrap()).unwrap();
    let note = pb::sub(&pb::sub(&root, 2).unwrap(), 3).unwrap();
    let units: Vec<u16> = pb::string(&note, 2).unwrap_or_default().encode_utf16().collect();
    let mut pos = 0;
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for r in pb::subs(&note, 5) {
        let n = pb::uint(&r, 1).unwrap() as usize;
        let text = String::from_utf16_lossy(&units[pos..pos + n]);
        pos += n;
        let mut a = norm(&r, 0);
        // the length, and an empty paragraph style (Notes writes one on some runs, same as none)
        a.fields.retain(|f| f.num != 1 && !(f.num == 2 && matches!(&f.val, Val::Bytes(b) if b.is_empty())));
        let a = a.encode();
        match out.last_mut() {
            Some((t, la)) if *la == a => t.push_str(&text),
            _ => out.push((text, a)),
        }
    }
    assert_eq!(pos, units.len(), "runs must cover the string");
    out
}

#[test]
fn decoded_text_matches_every_sample() {
    let r = research_or_skip!();
    for (stem, raw, j) in samples(&r) {
        let b = body::decode_note(&raw).unwrap();
        let want = j["document"]["document"]["note"]["string"].as_str().unwrap_or("");
        assert_eq!(b.text(), want, "{stem}");
        assert_eq!(body::note_text(&raw).unwrap(), want, "{stem}");
        // the decoder's runs agree with the research decoder's run list
        let jr: Vec<String> = j["runs"].as_array().unwrap().iter().map(|r| r["text"].as_str().unwrap().to_string()).collect();
        assert_eq!(jr.concat(), want, "{stem}");
    }
}

#[test]
fn round_trip_every_sample() {
    let r = research_or_skip!();
    for (stem, raw, _) in samples(&r) {
        let b = body::decode_note(&raw).unwrap();
        let enc = body::encode_note(&b);
        let b2 = body::decode_note(&enc).unwrap();
        assert_eq!(b, b2, "{stem}: decode(encode(body)) != body");
        assert_eq!(runs(&raw), runs(&enc), "{stem}: attribute runs changed");
        // unchanged paragraph uuids survive
        for (p, q) in b.paragraphs.iter().zip(&b2.paragraphs) {
            assert_eq!(p.id, q.id, "{stem}");
        }
    }
}

fn sample(r: &Path, stem: &str) -> nt_core::model::Body {
    body::decode_note(&std::fs::read(r.join(format!("sample-files/minimal/{stem}.pb"))).unwrap()).unwrap()
}

#[test]
fn paragraph_styles() {
    let r = research_or_skip!();
    let style = |stem: &str, i: usize| sample(&r, stem).paragraphs[i].style;
    assert_eq!(style("01_Plain", 0), ParaStyle::Title);
    assert_eq!(style("01_Plain", 1), ParaStyle::Body);
    assert_eq!(style("02_Heading", 1), ParaStyle::Heading);
    assert_eq!(style("03_Subheading", 1), ParaStyle::Subheading);
    assert_eq!(style("04_Monostyled", 1), ParaStyle::Monostyled);
    assert_eq!(style("05_Bulleted", 2), ParaStyle::Bulleted);
    assert_eq!(style("06_Dashed", 1), ParaStyle::Dashed);
    assert_eq!(style("07_Numbered", 2), ParaStyle::Numbered);
    let q = sample(&r, "08_Block_Quote");
    assert!(q.paragraphs[1].block_quote);
    assert_eq!(q.paragraphs[1].style, ParaStyle::Title, "Notes' quirk: block quote kept the Title style");
    let c = sample(&r, "09_Checklist");
    assert_eq!(c.paragraphs[1].style, ParaStyle::Checklist);
    assert!(!c.paragraphs[1].checked);
    assert!(c.paragraphs[2].checked);
    assert_eq!(c.paragraphs[1].check_id, "54E32E92-A60B-4B83-A26D-2E21F8253B28");
    assert_eq!(sample(&r, "16_Center").paragraphs[1].align, Align::Center);
    assert_eq!(sample(&r, "17_Indent").paragraphs[1].indent, 1);
    let n = sample(&r, "18_Nested_List");
    assert_eq!((n.paragraphs[2].style, n.paragraphs[2].indent), (ParaStyle::Bulleted, 1));
    let a = sample(&r, "27_Autoformat");
    assert_eq!(a.paragraphs[3].style, ParaStyle::Numbered);
    assert_eq!(a.paragraphs[3].start_number, Some(1));
    assert_eq!(a.paragraphs[1].style, ParaStyle::Dashed);
    assert_eq!(a.paragraphs[2].style, ParaStyle::Bulleted);
    assert_eq!(sample(&r, "23_Collapsed_Heading").paragraphs[1].id, "85DEFC66-441E-4E00-A7E6-ED5766D3C300");
}

#[test]
fn character_styles() {
    let r = research_or_skip!();
    let world = |stem: &str| {
        let b = sample(&r, stem);
        b.paragraphs[1].spans.iter().find(|s| s.text == "world").cloned().unwrap_or_else(|| panic!("{stem}: {:?}", b.paragraphs[1]))
    };
    assert!(world("10_Bold").bold);
    assert!(world("11_Italic").italic);
    assert!(world("12_Underline").underline);
    assert!(world("13_Strikethrough").strike);
    assert_eq!(world("14_Highlight").highlight, Some(3));
    assert_eq!(world("15_Superscript").script, 1);
    assert_eq!(world("24_Link").link.as_deref(), Some("https://example.com/bramble"));
    let h = sample(&r, "34_Highlight_Colors");
    let cols: Vec<_> = h.paragraphs[1..].iter().map(|p| p.spans.first().and_then(|s| s.highlight)).collect();
    assert_eq!(cols, [Some(1), Some(2), Some(3), Some(4), Some(5), None]);
    let e = sample(&r, "22_Emoji");
    assert_eq!(e.paragraphs[1].text(), "Hello 👋🏽 world 𝄞");
    let blank = sample(&r, "00_Blank");
    let f = blank.paragraphs[1].spans[0].font.clone().unwrap();
    assert_eq!(f.name.as_deref(), Some(".AppleSystemUIFontBold"));
    assert!(blank.paragraphs[1].spans[0].bold);
}

#[test]
fn tokens_and_attachments() {
    let r = research_or_skip!();
    let toks = |stem: &str| sample(&r, stem).tokens().map(|t| (t.id.clone(), t.uti.clone())).collect::<Vec<_>>();
    assert_eq!(toks("19_Table"), [("75BE093F-C841-46F3-BB9B-85994DFD5BE3".into(), "com.apple.notes.table".into())]);
    assert_eq!(toks("20_Divider")[0].1, "com.apple.notes.inlinetextattachment.dividerline");
    assert_eq!(toks("21_Tag")[0].1, "com.apple.notes.inlinetextattachment.hashtag");
    assert_eq!(toks("25_Note_Link")[0].1, "com.apple.notes.inlinetextattachment.link");
    assert_eq!(toks("26_Math").len(), 2);
    assert_eq!(toks("28_Image").len(), 2);
    assert_eq!(toks("29_PDF")[0].1, "com.adobe.pdf");
    assert_eq!(toks("30_File")[0].1, "public.plain-text");
    assert_eq!(toks("33_Drawing")[0].1, "com.apple.paper");
    // a token span holds exactly one U+FFFC
    for p in sample(&r, "28_Image").paragraphs {
        for s in p.spans {
            if s.token.is_some() {
                assert_eq!(s.text, "\u{FFFC}");
            }
        }
    }
}

#[test]
fn new_body_encodes_like_notes() {
    use nt_core::model::{Body, Paragraph, Span};
    let mut b = Body::from_lines("Shopping", &["milk", ""]);
    b.paragraphs.push(Paragraph { style: ParaStyle::Checklist, spans: vec![Span::plain("eggs")], checked: true, ..Default::default() });
    b.paragraphs.push(Paragraph { style: ParaStyle::Heading, spans: vec![Span { text: "Bold".into(), bold: true, ..Default::default() }], collapsed: true, ..Default::default() });
    body::assign_ids(&mut b);
    let enc = body::encode_note(&b);
    assert!(pb::is_gzip(&enc));
    let d = body::decode_note(&enc).unwrap();
    let mut want = b.clone();
    want.paragraphs.iter_mut().for_each(|p| p.collapsed = false);
    assert_eq!(d, want);
    assert_eq!(d.text(), "Shopping\nmilk\n\neggs\nBold");
    // collapse state round trip
    let os = body::encode_outline_state(&b).unwrap();
    let ids = body::decode_outline_state(&os);
    assert_eq!(ids, vec![b.paragraphs[4].id.clone()]);
    let (t, s) = body::title_and_snippet(&b);
    assert_eq!((t.as_str(), s.as_str()), ("Shopping", "milk"));
    // empty note
    let e = body::decode_note(&body::encode_note(&Body::default())).unwrap();
    assert_eq!(e.text(), "");
}

#[test]
fn collapsed_heading_state_from_store() {
    // the 23 Collapsed Heading note's ZOUTLINESTATEDATA bytes (from the research store)
    let raw = hex("091cd14d57374dc841122a3a285b2238354445464336362d343431452d344530302d413745362d454435373636443343333030225d");
    assert_eq!(body::decode_outline_state(&raw), vec!["85DEFC66-441E-4E00-A7E6-ED5766D3C300".to_string()]);
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}
