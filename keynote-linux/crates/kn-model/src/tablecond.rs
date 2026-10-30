//! Format > Conditional Highlighting: number rules ("greater than 5 -> Red Fill") on table
//! cells, stored the way Keynote 15.4 does it (Mac round 13, R13-condhl-*):
//!
//! - a TST.ConditionalStyleSetArchive in the table's conditional style list (one entry shared by
//!   the cells, refcount = cells) with the rule twice: `rules_prepivot` and `rules`, each with
//!   the predicate formula [cell, number, comparison] and the rule's cell and text styles;
//! - the styles (a TST.CellStyleArchive and a TSWP.ParagraphStyleArchive) in the shared
//!   ObjectContainer;
//! - each cell record gets the list key (field 0x80) and the matching rule (0x100: the rule's
//!   index, 15 when none applies);
//! - the calculation engine: one formula per cell in the table's conditional-style owner (cell
//!   records with an edge to the same cell of the table), and those cells in the UUID map.

use std::collections::BTreeSet;

use kn_format::{Archive, Msg, ObjId};

use crate::edit::History;
use crate::table::CellValue;
use crate::tablecells::{list_add, list_release, record_with, CellInput};
use crate::Document;

/// Comparison of a cell's number with a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

impl Cmp {
    pub const ALL: [Cmp; 6] = [Cmp::Equal, Cmp::NotEqual, Cmp::Greater, Cmp::GreaterOrEqual, Cmp::Less, Cmp::LessOrEqual];

    pub fn name(self) -> &'static str {
        match self {
            Cmp::Equal => "Equal to",
            Cmp::NotEqual => "Not equal to",
            Cmp::Greater => "Greater than",
            Cmp::GreaterOrEqual => "Greater than or equal to",
            Cmp::Less => "Less than",
            Cmp::LessOrEqual => "Less than or equal to",
        }
    }

    /// Keynote's predicate_type and AST comparison node (Mac round 23).
    fn keynote(self) -> (u64, &'static str) {
        match self {
            Cmp::Equal => (5, "EQUAL_TO_NODE"),
            Cmp::NotEqual => (6, "NOT_EQUAL_TO_NODE"),
            Cmp::Greater => (7, "GREATER_THAN_NODE"),
            Cmp::GreaterOrEqual => (8, "GREATER_THAN_OR_EQUAL_TO_NODE"),
            Cmp::Less => (9, "LESS_THAN_NODE"),
            Cmp::LessOrEqual => (10, "LESS_THAN_OR_EQUAL_TO_NODE"),
        }
    }

    fn from_keynote(t: u64) -> Option<Cmp> {
        Cmp::ALL.into_iter().find(|c| c.keynote().0 == t)
    }
}

/// A rule's look: the cell fill and text colour Keynote's presets use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub name: &'static str,
    /// Cell fill (fill styles; text styles get an empty cell style).
    pub fill: Option<[f32; 4]>,
    /// Text colour (written as font colour and text fill).
    pub text: Option<[f32; 4]>,
    pub bold: bool,
}

const BLACK: Option<[f32; 4]> = Some([0.0, 0.0, 0.0, 1.0]);

/// Keynote's built-in rule styles (Mac rounds 13 and 23).
pub const LOOKS: &[Look] = &[
    Look { name: "Red Fill", fill: Some([1.0, 0.592_156_9, 0.505_882_4, 0.9]), text: BLACK, bold: false },
    Look { name: "Green Fill", fill: Some([0.686_274_5, 0.894_117_7, 0.537_254_9, 0.9]), text: BLACK, bold: false },
    Look { name: "Blue Fill", fill: Some([0.533_333_3, 0.8, 1.0, 0.9]), text: BLACK, bold: false },
    Look { name: "Yellow Fill", fill: Some([1.0, 0.988_235_3, 0.596_078_4, 0.9]), text: BLACK, bold: false },
    Look { name: "Red Text", fill: None, text: Some([0.890_196_1, 0.141_176_5, 0.0, 1.0]), bold: false },
    Look { name: "Bold", fill: None, text: None, bold: true },
];

/// A cell's (row, col) and its raw record.
type RawCell = ((u32, u32), Vec<u8>);

/// A cell's rule, read back (Numbers lists them in the Cell inspector).
#[derive(Clone, Debug, PartialEq)]
pub struct HighlightRule {
    /// None: a kind of rule this editor doesn't make (text, dates...), shown by its look only.
    pub cmp: Option<Cmp>,
    pub value: f64,
    /// The look's name (Red Fill...), "Custom Style" when it has none.
    pub look: String,
}

fn color(m: &mut Msg, c: [f32; 4]) {
    m.set_enum("model", "rgb");
    m.set_f32("r", c[0]);
    m.set_f32("g", c[1]);
    m.set_f32("b", c[2]);
    m.set_f32("a", c[3]);
    m.set_enum("rgbspace", "srgb");
    m.set_f32("headroom", 1.0);
}

/// Keynote's decimal form of a number in predicates: (low, high) of the decimal128.
pub(crate) fn decimal_parts(v: f64) -> (u64, u64) {
    let b = crate::tablecells::decimal128_bytes(v);
    (u64::from_le_bytes(b[..8].try_into().unwrap()), u64::from_le_bytes(b[8..].try_into().unwrap()))
}

/// The predicate formula: [this cell, value, comparison].
fn predicate_formula(m: &mut Msg, table_cf: (u64, u64), cmp: Cmp, value: f64) {
    let nodes = m.msg_mut_or_create("AST_node_array");
    let cell = nodes.push_new("AST_node");
    cell.set_enum("AST_node_type", "LINKED_CELL_REF_NODE");
    let t = cell.msg_mut_or_create("AST_cross_table_reference_extra_info").msg_mut_or_create("table_id");
    t.set_u64("uuid_w0", table_cf.0 & 0xffff_ffff);
    t.set_u64("uuid_w1", table_cf.0 >> 32);
    t.set_u64("uuid_w2", table_cf.1 & 0xffff_ffff);
    t.set_u64("uuid_w3", table_cf.1 >> 32);
    let n = nodes.push_new("AST_node");
    n.set_enum("AST_node_type", "NUMBER_NODE");
    n.set_f64("AST_number_node_number", value);
    let (lo, hi) = decimal_parts(value);
    n.set_u64("AST_number_node_decimal_low", lo);
    n.set_u64("AST_number_node_decimal_high", hi);
    nodes.push_new("AST_node").set_enum("AST_node_type", cmp.keynote().1);
}

impl Document {
    /// Whether Format > Conditional Highlighting can add rules to this table.
    pub fn can_highlight(&self, table: ObjId) -> bool {
        let Some(model) = self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)) else { return false };
        model.msg("base_data_store").and_then(|d| d.ref_id("conditionalstyletable")).is_some() && model.msg("conditional_style_formula_owner_id").is_some() && self.merge_ctx(table).is_some()
    }

    /// Add the rule "cell `cmp` `value` -> `look`" to the cells of block (r0, c0, r1, c1) that
    /// hold values (empty cells are left as they are). Cells that have rules keep them: the new
    /// rule comes after theirs (as Numbers' Add a Rule), each group of cells sharing a rule set
    /// getting a new set.
    pub fn add_highlight_rule(&mut self, h: &mut History, table: ObjId, block: (u32, u32, u32, u32), cmp: Cmp, value: f64, look: &Look) -> Result<usize, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let raws = self.block_records(table, block);
        if raws.is_empty() {
            return Err(err("select cells with values first"));
        }
        let mut groups: std::collections::BTreeMap<Option<u32>, Vec<RawCell>> = Default::default();
        for (rc, rec) in raws {
            groups.entry(crate::tablecells::record_conditional(&rec)).or_default().push((rc, rec));
        }
        let mut n = 0;
        for (old, cells) in groups {
            n += self.add_rule_to_cells(h, table, cells, old, cmp, value, look)?;
        }
        Ok(n)
    }

    /// The raw records of the cells (with values) in `block`.
    fn block_records(&self, table: ObjId, block: (u32, u32, u32, u32)) -> Vec<((u32, u32), Vec<u8>)> {
        let Some(model) = self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)) else { return Vec::new() };
        crate::table::raw_cell_records(&self.file, model)
            .into_iter()
            .filter(|((r, c), _)| (block.0..=block.2).contains(r) && (block.1..=block.3).contains(c))
            .collect()
    }

    /// Add a rule to cells that share the rule set `old` (or have none).
    #[allow(clippy::too_many_arguments)]
    fn add_rule_to_cells(&mut self, h: &mut History, table: ObjId, raws: Vec<((u32, u32), Vec<u8>)>, old: Option<u32>, cmp: Cmp, value: f64, look: &Look) -> Result<usize, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let cx = self.merge_ctx(table).ok_or(err("this table's rules can't be edited"))?;
        let model = self.file.obj(cx.model).ok_or(err("no table"))?;
        let list = model.msg("base_data_store").and_then(|d| d.ref_id("conditionalstyletable")).ok_or(err("no conditional style list"))?;
        let w = model.msg("conditional_style_formula_owner_id").ok_or(err("no conditional style owner"))?;
        let wv = |n: &str| w.u64(n).unwrap_or(0);
        let cond_uuid = (wv("uuid_w0") | wv("uuid_w1") << 32, wv("uuid_w2") | wv("uuid_w3") << 32);
        let table_uuid = {
            let d = self.file.obj(cx.table_deps).and_then(|d| d.msg("formula_owner_uid")).ok_or(err("no table owner"))?;
            (d.u64("lower").unwrap_or(0), d.u64("upper").unwrap_or(0))
        };
        let (engine_obj, _) = self.file.first_of("TSCE.CalculationEngineArchive").ok_or(err("no calculation engine"))?;
        let owners = self.file.obj(engine_obj).and_then(|ce| ce.msg("dependency_tracker")).map(|d| d.ref_ids("formula_owner_dependencies")).unwrap_or_default();
        let cond_deps = owners
            .into_iter()
            .find(|d| self.file.obj(*d).and_then(|m| m.msg("formula_owner_uid")).is_some_and(|u| (u.u64("lower").unwrap_or(0), u.u64("upper").unwrap_or(0)) == cond_uuid))
            .ok_or(err("no conditional style owner in the calculation engine"))?;
        let cond_owner = self.file.obj(cond_deps).and_then(|m| m.u64("internal_formula_owner_id")).ok_or(err("owner id"))?;

        // the cells with values, and their raw records
        let values: std::collections::HashMap<(u32, u32), CellValue> = crate::table::cell_records(&self.file, model).into_iter().map(|(rc, r)| (rc, r.value)).collect();
        if raws.is_empty() {
            return Err(err("select cells with values first"));
        }
        if raws.iter().any(|(_, r)| r.first() != Some(&5)) {
            return Err(err("this table uses an older cell format"));
        }
        // the rules the cells have, kept first in the new set
        let old_set: Option<(ObjId, Msg)> = old.and_then(|key| {
            let id = self.file.obj(list)?.msgs("entries").find(|e| e.u32("key") == Some(key))?.ref_id("reference")?;
            Some((id, self.file.obj(id)?.clone()))
        });

        // styles in the ObjectContainer
        let (oc, oc_file) = self.ensure_object_container(h)?;
        let list_file = self.file.file_of(list).ok_or(err("no list file"))?.to_string();
        let n_cells = raws.len() as u64;
        let (cell_style, text_style, set_id) = self.structural(h, &[oc, list], |k| {
            let ids = k.alloc_ids(3);
            let mut ts = Msg::new("TSWP.ParagraphStyleArchive");
            ts.msg_mut_or_create("super").set_str("name", look.name);
            ts.set_u64("override_count", 1);
            let cp = ts.msg_mut_or_create("char_properties");
            if let Some(t) = look.text {
                color(cp.msg_mut_or_create("font_color"), t);
                color(cp.msg_mut_or_create("tsd_fill").msg_mut_or_create("color"), t);
            }
            if look.bold {
                cp.set_bool("bold", true);
            }
            ts.msg_mut_or_create("para_properties");
            // every rule needs a cell style (required field; Keynote aborts without one): text-only
            // looks get an empty one (Mac round 21)
            let mut cs = Msg::new("TST.CellStyleArchive");
            cs.msg_mut_or_create("super");
            if let Some(f) = look.fill {
                cs.set_u64("override_count", 1);
                color(cs.msg_mut_or_create("cell_properties").msg_mut_or_create("cell_fill").msg_mut_or_create("color"), f);
            }
            let objs = vec![(ids[1], ts), (ids[2], cs)];
            for (id, m) in objs {
                k.insert_archive(&oc_file, Archive::new_object(id, m));
                if let Some(o) = k.obj_mut(oc) {
                    o.push_ref("objects", id);
                }
                k.add_uuid_entry(&oc_file, id);
            }
            // the rule set
            let mut set = Msg::new("TST.ConditionalStyleSetArchive");
            let before = old_set.as_ref().map(|(_, o)| o.msgs("rules_prepivot").cloned().collect::<Vec<_>>()).unwrap_or_default();
            set.set_u64("ruleCount", before.len() as u64 + 1);
            for r in before {
                set.push_msg("rules_prepivot", r);
            }
            let pre = set.push_new("rules_prepivot");
            let p = pre.msg_mut_or_create("predicate");
            predicate_formula(p.msg_mut_or_create("formula"), (table_uuid.0, table_uuid.1), cmp, value);
            p.set_u64("predicate_type", cmp.keynote().0);
            p.set_u64("qualifier1", 0);
            p.set_u64("qualifier2", 0);
            p.set_u64("param_index1", 1);
            p.set_i64("param_index2", -1);
            p.set_u64("param_index0", 0);
            pre.set_ref("cell_style", ids[2]);
            pre.set_ref("text_style", ids[1]);
            for r in old_set.as_ref().and_then(|(_, o)| o.msg("rules")).map(|r| r.msgs("rule").cloned().collect::<Vec<_>>()).unwrap_or_default() {
                set.msg_mut_or_create("rules").push_msg("rule", r);
            }
            let rule = set.msg_mut_or_create("rules").push_new("rule");
            let p = rule.msg_mut_or_create("predicate");
            p.set_u64("predicate_type", cmp.keynote().0);
            p.set_u64("qualifier1", 0);
            p.set_u64("qualifier2", 0);
            let a0 = p.msg_mut_or_create("param_value0");
            a0.set_u64("arg_type", 4);
            let rc = a0.msg_mut_or_create("relative_cell_ref");
            rc.set_i64("relative_row_offset", 0);
            rc.set_i64("relative_column_offset", 0);
            let tu = rc.msg_mut_or_create("table_uid");
            tu.set_u64("lower", table_uuid.0);
            tu.set_u64("upper", table_uuid.1);
            for f in ["preserve_column", "preserve_row", "is_spanning_column", "is_spanning_row"] {
                rc.set_u64(f, 0);
            }
            a0.set_u64("preserve_row", 0);
            a0.set_u64("preserve_column", 0);
            let a1 = p.msg_mut_or_create("param_value1");
            a1.set_u64("arg_type", 1);
            let av = a1.msg_mut_or_create("arg_value");
            av.set_f64("double_value", value);
            let (lo, hi) = decimal_parts(value);
            av.set_u64("decimal_low", lo);
            av.set_u64("decimal_high", hi);
            p.msg_mut_or_create("param_value2").set_u64("arg_type", 0);
            predicate_formula(p.msg_mut_or_create("formula"), (table_uuid.0, table_uuid.1), cmp, value);
            p.set_u64("for_conditional_style", 1);
            rule.set_ref("cell_style", ids[2]);
            rule.set_ref("text_style", ids[1]);
            let mut a = Archive::new_object(ids[0], set);
            a.refresh_object_references();
            k.insert_archive(&list_file, a);
            (ids[2], ids[1], ids[0])
        });
        let _ = (cell_style, text_style);

        // the list entry, shared by the cells
        self.touch(h, list);
        let l = self.file.obj_mut(list).ok_or(err("list"))?;
        let key = list_add(l, |_| false, |e| e.set_ref("reference", set_id));
        if let Some(e) = l.msgs_mut("entries").find(|e| e.u32("key") == Some(key)) {
            e.set_u64("refcount", n_cells);
        }
        self.file.refresh_refs(list);

        // the cells
        for ((r, c), rec) in &raws {
            let applied = self.conditional_rule_applied(table, key, values.get(&(*r, *c)).unwrap_or(&CellValue::Empty));
            let new = record_with(rec, &[(0x80, key), (0x100, applied)], &[]).ok_or(err("cell record"))?;
            self.set_cell(h, table, *r, *c, &CellInput::Raw(new))?;
        }

        // the old set loses the cells (and goes once nothing uses it); they are in the
        // calculation engine already
        if let (Some(old), Some((old_id, _))) = (old, &old_set) {
            self.release_rule_set(h, list, old, *old_id, raws.len());
            return Ok(raws.len());
        }

        // the calculation engine
        let ce_file = self.file.file_of(cond_deps).ok_or(err("engine file"))?.to_string();
        let cells: Vec<(u32, u32)> = raws.iter().map(|(rc, _)| *rc).collect();
        let table_owner = cx.table_owner;
        self.structural(h, &[engine_obj, cond_deps, cx.table_deps], |k| {
            if let Some(dt) = k.obj_mut(engine_obj).map(|ce| ce.msg_mut_or_create("dependency_tracker")) {
                let n = dt.u64("number_of_formulas").unwrap_or(0);
                dt.set_u64("number_of_formulas", n + n_cells);
            }
            // the UUID map: the conditional owner refers to these cells of the table
            if let Some(ce) = k.obj_mut(engine_obj) {
                let map = ce.msg_mut_or_create("uuid_reference_map");
                if !map.msgs("cell_refs_for_uuid").any(|e| e.msg("uuid").is_some_and(|u| (u.u64("lower").unwrap_or(0), u.u64("upper").unwrap_or(0)) == table_uuid)) {
                    let u = map.push_new("cell_refs_for_uuid").msg_mut_or_create("uuid");
                    u.set_u64("lower", table_uuid.0);
                    u.set_u64("upper", table_uuid.1);
                }
                let e = map.msgs_mut("cell_refs_for_uuid").find(|e| e.msg("uuid").is_some_and(|u| (u.u64("lower").unwrap_or(0), u.u64("upper").unwrap_or(0)) == table_uuid)).unwrap();
                let refs = e.msg_mut_or_create("cell_refs");
                if !refs.msgs("owner_entries").any(|o| o.u64("owner_id") == Some(cond_owner)) {
                    refs.push_new("owner_entries").set_u64("owner_id", cond_owner);
                }
                let o = refs.msgs_mut("owner_entries").find(|o| o.u64("owner_id") == Some(cond_owner)).unwrap();
                set_coords(o.msg_mut_or_create("coord_set"), &cells);
            }
            // the conditional owner: a formula per cell, with an edge to the same table cell
            let record = |m: &mut Msg, (r, c): (u32, u32)| {
                m.set_u64("column", u64::from(c));
                m.set_u64("row", u64::from(r));
                let e = m.msg_mut_or_create("expanded_edges");
                e.set_u64("edge_with_owner_rows", u64::from(r));
                e.set_u64("edge_with_owner_columns", u64::from(c));
                e.set_u64("internal_owner_id_for_edge", table_owner);
            };
            for rc in &cells {
                // each cell's record goes in the owner's tile covering it (128 rows x 32 columns)
                let tile = crate::tableformula::cell_record_tile(k, cond_deps, &ce_file, cond_owner, rc.0, rc.1);
                if let Some(m) = k.obj_mut(tile) {
                    record(m.push_new("cell_records"), *rc);
                }
                if let Some(d) = k.obj_mut(cond_deps) {
                    record(d.msg_mut_or_create("cell_dependencies").push_new("cell_record"), *rc);
                }
            }
            // and Keynote gives the table owner an (empty) cell tile
            let table_has = k
                .obj(cx.table_deps)
                .and_then(|d| d.msg("tiled_cell_dependencies"))
                .map(|t| t.ref_ids("cell_record_tiles"))
                .unwrap_or_default()
                .into_iter()
                .any(|t| k.obj(t).and_then(|m| m.u64("internal_owner_id")) == Some(table_owner));
            if !table_has {
                let id = k.alloc_ids(1)[0];
                let mut m = Msg::new("TSCE.CellRecordTileArchive");
                m.set_u64("internal_owner_id", table_owner);
                m.set_u64("tile_column_begin", 0);
                m.set_u64("tile_row_begin", 0);
                k.insert_archive(&ce_file, Archive::new_object(id, m));
                if let Some(d) = k.obj_mut(cx.table_deps) {
                    d.msg_mut_or_create("tiled_cell_dependencies").push_ref("cell_record_tiles", id);
                }
                k.refresh_refs(cx.table_deps);
            }
            k.refresh_refs(cond_deps);
            k.ensure_external_refs(&ce_file);
            k.ensure_external_refs(&list_file);
        });
        Ok(cells.len())
    }
}

impl Document {
    /// The rules of cell (row, col), in the order they are tried.
    pub fn highlight_rules(&self, table: ObjId, row: u32, col: u32) -> Vec<HighlightRule> {
        let Some(model) = self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)) else { return Vec::new() };
        let Some(key) = crate::table::raw_cell_records(&self.file, model).into_iter().find(|(rc, _)| *rc == (row, col)).and_then(|(_, r)| crate::tablecells::record_conditional(&r)) else { return Vec::new() };
        let set = model
            .msg("base_data_store")
            .and_then(|d| d.ref_id("conditionalstyletable"))
            .and_then(|l| self.file.obj(l))
            .and_then(|l| l.msgs("entries").find(|e| e.u32("key") == Some(key))?.ref_id("reference"))
            .and_then(|s| self.file.obj(s));
        let Some(set) = set else { return Vec::new() };
        set.msgs("rules_prepivot")
            .map(|r| {
                let p = r.msg("predicate");
                let value = p.and_then(|p| p.msg("formula")).and_then(|f| f.msg("AST_node_array")).and_then(|a| a.msgs("AST_node").find_map(|n| n.f64("AST_number_node_number"))).unwrap_or(0.0);
                let look = r.ref_id("text_style").and_then(|t| self.file.obj(t)).and_then(|t| t.msg("super")?.str("name").map(str::to_string)).filter(|n| !n.is_empty()).unwrap_or_else(|| "Custom Style".into());
                HighlightRule { cmp: p.and_then(|p| p.u64("predicate_type")).and_then(Cmp::from_keynote), value, look }
            })
            .collect()
    }

    /// Format > Conditional Highlighting > Clear All Rules for the cells of `block`.
    pub fn clear_highlight_rules(&mut self, h: &mut History, table: ObjId, block: (u32, u32, u32, u32)) -> Result<usize, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let raws: Vec<((u32, u32), Vec<u8>, u32)> = self.block_records(table, block).into_iter().filter_map(|(rc, r)| Some((rc, r.clone(), crate::tablecells::record_conditional(&r)?))).collect();
        if raws.is_empty() {
            return Ok(0);
        }
        let model = self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)).ok_or(err("no table"))?;
        let list = model.msg("base_data_store").and_then(|d| d.ref_id("conditionalstyletable")).ok_or(err("no conditional style list"))?;
        for ((r, c), rec, _) in &raws {
            let new = record_with(rec, &[], &[0x80, 0x100]).ok_or(err("cell record"))?;
            self.set_cell(h, table, *r, *c, &CellInput::Raw(new))?;
        }
        let mut per_set: std::collections::BTreeMap<u32, usize> = Default::default();
        for (_, _, k) in &raws {
            *per_set.entry(*k).or_default() += 1;
        }
        for (key, n) in per_set {
            let id = self.file.obj(list).and_then(|l| l.msgs("entries").find(|e| e.u32("key") == Some(key))?.ref_id("reference"));
            if let Some(id) = id {
                self.release_rule_set(h, list, key, id, n);
            }
        }
        // the calculation engine forgets the cells
        let cells: Vec<(u32, u32)> = raws.iter().map(|(rc, ..)| *rc).collect();
        let model = self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)).ok_or(err("no table"))?;
        let w = model.msg("conditional_style_formula_owner_id").ok_or(err("no conditional style owner"))?;
        let wv = |n: &str| w.u64(n).unwrap_or(0);
        let cond_uuid = (wv("uuid_w0") | wv("uuid_w1") << 32, wv("uuid_w2") | wv("uuid_w3") << 32);
        let (engine_obj, _) = self.file.first_of("TSCE.CalculationEngineArchive").ok_or(err("no calculation engine"))?;
        let owners = self.file.obj(engine_obj).and_then(|ce| ce.msg("dependency_tracker")).map(|d| d.ref_ids("formula_owner_dependencies")).unwrap_or_default();
        let uid = |m: &Msg| (m.u64("lower").unwrap_or(0), m.u64("upper").unwrap_or(0));
        let Some(cond_deps) = owners.into_iter().find(|d| self.file.obj(*d).and_then(|m| m.msg("formula_owner_uid")).is_some_and(|u| uid(u) == cond_uuid)) else { return Ok(cells.len()) };
        let cond_owner = self.file.obj(cond_deps).and_then(|m| m.u64("internal_formula_owner_id")).unwrap_or(0);
        let tiles: Vec<ObjId> = self.file.obj(cond_deps).and_then(|d| d.msg("tiled_cell_dependencies")).map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default();
        let mut touch = vec![engine_obj, cond_deps];
        touch.extend(&tiles);
        let n_cells = cells.len() as u64;
        self.structural(h, &touch, |k| {
            if let Some(dt) = k.obj_mut(engine_obj).map(|ce| ce.msg_mut_or_create("dependency_tracker")) {
                let n = dt.u64("number_of_formulas").unwrap_or(0);
                dt.set_u64("number_of_formulas", n.saturating_sub(n_cells));
            }
            if let Some(map) = k.obj_mut(engine_obj).and_then(|ce| ce.msg_mut("uuid_reference_map")) {
                for e in map.msgs_mut("cell_refs_for_uuid") {
                    let Some(refs) = e.msg_mut("cell_refs") else { continue };
                    for o in refs.msgs_mut("owner_entries").filter(|o| o.u64("owner_id") == Some(cond_owner)) {
                        unset_coords(o.msg_mut_or_create("coord_set"), &cells);
                    }
                }
            }
            let gone = |m: &Msg| cells.contains(&(m.u32("row").unwrap_or(u32::MAX), m.u32("column").unwrap_or(u32::MAX)));
            for t in &tiles {
                if let Some(m) = k.obj_mut(*t).filter(|m| m.u64("internal_owner_id") == Some(cond_owner)) {
                    m.retain_msgs("cell_records", |r| !gone(r));
                }
            }
            if let Some(cd) = k.obj_mut(cond_deps).and_then(|d| d.msg_mut("cell_dependencies")) {
                cd.retain_msgs("cell_record", |r| !gone(r));
            }
        });
        Ok(cells.len())
    }

    /// `n` cells stop using rule set `key` (object `id`): its entry's count goes down, and the
    /// set goes when no cell has it.
    fn release_rule_set(&mut self, h: &mut History, list: ObjId, key: u32, id: ObjId, n: usize) {
        self.touch(h, list);
        let Some(l) = self.file.obj_mut(list) else { return };
        for _ in 0..n {
            list_release(l, key);
        }
        let used = l.msgs("entries").any(|e| e.u32("key") == Some(key));
        if !used {
            self.touch(h, id);
            self.file.remove_archive(id);
        }
        self.file.refresh_refs(list);
    }
}

/// Take cells out of a TSCE.CellCoordSetArchive.
fn unset_coords(set: &mut Msg, cells: &[(u32, u32)]) {
    let mut keep: Vec<(u32, u32)> = Vec::new();
    for e in set.msgs("column_entries") {
        let col = e.u32("column").unwrap_or(0);
        for x in e.msg("row_set").map(|r| r.msgs("entries").collect::<Vec<_>>()).unwrap_or_default() {
            let b = x.u32("range_begin").unwrap_or(0);
            keep.extend((b..=x.u32("range_end").unwrap_or(b)).map(|r| (r, col)).filter(|rc| !cells.contains(rc)));
        }
    }
    set.remove("column_entries");
    set_coords(set, &keep);
}

/// Add cells to a TSCE.CellCoordSetArchive, keeping each column's rows as ranges.
fn set_coords(set: &mut Msg, cells: &[(u32, u32)]) {
    let cols: BTreeSet<u32> = cells.iter().map(|c| c.1).collect();
    for col in cols {
        let mut rows: BTreeSet<u32> = BTreeSet::new();
        if let Some(e) = set.msgs("column_entries").find(|e| e.u32("column") == Some(col)) {
            for x in e.msg("row_set").map(|r| r.msgs("entries").collect::<Vec<_>>()).unwrap_or_default() {
                let b = x.u32("range_begin").unwrap_or(0);
                rows.extend(b..=x.u32("range_end").unwrap_or(b));
            }
        }
        rows.extend(cells.iter().filter(|c| c.1 == col).map(|c| c.0));
        set.retain_msgs("column_entries", |e| e.u32("column") != Some(col));
        let e = set.push_new("column_entries");
        e.set_u64("column", u64::from(col));
        let rs = e.msg_mut_or_create("row_set");
        let v: Vec<u32> = rows.into_iter().collect();
        let mut i = 0;
        while i < v.len() {
            let mut j = i;
            while j + 1 < v.len() && v[j + 1] == v[j] + 1 {
                j += 1;
            }
            let x = rs.push_new("entries");
            x.set_u64("range_begin", u64::from(v[i]));
            if j > i {
                x.set_u64("range_end", u64::from(v[j]));
            }
            i = j + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DrawableKind;

    fn first_table(d: &Document) -> ObjId {
        let s = d.slides();
        d.scene(&s, 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id
    }

    /// "Greater than 5 -> Red Fill" on B2:B9 matches Keynote's own rule (R13-condhl-1-rule).
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn greater_than_rule_matches_keynote() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref");
        let mut d = Document::open(dir.join("R13-condhl-0-before.key")).unwrap();
        let k = Document::open(dir.join("R13-condhl-1-rule.key")).unwrap();
        let t = first_table(&d);
        let mut h = History::default();
        d.begin(&mut h, "Rule");
        assert_eq!(d.add_highlight_rule(&mut h, t, (1, 1, 8, 1), Cmp::Greater, 5.0, &LOOKS[0]).unwrap(), 8);
        d.end(&mut h);
        let d = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let shape = |doc: &Document| {
            let t = first_table(doc);
            let model = doc.file.obj(doc.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap();
            // the list key itself may differ (Keynote's was 2); everything else must match
            let mut raws: Vec<_> = crate::table::raw_cell_records(&doc.file, model)
                .into_iter()
                .map(|(rc, r)| (rc, if crate::tablecells::record_conditional(&r).is_some() { record_with(&r, &[(0x80, 0)], &[]).unwrap() } else { r }))
                .collect();
            raws.sort();
            let list = doc.file.obj(model.msg("base_data_store").unwrap().ref_id("conditionalstyletable").unwrap()).unwrap();
            let set = doc.file.obj(list.msgs("entries").next().unwrap().ref_id("reference").unwrap()).unwrap();
            let mut set = set.clone();
            set.for_each_ref_mut(&mut |r| *r = 0);
            let (ce, cem) = doc.file.first_of("TSCE.CalculationEngineArchive").unwrap();
            let _ = ce;
            let sc = doc.scene(&doc.slides(), 0).unwrap();
            let fills = match &sc.drawables.iter().find(|x| x.id == t).unwrap().kind {
                DrawableKind::Table(tb) => format!("{:?}", tb.cell_styles.iter().map(|s| s.fill.clone()).collect::<Vec<_>>()),
                _ => String::new(),
            };
            (
                raws,
                format!("{set:?}"),
                list.msgs("entries").map(|e| e.u32("refcount")).collect::<Vec<_>>(),
                cem.msg("dependency_tracker").unwrap().u64("number_of_formulas"),
                format!("{:?}", cem.msg("uuid_reference_map")),
                fills,
            )
        };
        // the conditional owner's per-cell records and tiles
        let deps = |doc: &Document| {
            let (_, ce) = doc.file.first_of("TSCE.CalculationEngineArchive").unwrap();
            let mut out = Vec::new();
            for id in ce.msg("dependency_tracker").unwrap().ref_ids("formula_owner_dependencies") {
                let m = doc.file.obj(id).unwrap();
                let tiles: Vec<String> = m
                    .msg("tiled_cell_dependencies")
                    .map(|t| t.ref_ids("cell_record_tiles"))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| format!("{:?}", doc.file.obj(t).unwrap()))
                    .collect();
                out.push((m.u64("internal_formula_owner_id"), format!("{:?}", m.msg("cell_dependencies")), tiles));
            }
            out.sort();
            out
        };
        assert_eq!(deps(&d), deps(&k), "engine owners");
        let (a, b) = (shape(&d), shape(&k));
        assert_eq!(a.0, b.0, "cell records");
        assert_eq!(a.1, b.1, "rule set");
        assert_eq!(a.2, b.2, "list entry");
        assert_eq!(a.3, b.3, "formula count");
        assert_eq!(a.4, b.4, "uuid map");
        assert_eq!(a.5, b.5, "fills");
    }

    /// Editing a highlighted cell keeps its rule and updates which one applies.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn edit_keeps_rule() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref");
        let mut d = Document::open(dir.join("R13-condhl-1-rule.key")).unwrap();
        let t = first_table(&d);
        let mut h = History::default();
        d.begin(&mut h, "Edit");
        d.set_cell(&mut h, t, 1, 1, &CellInput::Number(50.0)).unwrap();
        d.end(&mut h);
        let model = d.file.obj(d.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap();
        let rec = crate::table::raw_cell_records(&d.file, model).into_iter().find(|(rc, _)| *rc == (1, 1)).unwrap().1;
        assert_eq!(crate::tablecells::record_conditional(&rec), Some(2));
        let sc = d.scene(&d.slides(), 0).unwrap();
        let DrawableKind::Table(tb) = &sc.drawables.iter().find(|x| x.id == t).unwrap().kind else { panic!() };
        assert!(matches!(tb.cell_styles[(tb.cols + 1) as usize].fill, crate::Fill::Color(c) if c.r > 0.9 && c.g < 0.7));
    }

    /// A second rule goes after the first (cells keep theirs), the rules read back in order,
    /// the shared set's count stays right, and Clear All Rules takes them away again.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn second_rule_list_and_clear() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref");
        let mut d = Document::open(dir.join("R13-condhl-0-before.key")).unwrap();
        let t = first_table(&d);
        let mut h = History::default();
        d.begin(&mut h, "Rule");
        let n = d.add_highlight_rule(&mut h, t, (1, 1, 8, 1), Cmp::Greater, 5.0, &LOOKS[0]).unwrap();
        // the second rule on part of the cells only: two sets afterwards
        d.add_highlight_rule(&mut h, t, (1, 1, 3, 1), Cmp::Less, 2.0, &LOOKS[1]).unwrap();
        d.end(&mut h);
        let rules = d.highlight_rules(t, 1, 1);
        assert_eq!(rules.iter().map(|r| (r.cmp, r.value, r.look.as_str())).collect::<Vec<_>>(), [(Some(Cmp::Greater), 5.0, "Red Fill"), (Some(Cmp::Less), 2.0, "Green Fill")]);
        assert_eq!(d.highlight_rules(t, 8, 1).len(), 1);
        let model = d.file.obj(d.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap();
        let list = d.file.obj(model.msg("base_data_store").unwrap().ref_id("conditionalstyletable").unwrap()).unwrap();
        let mut counts: Vec<u32> = list.msgs("entries").map(|e| e.u32("refcount").unwrap()).collect();
        counts.sort();
        assert_eq!(counts, [3, n as u32 - 3]);
        d.begin(&mut h, "Clear");
        d.clear_highlight_rules(&mut h, t, (1, 1, 8, 1)).unwrap();
        d.end(&mut h);
        assert!(d.highlight_rules(t, 1, 1).is_empty() && d.highlight_rules(t, 8, 1).is_empty());
        let model = d.file.obj(d.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap();
        let list = d.file.obj(model.msg("base_data_store").unwrap().ref_id("conditionalstyletable").unwrap()).unwrap();
        assert_eq!(list.count("entries"), 0);
        // and it saves and opens again
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        assert!(d2.highlight_rules(first_table(&d2), 1, 1).is_empty());
        d.undo(&mut h);
        assert_eq!(d.highlight_rules(t, 1, 1).len(), 2);
    }
}
