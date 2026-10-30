//! The Organize inspector, as Numbers 15.4 lays it out (research screenshots N-organize-1/2/3):
//! Categories | Sort | Filter.
//!
//! - Sort: Sort Now, Sort Entire Table / Sort Selected Rows, one rule per column (ascending or
//!   descending; the first decides, the next breaks ties), Add a Column…
//! - Filter: the Filters switch, a rule per column (Numbers' rule chooser: number, text, date,
//!   cell, Quick Filter), Match all / any; rows a filter hides are the table's `filtered`
//!   hidden states, set again after every edit.
//! - Categories: the Categories switch, a column to group the rows by, ascending or descending
//!   groups, Hide Category Column, each group collapsible.

use std::collections::HashSet;

use egui::{Color32, RichText};
use kn_ui::theme;
use nm_core::kn_format::ObjId;
use nm_core::kn_model::edit::History;
use nm_core::kn_model::{Document, TableData};
use nm_core::organize::{Cell, FilterRule, Filters, Rule};

use crate::app::App;
use crate::canvas::commit;

/// A table's categories (made in Values): up to five levels, each a column and its group
/// order; groups collapsed by their path ("Office › Paris").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cats {
    pub on: bool,
    /// (column, descending) per level, outermost first.
    pub levels: Vec<(u32, bool)>,
    pub hide_col: bool,
    pub collapsed: HashSet<String>,
}

/// A group row of a categorized view: its path key and level.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupRow {
    pub key: String,
    pub level: usize,
    /// The model rows in the group (all levels under it).
    pub rows: Vec<u32>,
}

/// The value a row is grouped by: the cell's text ("(blank)" when empty).
fn group_label(t: &TableData, r: u32, c: u32) -> String {
    let s = t.cells.get((r * t.cols + c) as usize).map(|s| s.trim().to_string()).unwrap_or_default();
    if s.is_empty() { "(blank)".into() } else { s }
}

/// Groups in order of their value: numbers by size, text alphabetically, blank last.
fn label_order(a: &str, b: &str, descending: bool) -> std::cmp::Ordering {
    let key = |l: &str| (l == "(blank)", l.replace([',', '$', '%'], "").parse::<f64>().ok(), l.to_lowercase());
    let (ka, kb) = (key(a), key(b));
    let o = match (ka.1, kb.1) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        _ => ka.2.cmp(&kb.2),
    };
    ka.0.cmp(&kb.0).then(if descending { o.reverse() } else { o })
}

/// Show `t` grouped as `c` says; the view maps its rows back to the model's, and each group
/// row (by view row) gets its path and level.
pub fn categorize(t: &mut TableData, c: &Cats) -> (nm_core::categories::View, std::collections::HashMap<usize, GroupRow>) {
    let levels: Vec<(u32, bool)> = c.levels.iter().copied().filter(|(col, _)| *col < t.cols).collect();
    if levels.is_empty() {
        return Default::default();
    }
    let body: Vec<u32> = (t.header_rows..t.rows.saturating_sub(t.footer_rows)).collect();
    let mut groups: Vec<nm_core::categories::Group> = Vec::new();
    let mut infos: Vec<GroupRow> = Vec::new();
    fn build(t: &TableData, rows: &[u32], levels: &[(u32, bool)], depth: usize, path: &str, c: &Cats, groups: &mut Vec<nm_core::categories::Group>, infos: &mut Vec<GroupRow>) {
        let (col, desc) = levels[0];
        let mut split: Vec<(String, Vec<u32>)> = Vec::new();
        for &r in rows {
            let l = group_label(t, r, col);
            match split.iter_mut().find(|g| g.0 == l) {
                Some(g) => g.1.push(r),
                None => split.push((l, vec![r])),
            }
        }
        split.sort_by(|a, b| label_order(&a.0, &b.0, desc));
        for (label, rs) in split {
            let key = if path.is_empty() { label.clone() } else { format!("{path} › {label}") };
            let closed = c.collapsed.contains(&key);
            let last = levels.len() == 1;
            groups.push(nm_core::categories::Group {
                label: format!("{}{} {label}  ({})", "      ".repeat(depth), if closed { "▶" } else { "▼" }, rs.len()),
                rows: if last && !closed { rs.clone() } else { Vec::new() },
            });
            infos.push(GroupRow { key: key.clone(), level: depth, rows: rs.clone() });
            if !last && !closed {
                build(t, &rs, &levels[1..], depth + 1, &key, c, groups, infos);
            }
        }
    }
    build(t, &body, &levels, 0, "", c, &mut groups, &mut infos);
    let view = nm_core::categories::apply(t, &groups);
    let mut map = std::collections::HashMap::new();
    let mut it = infos.into_iter();
    for (i, r) in view.rows.iter().enumerate() {
        if r.is_none() {
            if let Some(g) = it.next() {
                map.insert(i, g);
            }
        }
    }
    if c.hide_col {
        for (col, _) in &levels {
            if let Some(w) = t.col_widths.get_mut(*col as usize) {
                *w = 0.0;
            }
        }
    }
    (view, map)
}

/// Set the rows a filter hides to what `fl` leaves out now (nothing when it's off).
pub fn apply_filters(doc: &mut Document, h: &mut History, t: ObjId, fl: &Filters) {
    let Some(td) = doc.file.obj(t).and_then(|m| nm_core::kn_model::table::table(&doc.file, m)) else { return };
    let vals = doc.cell_values(t);
    let rows: Vec<u32> = (td.header_rows..td.rows.saturating_sub(td.footer_rows)).collect();
    let cell = |r: u32, c: u32| Cell { value: vals.get(&(r, c)), shown: td.cells.get((r * td.cols + c) as usize).map(String::as_str).unwrap_or("") };
    let shown = nm_core::organize::shown_rows(fl, &rows, &cell, nm_core::typed::today());
    let hide: Vec<u32> = rows.into_iter().filter(|r| !shown.contains(r)).collect();
    if nm_core::filtered(&doc.file, t) != hide {
        let _ = nm_core::set_filtered(doc, h, t, &hide);
    }
}

/// A column's name: its header cell's text, else "Column B".
pub(crate) fn col_label(t: &TableData, c: u32) -> String {
    let head = (t.header_rows > 0).then(|| t.cells.get(c as usize).map(|s| s.trim().replace('\n', " "))).flatten().filter(|s| !s.is_empty());
    head.unwrap_or_else(|| format!("Column {}", crate::canvas::col_name(c)))
}

fn note(ui: &mut egui::Ui, s: &str) {
    ui.add_space(8.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(s).color(theme::TEXT_DIM()));
    });
    ui.add_space(6.0);
}

/// A full-width pop-up listing the columns; the picked one.
fn column_menu(ui: &mut egui::Ui, id: &str, label: &str, t: &TableData) -> Option<u32> {
    let mut pick = None;
    ui.vertical_centered(|ui| {
        theme::combo(id).selected_text(label).width((ui.available_width() - 40.0).max(120.0)).height(360.0).show_ui(ui, |ui| {
            for c in 0..t.cols {
                let r = ui.selectable_label(false, col_label(t, c));
                if r.clicked() {
                    pick = Some(c);
                }
            }
        });
    });
    pick
}

/// The section title with its switch at the right (Filters, Categories).
fn title_switch(ui: &mut egui::Ui, title: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(theme::bold(title).size(14.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = theme::toggle(ui, on).changed();
        });
    });
    ui.separator();
    changed
}

fn remove_button(ui: &mut egui::Ui) -> bool {
    ui.add(egui::Button::new(RichText::new("✕").size(12.0)).frame(false)).on_hover_text("Remove").clicked()
}

pub fn ui(app: &mut App, ui: &mut egui::Ui, t: ObjId) {
    // a pivot table: its Pivot Options (Numbers shows those instead)
    if app.pivots.contains_key(&t) {
        let mut one = 0;
        crate::objinsp::tabs(ui, &["Pivot Options"], &mut one);
        crate::pivotui::options(app, ui, t);
        return;
    }
    let key = egui::Id::unique("organize-tab");
    // Numbers' order: Categories | Sort | Filter (Sort first shown)
    let mut tab: usize = ui.data(|d| d.get_temp(key)).unwrap_or(1);
    // the inspector's top tabs: the capsule, as Format's (and Letters') are
    crate::objinsp::tabs(ui, &["Categories", "Sort", "Filter"], &mut tab);
    ui.data_mut(|d| d.insert_temp(key, tab));
    // the table as stored (not as categories show it)
    let Some(td) = app.wb.as_ref().and_then(|w| w.table_data(t)) else { return };
    match tab {
        0 => categories(app, ui, t, &td),
        1 => sort(app, ui, t, &td),
        _ => filter(app, ui, t, &td),
    }
}

fn sort(app: &mut App, ui: &mut egui::Ui, t: ObjId, td: &TableData) {
    let categorized = app.categorized(t);
    let (mut rules, mut selected) = app.sorts.get(&t).cloned().unwrap_or_default();
    // Sort Now (green, as Numbers)
    let can = !rules.is_empty() && !categorized;
    let w = ui.available_width();
    let b = egui::Button::new(RichText::new("Sort Now").color(Color32::WHITE).size(14.0)).fill(if can { theme::ACCENT() } else { theme::ACCENT().gamma_multiply(0.5) }).corner_radius(7.0).min_size(egui::vec2(w, 30.0));
    if ui.add_enabled(can, b).clicked() {
        commit(app);
        let range = if selected {
            app.sel.filter(|s| s.table == t).map(|s| {
                let b = s.block();
                (app.mrow(t, b.0).unwrap_or(b.0), app.mrow(t, b.2).unwrap_or(b.2))
            })
        } else {
            None
        };
        let keys = rules.clone();
        if let Some(Err(e)) = app.edit("Sort", |d, h| d.sort_table_rows_by(h, t, &keys, range)) {
            app.notify(format!("Sort: {e}"));
        }
    }
    ui.add_space(6.0);
    ui.separator();
    theme::combo("sort-scope").selected_text(if selected { "Sort Selected Rows" } else { "Sort Entire Table" }).width(ui.available_width()).show_ui(ui, |ui| {
        ui.selectable_value(&mut selected, false, "Sort Entire Table");
        ui.selectable_value(&mut selected, true, "Sort Selected Rows");
    });
    ui.separator();
    let mut remove = None;
    for (i, (col, asc)) in rules.iter_mut().enumerate() {
        egui::Frame::new().fill(theme::CONTROL_BG()).corner_radius(7.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(theme::bold(if i == 0 { "Sort by" } else { "then by" }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if remove_button(ui) {
                        remove = Some(i);
                    }
                });
            });
            theme::combo(("sort-col", i)).selected_text(col_label(td, *col)).width(ui.available_width()).height(360.0).show_ui(ui, |ui| {
                for c in 0..td.cols {
                    ui.selectable_value(col, c, col_label(td, c));
                }
            });
            theme::combo(("sort-dir", i)).selected_text(if *asc { "Ascending" } else { "Descending" }).width(ui.available_width()).show_ui(ui, |ui| {
                ui.selectable_value(asc, true, "Ascending");
                ui.selectable_value(asc, false, "Descending");
            });
        });
        ui.add_space(4.0);
    }
    if let Some(i) = remove {
        rules.remove(i);
    }
    if rules.is_empty() {
        note(ui, "Sort rows by multiple columns.");
    }
    if let Some(c) = column_menu(ui, "sort-add", "Add a Column…", td) {
        rules.push((c, true));
    }
    if categorized {
        note(ui, "Sorting waits until categories are off.");
    }
    app.sorts.insert(t, (rules, selected));
}

fn filter(app: &mut App, ui: &mut egui::Ui, t: ObjId, td: &TableData) {
    let before = app.filters.get(&t).cloned().unwrap_or_default();
    let mut fl = before.clone();
    title_switch(ui, "Filters", &mut fl.on);
    let vals = app.wb.as_ref().map(|w| w.doc.cell_values(t)).unwrap_or_default();
    let mut remove = None;
    let mut typing = false;
    for (i, r) in fl.rules.iter_mut().enumerate() {
        egui::Frame::new().fill(theme::CONTROL_BG()).corner_radius(7.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(theme::bold(col_label(td, r.col)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if remove_button(ui) {
                        remove = Some(i);
                    }
                });
            });
            // the rule chooser, grouped as Numbers'
            theme::combo(("filter-rule", i)).selected_text(r.rule.label()).width(ui.available_width()).height(420.0).show_ui(ui, |ui| {
                for (group, rules) in Rule::GROUPS {
                    ui.label(RichText::new(*group).color(theme::TEXT_DIM()).small());
                    for (rule, label) in *rules {
                        ui.selectable_value(&mut r.rule, *rule, *label);
                    }
                    ui.separator();
                }
            });
            let n = r.rule.operands();
            if n >= 1 {
                let hint = match r.rule {
                    Rule::Top | Rule::Bottom => "how many (or 10%)",
                    Rule::InLastDays | Rule::InNextDays => "days",
                    Rule::DateIs | Rule::Before | Rule::After | Rule::DateBetween => "a date",
                    _ => "a value",
                };
                typing |= ui.add(egui::TextEdit::singleline(&mut r.a).hint_text(hint).desired_width(ui.available_width())).changed();
            }
            if n == 2 {
                ui.label(RichText::new("and").color(theme::TEXT_DIM()));
                typing |= ui.add(egui::TextEdit::singleline(&mut r.b).hint_text("a value").desired_width(ui.available_width())).changed();
            }
            if r.rule == Rule::Quick {
                // each value in the column, with how many rows hold it
                let mut counts: Vec<(String, usize)> = Vec::new();
                for row in td.header_rows..td.rows.saturating_sub(td.footer_rows) {
                    let s = td.cells.get((row * td.cols + r.col) as usize).map(|s| s.trim().to_string()).unwrap_or_default();
                    match counts.iter_mut().find(|c| c.0 == s) {
                        Some(c) => c.1 += 1,
                        None => counts.push((s, 1)),
                    }
                }
                counts.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
                ui.horizontal(|ui| {
                    if ui.small_button("Select All").clicked() {
                        r.values = counts.iter().map(|c| c.0.clone()).collect();
                    }
                    if ui.small_button("Deselect All").clicked() {
                        r.values.clear();
                    }
                });
                egui::ScrollArea::vertical().id_salt(("quick", i)).max_height(200.0).show(ui, |ui| {
                    for (v, n) in &counts {
                        let mut on = r.values.contains(v);
                        let shown = if v.is_empty() { "(Blank)" } else { v.as_str() };
                        if theme::checkbox(ui, &mut on, &format!("{shown}   {n}")).changed() {
                            if on {
                                r.values.push(v.clone());
                            } else {
                                r.values.retain(|x| x != v);
                            }
                        }
                    }
                });
            }
        });
        ui.add_space(4.0);
    }
    let _ = &vals;
    if let Some(i) = remove {
        fl.rules.remove(i);
    }
    if fl.rules.len() > 1 {
        theme::combo("filter-any").selected_text(if fl.any { "Match any filter" } else { "Match all filters" }).width(ui.available_width()).show_ui(ui, |ui| {
            ui.selectable_value(&mut fl.any, false, "Match all filters");
            ui.selectable_value(&mut fl.any, true, "Match any filter");
        });
    }
    if fl.rules.is_empty() {
        note(ui, "Add filters to choose which rows are shown in the table.");
    }
    if let Some(c) = column_menu(ui, "filter-add", "Add a Filter…", td) {
        // numbers get a number rule, anything else a text one (Numbers opens its chooser)
        let numeric = (td.header_rows..td.rows).any(|r| matches!(vals.get(&(r, c)), Some(nm_core::kn_model::table::CellValue::Number(_))));
        fl.rules.push(FilterRule::new(c, if numeric { Rule::Greater } else { Rule::Contains }));
        fl.on = true;
    }
    if fl != before {
        app.filters.insert(t, fl);
        commit(app);
        // typing a value: one undo step for the whole value
        app.merge_next_edit = typing;
        let _ = app.edit("Filter", |_, _| ());
        app.merge_next_edit = false;
    }
}

fn categories(app: &mut App, ui: &mut egui::Ui, t: ObjId, td: &TableData) {
    let before = app.cats.get(&t).cloned().unwrap_or_default();
    let mut c = before.clone();
    let from_file = c.levels.is_empty() && app.wb.as_ref().is_some_and(|w| nm_core::categories::groups(w.file(), t).is_some());
    let mut on = c.on || from_file;
    if title_switch(ui, "Categories", &mut on) && !from_file {
        c.on = on;
    }
    if from_file {
        if let Some(g) = app.wb.as_ref().and_then(|w| nm_core::categories::groups(w.file(), t)) {
            ui.label(format!("Grouped in the document: {} groups", g.len()));
            for grp in g.iter().take(40) {
                ui.label(format!("• {} ({})", grp.label, grp.rows.len()));
            }
            note(ui, "Add a category here to group the table your own way.");
        }
    } else if c.levels.is_empty() {
        note(ui, "Group rows into categories to organize and summarize the table.");
    }
    // the levels: column, group order, move up / down, remove
    let n = c.levels.len();
    let mut action: Option<(usize, i32)> = None; // (level, -1 up / +1 down / 0 remove)
    for (i, (col, desc)) in c.levels.iter_mut().enumerate() {
        egui::Frame::new().fill(theme::CONTROL_BG()).corner_radius(7.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(theme::bold(if i == 0 { "Category".to_string() } else { format!("Subcategory {i}") }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if remove_button(ui) {
                        action = Some((i, 0));
                    }
                    if i + 1 < n && ui.add(egui::Button::new("↓").frame(false)).on_hover_text("Demote").clicked() {
                        action = Some((i, 1));
                    }
                    if i > 0 && ui.add(egui::Button::new("↑").frame(false)).on_hover_text("Promote").clicked() {
                        action = Some((i, -1));
                    }
                });
            });
            theme::combo(("cat-col", i)).selected_text(col_label(td, *col)).width(ui.available_width()).height(360.0).show_ui(ui, |ui| {
                for k in 0..td.cols {
                    ui.selectable_value(col, k, col_label(td, k));
                }
            });
            theme::combo(("cat-dir", i)).selected_text(if *desc { "Descending" } else { "Ascending" }).width(ui.available_width()).show_ui(ui, |ui| {
                ui.selectable_value(desc, false, "Ascending");
                ui.selectable_value(desc, true, "Descending");
            });
        });
        ui.add_space(4.0);
    }
    match action {
        Some((i, 0)) => {
            c.levels.remove(i);
            c.collapsed.clear();
        }
        Some((i, d)) => {
            c.levels.swap(i, (i as i32 + d) as usize);
            c.collapsed.clear();
        }
        None => {}
    }
    if c.levels.len() < 5 {
        if let Some(col) = column_menu(ui, "cat-add", if c.levels.is_empty() { "Add a Category…" } else { "Add a Subcategory…" }, td) {
            c.levels.push((col, false));
            c.on = true;
        }
    }
    if !c.levels.is_empty() {
        ui.add_space(4.0);
        theme::checkbox(ui, &mut c.hide_col, "Hide category columns");
        ui.add_space(4.0);
        // the groups as shown, each collapsible
        let mut probe = td.clone();
        let (_, groups) = categorize(&mut probe, &Cats { collapsed: HashSet::new(), ..c.clone() });
        let mut keys: Vec<(usize, GroupRow)> = groups.into_iter().collect();
        keys.sort_by_key(|k| k.0);
        ui.horizontal(|ui| {
            if ui.button("Collapse All").clicked() {
                c.collapsed = keys.iter().map(|k| k.1.key.clone()).collect();
            }
            if ui.button("Expand All").clicked() {
                c.collapsed.clear();
            }
        });
        egui::ScrollArea::vertical().id_salt("cat-groups").max_height(260.0).show(ui, |ui| {
            for (_, g) in &keys {
                let closed = c.collapsed.contains(&g.key);
                let name = g.key.rsplit(" › ").next().unwrap_or(&g.key);
                if ui.selectable_label(false, format!("{}{} {name}  ({})", "    ".repeat(g.level), if closed { "▶" } else { "▼" }, g.rows.len())).clicked() {
                    if closed {
                        c.collapsed.remove(&g.key);
                    } else {
                        c.collapsed.insert(g.key.clone());
                    }
                }
            }
        });
    }
    if c != before {
        app.cats.insert(t, c);
        app.version += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TableData {
        // a header row, then Team / Name rows
        let cells = ["Team", "Name", "B", "Ann", "A", "Bob", "", "Cy", "B", "Di"].iter().map(|s| s.to_string()).collect();
        TableData { rows: 5, cols: 2, header_rows: 1, row_heights: vec![20.0; 5], col_widths: vec![80.0; 2], cells, ..Default::default() }
    }

    /// Groups in value order (blank last), each group row with its count, collapsed groups
    /// without their rows, the category column hidden on request.
    #[test]
    fn groups_rows_by_a_column() {
        let mut t = table();
        let (view, _) = categorize(&mut t, &Cats { on: true, levels: vec![(0, false)], ..Default::default() });
        assert_eq!(view.rows, [Some(0), None, Some(2), None, Some(1), Some(4), None, Some(3)]);
        assert_eq!(t.cells[2], "▼ A  (1)");
        let mut t = table();
        let mut c = Cats { on: true, levels: vec![(0, true)], hide_col: true, ..Default::default() };
        c.collapsed.insert("B".into());
        let (view, _) = categorize(&mut t, &c);
        assert_eq!(view.rows, [Some(0), None, None, Some(2), None, Some(3)]);
        assert_eq!(t.cells[2], "▶ B  (2)");
        assert_eq!(t.col_widths[0], 0.0);
        assert_eq!(col_label(&t, 1), "Name");
        // two levels: Team, then Name
        let mut t = table();
        let (view, groups) = categorize(&mut t, &Cats { on: true, levels: vec![(0, false), (1, false)], ..Default::default() });
        assert_eq!(view.rows, [Some(0), None, None, Some(2), None, None, Some(1), None, Some(4), None, None, Some(3)]);
        assert_eq!(groups[&5].key, "B › Ann");
        assert_eq!((groups[&4].level, groups[&4].rows.clone()), (0, vec![1, 4]));
    }
}

/// The Organize menu's commands (Values names them as Numbers 15.4 does).
pub const MENU: &[&str] = &[
    "Apply Sort Rules",
    "Sort Groups",
    "Sort Values",
    "Turn Filters On",
    "Turn Categories On",
    "Add Row to Group",
    "Create Group for Row",
    "Promote",
    "Demote",
    "Remove",
    "Collapse",
    "Collapse All Groups",
    "Expand",
    "Expand All Groups",
];

/// Whether `name` is one of the Organize commands (the "Add Category for …" item included).
pub fn is_menu(name: &str) -> bool {
    MENU.contains(&name) || name.starts_with("Add Category for")
}

/// The selected group row of a table categorized here: (table, its group row).
fn selected_group(app: &App) -> Option<(ObjId, GroupRow)> {
    let sel = app.sel?;
    let g = app.cat_groups.get(&sel.table)?.get(&(sel.cursor.0 as usize))?;
    Some((sel.table, g.clone()))
}

/// The label a menu item shows now (switches, the selected column's name).
pub fn label(app: &App, name: &str) -> Option<String> {
    let t = app.sel.map(|s| s.table)?;
    match name {
        "Turn Filters On" if app.filters.get(&t).is_some_and(|f| f.on) => Some("Turn Filters Off".into()),
        "Turn Categories On" if app.cats.get(&t).is_some_and(|c| c.on && !c.levels.is_empty()) => Some("Turn Categories Off".into()),
        n if n.starts_with("Add Category for") => {
            let td = app.wb.as_ref()?.table_data(t)?;
            Some(format!("Add Category for “{}”", col_label(&td, app.sel?.cursor.1)))
        }
        _ => None,
    }
}

pub fn enabled(app: &App, name: &str) -> bool {
    let Some(sel) = app.sel else { return false };
    let t = sel.table;
    let cats = app.cats.get(&t).filter(|c| c.on && !c.levels.is_empty());
    match name {
        "Apply Sort Rules" | "Sort Values" => app.sorts.get(&t).is_some_and(|s| !s.0.is_empty()),
        "Turn Filters On" => app.filters.get(&t).is_some_and(|f| !f.rules.is_empty()),
        "Turn Categories On" => app.cats.get(&t).is_some_and(|c| !c.levels.is_empty()),
        "Sort Groups" | "Collapse All Groups" | "Expand All Groups" => cats.is_some(),
        "Promote" => selected_group(app).is_some_and(|(_, g)| g.level > 0),
        "Demote" => selected_group(app).is_some_and(|(_, g)| g.level + 1 < cats.map(|c| c.levels.len()).unwrap_or(0)),
        "Remove" | "Collapse" | "Expand" => selected_group(app).is_some(),
        "Add Row to Group" | "Create Group for Row" => cats.is_some() && app.mrow(t, sel.cursor.0).is_some(),
        n if n.starts_with("Add Category for") => app.cats.get(&t).is_none_or(|c| c.levels.len() < 5 && !c.levels.iter().any(|l| l.0 == sel.cursor.1)),
        _ => false,
    }
}

pub fn run(app: &mut App, name: &str) {
    let Some(sel) = app.sel else { return };
    let t = sel.table;
    let mut cats = app.cats.get(&t).cloned().unwrap_or_default();
    match name {
        "Apply Sort Rules" | "Sort Values" => {
            commit(app);
            let keys = app.sorts.get(&t).map(|s| s.0.clone()).unwrap_or_default();
            // rows sorted in the table stay sorted inside each group
            if let Some(Err(e)) = app.edit("Sort", |d, h| d.sort_table_rows_by(h, t, &keys, None)) {
                app.notify(format!("Sort: {e}"));
            }
            return;
        }
        "Turn Filters On" => {
            if let Some(f) = app.filters.get_mut(&t) {
                f.on = !f.on;
            }
            commit(app);
            let _ = app.edit("Filter", |_, _| ());
            return;
        }
        "Turn Categories On" => cats.on = !cats.on,
        "Sort Groups" => {
            // the selected group's level (else the outermost) the other way round
            let level = selected_group(app).map(|g| g.1.level).unwrap_or(0);
            if let Some(l) = cats.levels.get_mut(level) {
                l.1 = !l.1;
            }
        }
        "Promote" | "Demote" => {
            let Some((_, g)) = selected_group(app) else { return };
            let to = if name == "Promote" { g.level - 1 } else { g.level + 1 };
            cats.levels.swap(g.level, to);
            cats.collapsed.clear();
        }
        "Remove" => {
            let Some((_, g)) = selected_group(app) else { return };
            cats.levels.remove(g.level);
            cats.collapsed.clear();
        }
        "Collapse" | "Expand" => {
            let Some((_, g)) = selected_group(app) else { return };
            if name == "Collapse" {
                cats.collapsed.insert(g.key);
            } else {
                cats.collapsed.remove(&g.key);
            }
        }
        "Collapse All Groups" => {
            cats.collapsed = app.cat_groups.get(&t).map(|m| m.values().map(|g| g.key.clone()).collect()).unwrap_or_default();
        }
        "Expand All Groups" => cats.collapsed.clear(),
        "Add Row to Group" | "Create Group for Row" => {
            let Some(row) = app.mrow(t, sel.cursor.0) else { return };
            let Some(td) = app.wb.as_ref().and_then(|w| w.table_data(t)) else { return };
            let levels = cats.levels.clone();
            commit(app);
            if name == "Add Row to Group" {
                // a new row under the selected one, in the same groups
                let values: Vec<(u32, String)> = levels.iter().map(|(c, _)| (*c, td.cells.get((row * td.cols + c) as usize).cloned().unwrap_or_default())).collect();
                let res = app.edit("Add Row", |d, h| -> Result<(), nm_core::kn_format::Error> {
                    crate::clip::insert_like(d, h, t, nm_core::kn_model::tablerows::Axis::Row, row + 1, row)?;
                    for (c, v) in &values {
                        nm_core::typed::set_typed(d, h, t, row + 1, *c, v)?;
                    }
                    Ok(())
                });
                if let Some(Err(e)) = res {
                    app.notify(format!("Add Row: {e}"));
                }
            } else {
                // the row in a group of its own at the innermost level ("Group 2", …)
                let Some((c, _)) = levels.last().copied() else { return };
                let taken: HashSet<String> = (0..td.rows).map(|r| group_label(&td, r, c)).collect();
                let new = (1..).map(|k| if k == 1 { "Group".to_string() } else { format!("Group {k}") }).find(|n| !taken.contains(n)).unwrap_or_default();
                if let Some(Err(e)) = app.edit("Create Group", |d, h| nm_core::typed::set_typed(d, h, t, row, c, &new)) {
                    app.notify(format!("Create Group: {e}"));
                }
            }
            return;
        }
        n if n.starts_with("Add Category for") => {
            cats.levels.push((sel.cursor.1, false));
            cats.on = true;
        }
        _ => return,
    }
    app.cats.insert(t, cats);
    app.version += 1;
}
