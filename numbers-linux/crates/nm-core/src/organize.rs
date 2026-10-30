//! Organize > Filter: Numbers' filter rules (research features/filter-and-highlight-rules.txt)
//! and which body rows they leave shown. Rules are on one column each; a table's rules must all
//! hold ("Match all filters") or any of them ("Match any filter"). Rows a filter hides are
//! written as `filtered` hidden states, as Numbers keeps them (research 31-filter).

use kn_model::table::CellValue;

/// One kind of rule, as the rule chooser lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    // Number (dates and durations compare by their value too)
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    Positive,
    Negative,
    Between,
    NotBetween,
    AboveAverage,
    BelowAverage,
    Top,
    Bottom,
    // Text
    Is,
    IsNot,
    StartsWith,
    NotStartsWith,
    EndsWith,
    NotEndsWith,
    Contains,
    NotContains,
    // Date
    Yesterday,
    Today,
    Tomorrow,
    InLastDays,
    InNextDays,
    DateIs,
    Before,
    After,
    DateBetween,
    // Cell
    Blank,
    NotBlank,
    Unique,
    Duplicate,
    True,
    False,
    /// Quick Filter: only the ticked values (`values`).
    Quick,
}

impl Rule {
    /// The chooser's groups: (group, rules with their labels).
    pub const GROUPS: &'static [(&'static str, &'static [(Rule, &'static str)])] = &[
        ("Quick Filter", &[(Rule::Quick, "Choose values…")]),
        (
            "Number",
            &[
                (Rule::Equal, "Equal to"),
                (Rule::NotEqual, "Not equal to"),
                (Rule::Greater, "Greater than"),
                (Rule::GreaterOrEqual, "Greater than or equal to"),
                (Rule::Less, "Less than"),
                (Rule::LessOrEqual, "Less than or equal to"),
                (Rule::Positive, "Is positive"),
                (Rule::Negative, "Is negative"),
                (Rule::Between, "Between"),
                (Rule::NotBetween, "Not between"),
                (Rule::AboveAverage, "Above average"),
                (Rule::BelowAverage, "Below average"),
                (Rule::Top, "In the top"),
                (Rule::Bottom, "In the bottom"),
            ],
        ),
        (
            "Text",
            &[
                (Rule::Is, "Is"),
                (Rule::IsNot, "Is not"),
                (Rule::StartsWith, "Starts with"),
                (Rule::NotStartsWith, "Does not start with"),
                (Rule::EndsWith, "Ends with"),
                (Rule::NotEndsWith, "Does not end with"),
                (Rule::Contains, "Contains"),
                (Rule::NotContains, "Does not contain"),
            ],
        ),
        (
            "Date",
            &[
                (Rule::Yesterday, "Yesterday"),
                (Rule::Today, "Today"),
                (Rule::Tomorrow, "Tomorrow"),
                (Rule::InLastDays, "In the last (days)"),
                (Rule::InNextDays, "In the next (days)"),
                (Rule::DateIs, "The date"),
                (Rule::Before, "Before the date"),
                (Rule::After, "After the date"),
                (Rule::DateBetween, "In the range"),
            ],
        ),
        (
            "Cell",
            &[
                (Rule::Blank, "Is blank"),
                (Rule::NotBlank, "Is not blank"),
                (Rule::Unique, "Is unique"),
                (Rule::Duplicate, "Is a duplicate"),
                (Rule::True, "Is TRUE / checked"),
                (Rule::False, "Is FALSE / not checked"),
            ],
        ),
    ];

    pub fn label(self) -> &'static str {
        Rule::GROUPS.iter().flat_map(|g| g.1.iter()).find(|r| r.0 == self).map(|r| r.1).unwrap_or("")
    }

    /// How many values the rule takes (0, 1 or 2).
    pub fn operands(self) -> usize {
        use Rule::*;
        match self {
            Between | NotBetween | DateBetween => 2,
            Positive | Negative | AboveAverage | BelowAverage | Yesterday | Today | Tomorrow | Blank | NotBlank | Unique | Duplicate | True | False | Quick => 0,
            _ => 1,
        }
    }
}

/// A rule on a column; `a` and `b` are the values typed in, `values` the Quick Filter's ticks.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterRule {
    pub col: u32,
    pub rule: Rule,
    pub a: String,
    pub b: String,
    pub values: Vec<String>,
}

impl FilterRule {
    pub fn new(col: u32, rule: Rule) -> FilterRule {
        FilterRule { col, rule, a: String::new(), b: String::new(), values: Vec::new() }
    }
}

/// A table's filters: on / off, all or any, the rules.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Filters {
    pub on: bool,
    pub any: bool,
    pub rules: Vec<FilterRule>,
}

/// The cell as filtering sees it: its value and the text shown.
#[derive(Clone, Debug)]
pub struct Cell<'a> {
    pub value: Option<&'a CellValue>,
    pub shown: &'a str,
}

fn num(v: Option<&CellValue>) -> Option<f64> {
    match v {
        Some(CellValue::Number(x) | CellValue::Duration(x) | CellValue::Date(x)) => Some(*x),
        Some(CellValue::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn parse_num(s: &str) -> Option<f64> {
    s.trim().trim_end_matches('%').replace([',', '$'], "").parse().ok()
}

/// The rows of `rows` that every rule (or any rule, `any`) leaves shown. `cell(row, col)`
/// gives each cell; `today` is midnight today in seconds since 2001 (date rules).
pub fn shown_rows<'a>(filters: &Filters, rows: &[u32], cell: &dyn Fn(u32, u32) -> Cell<'a>, today: f64) -> Vec<u32> {
    if !filters.on || filters.rules.is_empty() {
        return rows.to_vec();
    }
    let day = 86_400.0;
    // per rule, the column's facts some rules need (average, top / bottom cut, counts)
    let prepared: Vec<(Option<f64>, Option<f64>, std::collections::HashMap<String, usize>)> = filters
        .rules
        .iter()
        .map(|r| {
            let nums: Vec<f64> = rows.iter().filter_map(|&row| num(cell(row, r.col).value)).collect();
            let avg = (!nums.is_empty()).then(|| nums.iter().sum::<f64>() / nums.len() as f64);
            let cut = match r.rule {
                Rule::Top | Rule::Bottom => {
                    let mut v = nums.clone();
                    v.sort_by(|a, b| a.total_cmp(b));
                    if r.rule == Rule::Top {
                        v.reverse();
                    }
                    let n = if r.a.trim().ends_with('%') { (parse_num(&r.a).unwrap_or(10.0) / 100.0 * v.len() as f64).ceil() as usize } else { parse_num(&r.a).unwrap_or(10.0) as usize };
                    n.checked_sub(1).and_then(|i| v.get(i.min(v.len().saturating_sub(1)))).copied()
                }
                _ => None,
            };
            let mut counts = std::collections::HashMap::new();
            if matches!(r.rule, Rule::Unique | Rule::Duplicate) {
                for &row in rows {
                    *counts.entry(cell(row, r.col).shown.to_lowercase()).or_insert(0) += 1;
                }
            }
            (avg, cut, counts)
        })
        .collect();
    rows.iter()
        .copied()
        .filter(|&row| {
            let mut results = filters.rules.iter().zip(&prepared).map(|(r, (avg, cut, counts))| {
                let c = cell(row, r.col);
                let x = num(c.value);
                let text = c.shown.trim();
                let t = text.to_lowercase();
                let (a, b) = (parse_num(&r.a), parse_num(&r.b));
                let la = r.a.trim().to_lowercase();
                let date = |s: &str| crate::typed::parse_date(s, today).map(|d| (d.0 / day).floor());
                let dx = match c.value {
                    Some(CellValue::Date(v)) => Some((v / day).floor()),
                    _ => None,
                };
                let td = today / day;
                use Rule::*;
                let ok = match r.rule {
                    Equal => x.zip(a).is_some_and(|(x, a)| x == a),
                    NotEqual => !x.zip(a).is_some_and(|(x, a)| x == a),
                    Greater => x.zip(a).is_some_and(|(x, a)| x > a),
                    GreaterOrEqual => x.zip(a).is_some_and(|(x, a)| x >= a),
                    Less => x.zip(a).is_some_and(|(x, a)| x < a),
                    LessOrEqual => x.zip(a).is_some_and(|(x, a)| x <= a),
                    Positive => x.is_some_and(|x| x > 0.0),
                    Negative => x.is_some_and(|x| x < 0.0),
                    Between => x.zip(a.zip(b)).is_some_and(|(x, (a, b))| x >= a.min(b) && x <= a.max(b)),
                    NotBetween => !x.zip(a.zip(b)).is_some_and(|(x, (a, b))| x >= a.min(b) && x <= a.max(b)),
                    AboveAverage => x.zip(*avg).is_some_and(|(x, m)| x > m),
                    BelowAverage => x.zip(*avg).is_some_and(|(x, m)| x < m),
                    Top => x.zip(*cut).is_some_and(|(x, k)| x >= k),
                    Bottom => x.zip(*cut).is_some_and(|(x, k)| x <= k),
                    Is => t == la,
                    IsNot => t != la,
                    StartsWith => t.starts_with(&la),
                    NotStartsWith => !t.starts_with(&la),
                    EndsWith => t.ends_with(&la),
                    NotEndsWith => !t.ends_with(&la),
                    Contains => t.contains(&la),
                    NotContains => !t.contains(&la),
                    Yesterday => dx == Some(td - 1.0),
                    Today => dx == Some(td),
                    Tomorrow => dx == Some(td + 1.0),
                    InLastDays => dx.zip(a).is_some_and(|(d, n)| d <= td && d > td - n),
                    InNextDays => dx.zip(a).is_some_and(|(d, n)| d >= td && d < td + n),
                    DateIs => dx.zip(date(&r.a)).is_some_and(|(d, w)| d == w),
                    Before => dx.zip(date(&r.a)).is_some_and(|(d, w)| d < w),
                    After => dx.zip(date(&r.a)).is_some_and(|(d, w)| d > w),
                    DateBetween => dx.zip(date(&r.a).zip(date(&r.b))).is_some_and(|(d, (p, q))| d >= p.min(q) && d <= p.max(q)),
                    Blank => text.is_empty(),
                    NotBlank => !text.is_empty(),
                    Unique => counts.get(&t).copied().unwrap_or(0) == 1,
                    Duplicate => counts.get(&t).copied().unwrap_or(0) > 1,
                    True => matches!(c.value, Some(CellValue::Bool(true))) || t == "true",
                    False => matches!(c.value, Some(CellValue::Bool(false))) || t == "false",
                    Quick => r.values.iter().any(|v| v.to_lowercase() == t),
                };
                // a rule with nothing typed in yet hides nothing
                ok || (r.rule.operands() > 0 && la.is_empty() && !matches!(r.rule, Is | IsNot))
            });
            if filters.any { results.any(|ok| ok) } else { results.all(|ok| ok) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_all_and_any() {
        let vals = [CellValue::Number(3.0), CellValue::Number(8.0), CellValue::Number(25.0), CellValue::Text("apple".into())];
        let shown = ["3", "8", "25", "apple"];
        let cell = |r: u32, _c: u32| Cell { value: Some(&vals[r as usize]), shown: shown[r as usize] };
        let rows = [0, 1, 2, 3];
        let f = |rules: Vec<FilterRule>, any: bool| shown_rows(&Filters { on: true, any, rules }, &rows, &cell, 0.0);
        let mut gt = FilterRule::new(0, Rule::Greater);
        gt.a = "5".into();
        assert_eq!(f(vec![gt.clone()], false), [1, 2]);
        let mut lt = FilterRule::new(0, Rule::Less);
        lt.a = "10".into();
        assert_eq!(f(vec![gt.clone(), lt.clone()], false), [1]);
        let mut c = FilterRule::new(0, Rule::Contains);
        c.a = "PP".into();
        assert_eq!(f(vec![lt, c], true), [0, 1, 3]);
        assert_eq!(f(vec![FilterRule::new(0, Rule::AboveAverage)], false), [2]);
        let mut top = FilterRule::new(0, Rule::Top);
        top.a = "2".into();
        assert_eq!(f(vec![top], false), [1, 2]);
        let mut q = FilterRule::new(0, Rule::Quick);
        q.values = vec!["3".into(), "apple".into()];
        assert_eq!(f(vec![q], false), [0, 3]);
        // nothing typed yet: the rule hides nothing
        assert_eq!(f(vec![FilterRule::new(0, Rule::Greater)], false), rows);
        // off: everything shows
        assert_eq!(shown_rows(&Filters { on: false, any: false, rules: vec![gt] }, &rows, &cell, 0.0), rows);
    }
}

/// A table's saved sort rules (TableModel.sort_order: column indices, ascending or not) and
/// whether they sort only a row range (Mac N29-I1).
pub fn sort_rules(k: &kn_format::KeyFile, info: kn_format::ObjId) -> Option<(Vec<(u32, bool)>, bool)> {
    let model = k.obj(info)?.ref_id("tableModel").and_then(|t| k.obj(t))?;
    let so = model.msg("sort_order")?;
    let rules: Vec<(u32, bool)> = so.msgs("rules").filter_map(|r| Some((r.u32("index")?, r.enum_name("direction") != Some("descending")))).collect();
    (!rules.is_empty()).then(|| (rules, so.enum_name("type") == Some("row_range")))
}

/// Save a table's sort rules (none: the table has no sort order).
pub fn set_sort_rules(doc: &mut kn_model::Document, h: &mut kn_model::edit::History, info: kn_format::ObjId, rules: &[(u32, bool)], row_range: bool) -> bool {
    let Some(model) = doc.file.obj(info).and_then(|m| m.ref_id("tableModel")) else { return false };
    let current = sort_rules(&doc.file, info);
    if current.as_ref().map(|c| (c.0.as_slice(), c.1)) == Some((rules, row_range)) || (current.is_none() && rules.is_empty()) {
        return false;
    }
    doc.touch(h, model);
    let Some(m) = doc.file.obj_mut(model) else { return false };
    m.remove("sort_order");
    if !rules.is_empty() {
        let mut so = kn_format::Msg::new("TST.TableSortOrderArchive");
        so.set_enum("type", if row_range { "row_range" } else { "entire_table" });
        for &(c, asc) in rules {
            let mut r = kn_format::Msg::new("TST.TableSortOrderArchive.SortRuleArchive");
            r.set_u64("index", c as u64);
            r.set_enum("direction", if asc { "ascending" } else { "descending" });
            so.push_msg("rules", r);
        }
        m.push_msg("sort_order", so);
    }
    true
}

#[cfg(test)]
mod sort_rule_tests {
    use super::*;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn mac_sort_rules_read_and_round_trip() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/research/out-N29/files/N29-I1-organize.numbers");
        if !p.exists() {
            return;
        }
        let mut wb = crate::Workbook::open(&p).unwrap();
        let t = wb.file().objects_of("TST.TableInfoArchive").next().unwrap().0;
        // Team ascending, then Score descending
        assert_eq!(sort_rules(wb.file(), t), Some((vec![(1, true), (2, false)], false)));
        let mut h = kn_model::edit::History::default();
        wb.doc.begin(&mut h, "Sort");
        assert!(set_sort_rules(&mut wb.doc, &mut h, t, &[(0, false)], true));
        wb.doc.end(&mut h);
        let d2 = kn_model::Document::from_file(kn_format::KeyFile::from_zip(&wb.doc.file.to_zip().unwrap()).unwrap());
        assert_eq!(sort_rules(&d2.file, t), Some((vec![(0, false)], true)));
    }
}

