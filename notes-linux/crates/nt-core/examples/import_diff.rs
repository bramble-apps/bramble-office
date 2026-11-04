//! Why attachments of a Mac library are not in an app library after importing it (counts only).
use std::collections::{BTreeMap, HashSet};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (mac, _) = nt_core::import::read_library_with(std::path::Path::new(&a[0]), &Default::default()).unwrap();
    let app = nt_core::store::Store::open(std::path::Path::new(&a[1])).unwrap().library().unwrap();
    let mut why: BTreeMap<String, usize> = BTreeMap::new();
    for n in &mac.notes {
        let Some(m) = app.note(&n.id) else {
            *why.entry(format!("note not imported (locked {} not downloaded {})", n.locked, n.not_downloaded)).or_default() += n.attachments.len();
            continue;
        };
        let have: HashSet<&str> = m.attachments.iter().map(|x| x.id.as_str()).collect();
        let shown: HashSet<&str> = n.body.tokens().map(|t| t.id.as_str()).collect();
        for x in &n.attachments {
            if !have.contains(x.id.as_str()) {
                let dup = mac.notes.iter().filter(|o| o.attachments.iter().any(|y| y.id == x.id)).count();
                let ci = n.body.tokens().any(|t| t.id.eq_ignore_ascii_case(&x.id));
                let tok_utis: Vec<&str> = n.body.tokens().filter(|t| !t.is_inline()).map(|t| t.uti.as_str()).collect();
                let same_kind_tokens = tok_utis.iter().filter(|u| **u == x.uti).count();
                let same_kind_atts = n.attachments.iter().filter(|y| y.uti == x.uti).count();
                *why.entry(format!("{:?}: in body {} (ignoring case {}), body tokens of its type {same_kind_tokens}, note's attachments of its type {same_kind_atts}, in {dup} Mac notes, deleted note {}", x.kind, shown.contains(x.id.as_str()), ci, n.deleted)).or_default() += 1;
            }
        }
    }
    for (k, v) in why {
        println!("{v} {k}");
    }
}
