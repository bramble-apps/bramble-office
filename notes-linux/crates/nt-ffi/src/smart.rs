//! Smart Folders' rules: Notes' query JSON (ZSMARTFOLDERQUERYJSON) read, matched and written.
//!
//! Apple's form, as Notes 4.13 stores it (read from real libraries):
//! `{"type":{"and":[{"deleted":false},{"and"|"or":[<filter>, …]}]},"entity":"note"}` with
//! filters such as `{"pinned":true}`, `{"attachment":true}` and
//! `{"creationDateRelativeRange":{"type":2}}` (2 = the past week, 4 = the past 3 months). The
//! shell's own earlier form (`{"tags":[…],"pinned":true,"match":"all"}`) is read too.
//!
//! A rule here is (filter kind, argument): the Smart Folder sheet's rows.

use serde_json::{json, Value};

use nt_core::model::Note;

// (copied from nt-app/src/smart.rs, which the Android build can't depend on: keep the two the same)

/// The filter kinds a rule can be (the sheet's pop-up, Notes' SmartFolderFilterRow order).
pub const FILTERS: [&str; 9] = ["Tags", "Date Created", "Date Edited", "Checklists", "Attachments", "Folders", "Quick Notes", "Pinned Notes", "Locked Notes"];
pub const TAGS: u8 = 0;
pub const CREATED: u8 = 1;
pub const EDITED: u8 = 2;
pub const CHECKLISTS: u8 = 3;
pub const ATTACHMENTS: u8 = 4;
pub const FOLDERS: u8 = 5;
pub const QUICK_NOTES: u8 = 6;
pub const PINNED: u8 = 7;
pub const LOCKED: u8 = 8;

/// The relative date ranges (Notes' creationDateRelativeRange "type"; 2 and 4 seen in real
/// libraries) and their length in days.
pub const RANGES: [(&str, f64); 6] = [("Today", 1.0), ("Yesterday", 2.0), ("Past 7 Days", 7.0), ("Past 30 Days", 30.0), ("Past 3 Months", 91.0), ("Past Year", 365.0)];

/// A Smart Folder's rules: whether any (or all) must match, and the rules.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub any: bool,
    pub rules: Vec<(u8, String)>,
}

/// Reads a query (Apple's form or the shell's earlier one); an unreadable one has no rules.
pub fn parse(json: &str) -> Query {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return Query::default() };
    // the shell's earlier form
    if v.get("type").is_none() {
        let mut q = Query { any: v.get("match").and_then(Value::as_str) == Some("any"), rules: Vec::new() };
        if let Some(tags) = v.get("tags").and_then(Value::as_array) {
            for t in tags.iter().filter_map(Value::as_str) {
                q.rules.push((TAGS, t.to_string()));
            }
        }
        if v.get("pinned").and_then(Value::as_bool) == Some(true) {
            q.rules.push((PINNED, String::new()));
        }
        return q;
    }
    // Apple's: {"type":{"and":[{"deleted":false},{"and"|"or":[filters]}]}}
    let mut q = Query::default();
    let top = v.get("type").and_then(|t| t.get("and")).and_then(Value::as_array).cloned().unwrap_or_default();
    for part in top {
        for (op, any) in [("and", false), ("or", true)] {
            if let Some(list) = part.get(op).and_then(Value::as_array) {
                q.any = any;
                for f in list {
                    if let Some(r) = rule_of(f) {
                        q.rules.push(r);
                    }
                }
            }
        }
    }
    q
}

/// One of Apple's filters as a rule.
fn rule_of(f: &Value) -> Option<(u8, String)> {
    let o = f.as_object()?;
    let (k, v) = o.iter().next()?;
    Some(match k.as_str() {
        "pinned" => (PINNED, String::new()),
        "locked" => (LOCKED, String::new()),
        "attachment" => (ATTACHMENTS, String::new()),
        "checklist" => (CHECKLISTS, String::new()),
        "quickNote" => (QUICK_NOTES, String::new()),
        "creationDateRelativeRange" => (CREATED, v.get("type").and_then(Value::as_u64).unwrap_or(2).to_string()),
        "modificationDateRelativeRange" => (EDITED, v.get("type").and_then(Value::as_u64).unwrap_or(2).to_string()),
        "tag" => (TAGS, v.get("name").and_then(Value::as_str).unwrap_or("").to_string()),
        "folder" => (FOLDERS, v.get("identifier").and_then(Value::as_str).unwrap_or("").to_string()),
        _ => return None,
    })
}

/// Writes a query in Apple's form (the deleted notes left out, as Notes does).
pub fn to_json(q: &Query) -> String {
    let filters: Vec<Value> = q
        .rules
        .iter()
        .map(|(kind, arg)| match *kind {
            TAGS => json!({ "tag": { "name": arg.trim().trim_start_matches('#').to_lowercase() } }),
            CREATED => json!({ "creationDateRelativeRange": { "type": arg.parse::<u64>().unwrap_or(2) } }),
            EDITED => json!({ "modificationDateRelativeRange": { "type": arg.parse::<u64>().unwrap_or(2) } }),
            CHECKLISTS => json!({ "checklist": true }),
            ATTACHMENTS => json!({ "attachment": true }),
            FOLDERS => json!({ "folder": { "identifier": arg } }),
            QUICK_NOTES => json!({ "quickNote": true }),
            LOCKED => json!({ "locked": true }),
            _ => json!({ "pinned": true }),
        })
        .collect();
    let op = if q.any { "or" } else { "and" };
    json!({ "type": { "and": [ { "deleted": false }, { op: filters } ] }, "entity": "note" }).to_string()
}

/// What a rule needs to know about a note beyond the note itself.
pub struct Facts<'a> {
    pub tags: &'a [String],
    pub quick_note: bool,
    pub has_attachment: bool,
    pub has_checklist: bool,
}

/// Whether a note matches the query (a query with no rules matches nothing, as in Notes).
pub fn matches(q: &Query, n: &Note, f: &Facts, now: f64) -> bool {
    if n.deleted || q.rules.is_empty() {
        return false;
    }
    let one = |(kind, arg): &(u8, String)| match *kind {
        TAGS => {
            let want = arg.trim().trim_start_matches('#').to_lowercase();
            if want.is_empty() { !f.tags.is_empty() } else { f.tags.iter().any(|t| t.to_lowercase() == want) }
        }
        CREATED | EDITED => {
            let days = RANGES.get(arg.parse::<usize>().unwrap_or(2)).map(|r| r.1).unwrap_or(7.0);
            let t = if *kind == CREATED { n.created } else { n.modified };
            now - t <= days * 86400.0
        }
        CHECKLISTS => f.has_checklist,
        ATTACHMENTS => f.has_attachment,
        FOLDERS => n.folder == *arg,
        QUICK_NOTES => f.quick_note,
        LOCKED => n.locked,
        _ => n.pinned,
    };
    if q.any { q.rules.iter().any(one) } else { q.rules.iter().all(one) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_apples_queries() {
        let q = parse(r#"{"type":{"and":[{"deleted":false},{"and":[{"pinned":true}]}]},"entity":"note"}"#);
        assert_eq!(q, Query { any: false, rules: vec![(PINNED, String::new())] });
        let q = parse(r#"{"type":{"and":[{"deleted":false},{"or":[{"creationDateRelativeRange":{"type":2}}]}]},"entity":"note"}"#);
        assert_eq!(q, Query { any: true, rules: vec![(CREATED, "2".into())] });
        let q = parse(r#"{"entity":"note","type":{"and":[{"deleted":false},{"and":[{"attachment":true}]}]}}"#);
        assert_eq!(q.rules, vec![(ATTACHMENTS, String::new())]);
    }

    #[test]
    fn reads_the_shells_earlier_form() {
        let q = parse(r#"{"tags":["recipes"],"pinned":true,"match":"any"}"#);
        assert_eq!(q, Query { any: true, rules: vec![(TAGS, "recipes".into()), (PINNED, String::new())] });
    }

    #[test]
    fn writes_what_it_reads() {
        let q = Query { any: true, rules: vec![(TAGS, "work".into()), (EDITED, "3".into()), (LOCKED, String::new()), (FOLDERS, "F1".into())] };
        assert_eq!(parse(&to_json(&q)), q);
    }
}
