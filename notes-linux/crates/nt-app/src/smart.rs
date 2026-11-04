//! Smart Folders' rules: Notes' query JSON (ZSMARTFOLDERQUERYJSON) read, matched and written.
//!
//! Apple's form, as Notes 4.13 stores it (read from real libraries):
//! `{"type":{"and":[{"deleted":false},{"and"|"or":[<filter>, …]}]},"entity":"note"}` with
//! filters such as `{"pinned":true}`, `{"attachment":true}` and
//! `{"creationDateRelativeRange":{"type":2}}` (2 = the past week, 4 = the past 3 months). Filters
//! with no sample from a Mac (tag selections, counted ranges, dates, checklist states, attachment
//! kinds, folders, Quick Notes, locked) are written in the same structure with plain keys; the
//! shell's earlier forms are read too.
//!
//! The filters are the Mac's Smart Folder sheet's (FolderComposerViewController and its
//! strings): Tags (Notes with Tags, or included and excluded tags, all or any), Date Created and
//! Date Edited (Today, Yesterday, In the Past N Hours/Days/Weeks/Months/Years, On, Before or
//! After a Date), Checklists (any, all done, some undone), Attachments (any, or one kind),
//! Folders (include or exclude some), Quick Notes, Pinned Notes and Locked Notes (include or
//! exclude); Shared and Mentions are iCloud's and left out.

use serde_json::{json, Map, Value};

use nt_core::model::{AttachmentKind, Note};

/// The filters a rule can be, in the sheet's pop-up order.
pub const FILTERS: [&str; 9] = ["Tags", "Date Created", "Date Edited", "Checklists", "Attachments", "Folders", "Quick Notes", "Pinned Notes", "Locked Notes"];

/// One of the sheet's rows.
#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    /// Notes with any tag (`any_tag`), or with the included tags (all or any of them) and none
    /// (or not all) of the excluded ones
    Tags { any_tag: bool, included: Vec<String>, excluded: Vec<String>, all: bool },
    /// Date Created (`edited` false) or Date Edited
    Date { edited: bool, range: Range },
    Checklists(Checklist),
    /// any attachment (None), or one kind
    Attachments(Option<Kind>),
    /// notes in (or not in) these folders
    Folders { include: bool, folders: Vec<String> },
    QuickNotes(bool),
    Pinned(bool),
    Locked(bool),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Range {
    Today,
    Yesterday,
    /// in the past `n` units
    Past(u32, Unit),
    /// on, before or after a local day (dates::day_number)
    On(i64),
    Before(i64),
    After(i64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    Hours,
    Days,
    Weeks,
    Months,
    Years,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Checklist {
    /// notes with a checklist
    Any,
    /// every item checked
    Done,
    /// some item unchecked
    Undone,
}

/// Attachment kinds (the Mac's attachments browser's sections and the filter's choices).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Photos,
    Scans,
    Drawings,
    Documents,
    Audio,
    Links,
    Tables,
}

pub const UNITS: [(Unit, &str); 5] = [(Unit::Hours, "Hours"), (Unit::Days, "Days"), (Unit::Weeks, "Weeks"), (Unit::Months, "Months"), (Unit::Years, "Years")];
pub const KINDS: [(Kind, &str); 7] = [(Kind::Photos, "Photos & Videos"), (Kind::Scans, "Scanned Documents"), (Kind::Drawings, "Drawings"), (Kind::Documents, "Documents"), (Kind::Audio, "Audio"), (Kind::Links, "Websites"), (Kind::Tables, "Tables")];

impl Rule {
    /// The row's filter (an index into FILTERS).
    pub fn filter(&self) -> usize {
        match self {
            Rule::Tags { .. } => 0,
            Rule::Date { edited: false, .. } => 1,
            Rule::Date { edited: true, .. } => 2,
            Rule::Checklists(_) => 3,
            Rule::Attachments(_) => 4,
            Rule::Folders { .. } => 5,
            Rule::QuickNotes(_) => 6,
            Rule::Pinned(_) => 7,
            Rule::Locked(_) => 8,
        }
    }

    /// A new row of a filter, with the Mac's first choice.
    pub fn new(filter: usize) -> Rule {
        match filter {
            1 => Rule::Date { edited: false, range: Range::Past(7, Unit::Days) },
            2 => Rule::Date { edited: true, range: Range::Past(7, Unit::Days) },
            3 => Rule::Checklists(Checklist::Any),
            4 => Rule::Attachments(None),
            5 => Rule::Folders { include: true, folders: Vec::new() },
            6 => Rule::QuickNotes(true),
            7 => Rule::Pinned(true),
            8 => Rule::Locked(true),
            _ => Rule::Tags { any_tag: true, included: Vec::new(), excluded: Vec::new(), all: false },
        }
    }
}

impl Unit {
    fn key(self) -> &'static str {
        match self {
            Unit::Hours => "hour",
            Unit::Days => "day",
            Unit::Weeks => "week",
            Unit::Months => "month",
            Unit::Years => "year",
        }
    }

    fn from_key(k: &str) -> Unit {
        UNITS.iter().map(|u| u.0).find(|u| u.key() == k).unwrap_or(Unit::Days)
    }

    fn seconds(self) -> f64 {
        match self {
            Unit::Hours => 3600.0,
            Unit::Days => 86400.0,
            Unit::Weeks => 7.0 * 86400.0,
            Unit::Months => 30.44 * 86400.0,
            Unit::Years => 365.25 * 86400.0,
        }
    }
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Photos => "photo",
            Kind::Scans => "scan",
            Kind::Drawings => "drawing",
            Kind::Documents => "document",
            Kind::Audio => "audio",
            Kind::Links => "link",
            Kind::Tables => "table",
        }
    }

    fn from_key(k: &str) -> Option<Kind> {
        KINDS.iter().map(|k| k.0).find(|x| x.key() == k)
    }

    /// Whether an attachment of `a` is this kind.
    pub fn is(self, a: AttachmentKind) -> bool {
        match self {
            Kind::Photos => matches!(a, AttachmentKind::Image),
            Kind::Scans => matches!(a, AttachmentKind::Scan),
            Kind::Drawings => matches!(a, AttachmentKind::Drawing),
            Kind::Documents => matches!(a, AttachmentKind::Pdf | AttachmentKind::File),
            Kind::Audio => matches!(a, AttachmentKind::Audio),
            Kind::Links => matches!(a, AttachmentKind::Url),
            Kind::Tables => matches!(a, AttachmentKind::Table),
        }
    }
}

/// A Smart Folder's rules: whether any (or all) must match, and the rules.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub any: bool,
    pub rules: Vec<Rule>,
}

/// The Mac's relative range types (creationDateRelativeRange {"type": N}; 2 and 4 seen).
const LEGACY_RANGES: [Range; 6] = [Range::Today, Range::Yesterday, Range::Past(7, Unit::Days), Range::Past(30, Unit::Days), Range::Past(3, Unit::Months), Range::Past(1, Unit::Years)];

/// Reads a query (Apple's form or the shell's earlier ones); an unreadable one has no rules.
pub fn parse(json: &str) -> Query {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return Query::default() };
    // the shell's first form: {"tags":[…],"pinned":true,"match":"all"}
    if v.get("type").is_none() {
        let mut q = Query { any: v.get("match").and_then(Value::as_str) == Some("any"), rules: Vec::new() };
        if let Some(tags) = v.get("tags").and_then(Value::as_array) {
            for t in tags.iter().filter_map(Value::as_str) {
                q.rules.push(Rule::Tags { any_tag: t.is_empty(), included: vec![t.to_string()].into_iter().filter(|t| !t.is_empty()).collect(), excluded: Vec::new(), all: true });
            }
        }
        if v.get("pinned").and_then(Value::as_bool) == Some(true) {
            q.rules.push(Rule::Pinned(true));
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
                q.rules.extend(list.iter().filter_map(rule_of));
            }
        }
    }
    q
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

fn range_of(v: &Value) -> Range {
    if let Some(t) = v.get("type").and_then(Value::as_u64) {
        return LEGACY_RANGES.get(t as usize).copied().unwrap_or(Range::Past(7, Unit::Days));
    }
    let n = v.get("count").and_then(Value::as_u64).unwrap_or(7) as u32;
    Range::Past(n, Unit::from_key(v.get("unit").and_then(Value::as_str).unwrap_or("day")))
}

fn day_of(v: &Value) -> Option<Range> {
    let day = |k: &str| v.get(k).and_then(Value::as_str).and_then(crate::dates::parse_day);
    day("on").map(Range::On).or_else(|| day("before").map(Range::Before)).or_else(|| day("after").map(Range::After))
}

/// One filter as a rule.
fn rule_of(f: &Value) -> Option<Rule> {
    let (k, v) = f.as_object()?.iter().next()?;
    let on = || v.as_bool().unwrap_or(true);
    Some(match k.as_str() {
        "pinned" => Rule::Pinned(on()),
        "locked" => Rule::Locked(on()),
        "quickNote" => Rule::QuickNotes(on()),
        "attachment" => Rule::Attachments(None),
        "attachmentKind" => Rule::Attachments(v.as_str().and_then(Kind::from_key)),
        "checklist" => Rule::Checklists(match v.as_str() {
            Some("done") => Checklist::Done,
            Some("undone") => Checklist::Undone,
            _ => Checklist::Any,
        }),
        "creationDateRelativeRange" => Rule::Date { edited: false, range: range_of(v) },
        "modificationDateRelativeRange" => Rule::Date { edited: true, range: range_of(v) },
        "creationDate" => Rule::Date { edited: false, range: day_of(v)? },
        "modificationDate" => Rule::Date { edited: true, range: day_of(v)? },
        "tags" => Rule::Tags {
            any_tag: v.get("anyTag").and_then(Value::as_bool).unwrap_or(false),
            included: strings(v.get("included")),
            excluded: strings(v.get("excluded")),
            all: v.get("operator").and_then(Value::as_str) != Some("any"),
        },
        // (the shell's second form)
        "tag" => {
            let name = v.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            Rule::Tags { any_tag: name.is_empty(), included: if name.is_empty() { Vec::new() } else { vec![name] }, excluded: Vec::new(), all: true }
        }
        "folders" => Rule::Folders { include: v.get("include").and_then(Value::as_bool).unwrap_or(true), folders: strings(v.get("identifiers")) },
        "folder" => Rule::Folders { include: true, folders: v.get("identifier").and_then(Value::as_str).map(|s| vec![s.to_string()]).unwrap_or_default() },
        _ => return None,
    })
}

fn range_json(range: Range) -> (bool, Value) {
    // (relative: the Mac's own types where one fits)
    if let Some(t) = LEGACY_RANGES.iter().position(|r| *r == range) {
        return (true, json!({ "type": t }));
    }
    match range {
        Range::Past(n, u) => (true, json!({ "count": n, "unit": u.key() })),
        Range::On(d) => (false, json!({ "on": crate::dates::format_day(d) })),
        Range::Before(d) => (false, json!({ "before": crate::dates::format_day(d) })),
        Range::After(d) => (false, json!({ "after": crate::dates::format_day(d) })),
        _ => (true, json!({ "type": 2 })),
    }
}

/// Writes a query in Apple's form (the deleted notes left out, as Notes does).
pub fn to_json(q: &Query) -> String {
    let filters: Vec<Value> = q
        .rules
        .iter()
        .map(|r| {
            let mut m = Map::new();
            let (k, v): (&str, Value) = match r {
                Rule::Tags { any_tag, included, excluded, all } => (
                    "tags",
                    json!({ "anyTag": any_tag, "included": included.iter().map(|t| norm(t)).collect::<Vec<_>>(), "excluded": excluded.iter().map(|t| norm(t)).collect::<Vec<_>>(), "operator": if *all { "all" } else { "any" } }),
                ),
                Rule::Date { edited, range } => {
                    let (relative, v) = range_json(*range);
                    (match (edited, relative) {
                        (false, true) => "creationDateRelativeRange",
                        (true, true) => "modificationDateRelativeRange",
                        (false, false) => "creationDate",
                        (true, false) => "modificationDate",
                    }, v)
                }
                Rule::Checklists(c) => ("checklist", json!(match c {
                    Checklist::Any => "any",
                    Checklist::Done => "done",
                    Checklist::Undone => "undone",
                })),
                Rule::Attachments(None) => ("attachment", json!(true)),
                Rule::Attachments(Some(k)) => ("attachmentKind", json!(k.key())),
                Rule::Folders { include, folders } => ("folders", json!({ "include": include, "identifiers": folders })),
                Rule::QuickNotes(b) => ("quickNote", json!(b)),
                Rule::Pinned(b) => ("pinned", json!(b)),
                Rule::Locked(b) => ("locked", json!(b)),
            };
            m.insert(k.to_string(), v);
            Value::Object(m)
        })
        .collect();
    let op = if q.any { "or" } else { "and" };
    json!({ "type": { "and": [ { "deleted": false }, { op: filters } ] }, "entity": "note" }).to_string()
}

fn norm(t: &str) -> String {
    t.trim().trim_start_matches('#').to_lowercase()
}

/// The tag browser's selection (Place::Tag): "" is All Tags; otherwise tags joined by ',', an
/// excluded one marked '-', with "any|" in front when any of them is enough (the Mac's tag
/// browser: "Select all/any including … and excluding …").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagSelection {
    pub included: Vec<String>,
    pub excluded: Vec<String>,
    pub any: bool,
}

impl TagSelection {
    pub fn parse(spec: &str) -> TagSelection {
        let (any, rest) = match spec.strip_prefix("any|") {
            Some(r) => (true, r),
            None => (false, spec),
        };
        let mut s = TagSelection { any, ..Default::default() };
        for t in rest.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            match t.strip_prefix('-') {
                Some(x) => s.excluded.push(x.to_string()),
                None => s.included.push(t.to_string()),
            }
        }
        s
    }

    pub fn spec(&self) -> String {
        let tags: Vec<String> = self.included.iter().cloned().chain(self.excluded.iter().map(|t| format!("-{t}"))).collect();
        if tags.is_empty() {
            return String::new();
        }
        format!("{}{}", if self.any && tags.len() > 1 { "any|" } else { "" }, tags.join(","))
    }

    pub fn is_empty(&self) -> bool {
        self.included.is_empty() && self.excluded.is_empty()
    }

    /// The selection as a Smart Folder's tag rule (All Tags: notes with any tag).
    pub fn rule(&self) -> Rule {
        Rule::Tags { any_tag: self.is_empty(), included: self.included.clone(), excluded: self.excluded.clone(), all: !self.any }
    }

    /// The list's title: "#work", "#work, #home", "#work, not #old".
    pub fn title(&self) -> String {
        if self.is_empty() {
            return "All Tags".into();
        }
        let mut parts: Vec<String> = self.included.iter().map(|t| format!("#{t}")).collect();
        parts.extend(self.excluded.iter().map(|t| format!("not #{t}")));
        parts.join(if self.any { " or " } else { ", " })
    }

    /// Whether a note with these tags is in the selection.
    pub fn matches(&self, tags: &[String]) -> bool {
        let f = Facts { tags, quick_note: false, checklist: (0, 0), kinds: &[], has_attachment: false };
        let n = Note::default();
        matches(&Query { any: false, rules: vec![self.rule()] }, &n, &f, 0.0)
    }
}

/// What the rules need to know about a note beyond the note itself.
pub struct Facts<'a> {
    pub tags: &'a [String],
    pub quick_note: bool,
    /// checklist items: (all of them, checked)
    pub checklist: (u32, u32),
    /// the kinds of its attachments (empty: none known)
    pub kinds: &'a [AttachmentKind],
    /// has some attachment (when its kinds aren't known: a light note's picture)
    pub has_attachment: bool,
}

/// Whether a note matches the query (a query with no rules matches nothing, as in Notes).
pub fn matches(q: &Query, n: &Note, f: &Facts, now: f64) -> bool {
    if n.deleted || q.rules.is_empty() {
        return false;
    }
    let tagged = |t: &String| f.tags.iter().any(|x| norm(x) == norm(t));
    let one = |r: &Rule| match r {
        Rule::Tags { any_tag: true, .. } => !f.tags.is_empty(),
        Rule::Tags { included, excluded, all, .. } => {
            // (the Mac's: "any" = at least one included or not at least one excluded; "all" =
            // every included and none excluded)
            if *all {
                included.iter().all(tagged) && !excluded.iter().any(tagged)
            } else {
                included.iter().any(tagged) || (!excluded.is_empty() && !excluded.iter().all(tagged))
            }
        }
        Rule::Date { edited, range } => {
            let t = if *edited { n.modified } else { n.created };
            let (day, today) = (crate::dates::local(t).days, crate::dates::local(now).days);
            match *range {
                Range::Today => day == today,
                Range::Yesterday => day == today - 1,
                Range::Past(k, u) => now - t <= k as f64 * u.seconds(),
                Range::On(d) => day == d,
                Range::Before(d) => day < d,
                Range::After(d) => day > d,
            }
        }
        Rule::Checklists(c) => {
            let (all, done) = f.checklist;
            match c {
                Checklist::Any => all > 0,
                Checklist::Done => all > 0 && done == all,
                Checklist::Undone => done < all,
            }
        }
        Rule::Attachments(None) => f.has_attachment || !f.kinds.is_empty(),
        Rule::Attachments(Some(k)) => f.kinds.iter().any(|a| k.is(*a)),
        Rule::Folders { include, folders } => folders.iter().any(|x| *x == n.folder) == *include,
        Rule::QuickNotes(b) => f.quick_note == *b,
        Rule::Pinned(b) => n.pinned == *b,
        Rule::Locked(b) => n.locked == *b,
    };
    if q.any { q.rules.iter().any(one) } else { q.rules.iter().all(one) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_apples_queries() {
        let q = parse(r#"{"type":{"and":[{"deleted":false},{"and":[{"pinned":true}]}]},"entity":"note"}"#);
        assert_eq!(q, Query { any: false, rules: vec![Rule::Pinned(true)] });
        let q = parse(r#"{"type":{"and":[{"deleted":false},{"or":[{"creationDateRelativeRange":{"type":2}}]}]},"entity":"note"}"#);
        assert_eq!(q, Query { any: true, rules: vec![Rule::Date { edited: false, range: Range::Past(7, Unit::Days) }] });
        let q = parse(r#"{"entity":"note","type":{"and":[{"deleted":false},{"and":[{"creationDateRelativeRange":{"type":4}}]}]}}"#);
        assert_eq!(q.rules, vec![Rule::Date { edited: false, range: Range::Past(3, Unit::Months) }]);
        let q = parse(r#"{"entity":"note","type":{"and":[{"deleted":false},{"and":[{"attachment":true}]}]}}"#);
        assert_eq!(q.rules, vec![Rule::Attachments(None)]);
    }

    #[test]
    fn reads_the_shells_earlier_forms() {
        let q = parse(r#"{"tags":["recipes"],"pinned":true,"match":"any"}"#);
        assert_eq!(q.rules, vec![Rule::Tags { any_tag: false, included: vec!["recipes".into()], excluded: vec![], all: true }, Rule::Pinned(true)]);
        let q = parse(r#"{"entity":"note","type":{"and":[{"deleted":false},{"and":[{"tag":{"name":""}}]}]}}"#);
        assert_eq!(q.rules, vec![Rule::Tags { any_tag: true, included: vec![], excluded: vec![], all: true }]);
    }

    #[test]
    fn writes_what_it_reads() {
        let q = Query {
            any: true,
            rules: vec![
                Rule::Tags { any_tag: false, included: vec!["work".into()], excluded: vec!["old".into()], all: false },
                Rule::Date { edited: true, range: Range::Past(3, Unit::Weeks) },
                Rule::Date { edited: false, range: Range::Past(7, Unit::Days) },
                Rule::Date { edited: false, range: Range::Before(crate::dates::day_number(2024, 1, 1)) },
                Rule::Checklists(Checklist::Undone),
                Rule::Attachments(Some(Kind::Scans)),
                Rule::Folders { include: false, folders: vec!["F1".into()] },
                Rule::QuickNotes(false),
                Rule::Locked(true),
            ],
        };
        assert_eq!(parse(&to_json(&q)), q);
        // (the Mac's own relative type where one fits)
        assert!(to_json(&Query { any: false, rules: vec![Rule::Date { edited: false, range: Range::Past(7, Unit::Days) }] }).contains(r#""creationDateRelativeRange":{"type":2}"#));
    }

    #[test]
    fn tag_selections() {
        let s = TagSelection::parse("any|work,home,-old");
        assert_eq!(s, TagSelection { included: vec!["work".into(), "home".into()], excluded: vec!["old".into()], any: true });
        assert_eq!(TagSelection::parse(&s.spec()), s);
        let tags = |v: &[&str]| v.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        let all = TagSelection::parse("work,-old");
        assert!(all.matches(&tags(&["work"])));
        assert!(!all.matches(&tags(&["work", "old"])));
        assert!(TagSelection::parse("").matches(&tags(&["x"])));
        assert!(!TagSelection::parse("").matches(&[]));
        assert_eq!(TagSelection::parse("work,-old").title(), "#work, not #old");
    }

    #[test]
    fn matches_like_the_mac() {
        let now = 1_793_717_760.0;
        let mut n = Note { created: now - 3.0 * 86400.0, modified: now - 3600.0, pinned: true, folder: "F1".into(), ..Default::default() };
        let tags = vec!["work".to_string()];
        let f = Facts { tags: &tags, quick_note: false, checklist: (3, 1), kinds: &[AttachmentKind::Scan], has_attachment: true };
        let m = |rules: Vec<Rule>, any: bool, n: &Note| matches(&Query { any, rules }, n, &f, now);
        assert!(m(vec![Rule::Date { edited: false, range: Range::Past(7, Unit::Days) }], false, &n));
        assert!(!m(vec![Rule::Date { edited: false, range: Range::Today }], false, &n));
        assert!(m(vec![Rule::Date { edited: true, range: Range::Today }], false, &n) || crate::dates::local(now - 3600.0).days != crate::dates::local(now).days);
        assert!(m(vec![Rule::Checklists(Checklist::Undone)], false, &n));
        assert!(!m(vec![Rule::Checklists(Checklist::Done)], false, &n));
        assert!(m(vec![Rule::Attachments(Some(Kind::Scans))], false, &n));
        assert!(!m(vec![Rule::Attachments(Some(Kind::Audio))], false, &n));
        assert!(!m(vec![Rule::Folders { include: false, folders: vec!["F1".into()] }], false, &n));
        assert!(m(vec![Rule::Tags { any_tag: false, included: vec!["work".into()], excluded: vec![], all: true }, Rule::Pinned(true)], false, &n));
        assert!(m(vec![Rule::Locked(true), Rule::Pinned(true)], true, &n));
        assert!(!m(vec![Rule::Locked(true), Rule::Pinned(true)], false, &n));
        n.deleted = true;
        assert!(!m(vec![Rule::Pinned(true)], false, &n));
    }
}
