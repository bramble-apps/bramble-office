//! Table attachments (com.apple.notes.table): ICAttachment.mergeableData (ZMERGEABLEDATA1), a gzipped
//! CRDT object graph (format 4.1/4.2) <-> [`model::Table`].
//!
//! Layout: MergableDataProto { 2: MergableDataObject { 1, 2 version, 3: Data { 1, 2, 3 Entry*,
//! 4 key_items*, 5 type_items*, 6 uuid_items*, 7 VectorTimestamp } } }. The root entry is a CustomMap
//! of type com.apple.notes.ICTable with crRows / crColumns (ordered sets of uuids), the column
//! direction register and cellColumns { column uuid -> { row uuid -> cell topotext } }.
//! Ported from apple_cloud_notes_parser's table logic (MIT, see THIRD-PARTY.md) and the NO1 research.

use std::collections::HashMap;

use kn_format::Msg;

use crate::body;
use crate::model::{Body, Table};
use crate::pb::{self, Result};

/// An ObjectID: {2 unsigned, 4 string, 6 object index}.
#[derive(Clone, Debug, PartialEq)]
enum Oid {
    Obj(usize),
    Int(u64),
    Str(String),
    None,
}

fn oid(m: &Msg) -> Oid {
    if let Some(i) = pb::uint(m, 6) {
        Oid::Obj(i as usize)
    } else if let Some(v) = pb::uint(m, 2) {
        Oid::Int(v)
    } else if let Some(s) = pb::string(m, 4) {
        Oid::Str(s)
    } else {
        Oid::None
    }
}

enum Entry {
    /// CustomMap: type name, key -> value
    Map(String, HashMap<String, Oid>),
    /// OrderedSet: positions in display order (array uuids), position uuid object -> value object
    Ordered(Vec<Vec<u8>>, Vec<(Oid, Oid)>),
    Dict(Vec<(Oid, Oid)>),
    Text(Msg),
    Register(Oid),
    Other,
}

struct Graph {
    uuids: Vec<Vec<u8>>,
    objs: Vec<Entry>,
}

impl Graph {
    fn obj(&self, o: &Oid) -> Option<&Entry> {
        match o {
            Oid::Obj(i) => self.objs.get(*i),
            _ => None,
        }
    }

    /// An NSUUID object (CustomMap UUIDIndex -> index into uuid_items) -> its 16 bytes.
    fn uuid_of(&self, o: &Oid) -> Option<Vec<u8>> {
        match self.obj(o)? {
            Entry::Map(_, e) => match e.get("UUIDIndex")? {
                Oid::Int(i) => self.uuids.get(*i as usize).cloned(),
                _ => None,
            },
            _ => None,
        }
    }

    /// An ordered set's stable uuids in display order.
    fn axis(&self, o: &Oid) -> Vec<Vec<u8>> {
        let Some(Entry::Ordered(order, contents)) = self.obj(o) else { return Vec::new() };
        let map: HashMap<Vec<u8>, Vec<u8>> =
            contents.iter().filter_map(|(k, v)| Some((self.uuid_of(k)?, self.uuid_of(v)?))).collect();
        order.iter().map(|u| map.get(u).cloned().unwrap_or_else(|| u.clone())).collect()
    }
}

fn dict(m: &Msg) -> Vec<(Oid, Oid)> {
    pb::subs(m, 1)
        .iter()
        .map(|e| (pb::sub(e, 1).map(|k| oid(&k)).unwrap_or(Oid::None), pb::sub(e, 2).map(|v| oid(&v)).unwrap_or(Oid::None)))
        .collect()
}

fn parse_graph(blob: &[u8]) -> Result<Graph> {
    let root = pb::parse(&pb::gunzip(blob)?)?;
    let data = pb::sub(&root, 2)
        .and_then(|o| pb::sub(&o, 3))
        .ok_or_else(|| crate::Error::Format("table data has no object graph".into()))?;
    let strs = |n: u32| -> Vec<String> {
        data.fields
            .iter()
            .filter(|f| f.num == n)
            .map(|f| match &f.val {
                kn_format::Val::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
                kn_format::Val::Msg(m) => String::from_utf8_lossy(&m.encode()).into_owned(),
                _ => String::new(),
            })
            .collect()
    };
    let keys = strs(4);
    let types = strs(5);
    let uuids: Vec<Vec<u8>> = data
        .fields
        .iter()
        .filter(|f| f.num == 6)
        .map(|f| match &f.val {
            kn_format::Val::Bytes(b) => b.clone(),
            kn_format::Val::Msg(m) => m.encode(),
            _ => Vec::new(),
        })
        .collect();
    let mut objs = Vec::new();
    for e in pb::subs(&data, 3) {
        let o = if let Some(m) = pb::sub(&e, 13) {
            let ty = pb::uint(&m, 1).and_then(|i| types.get(i as usize).cloned()).unwrap_or_default();
            let mut entries = HashMap::new();
            for me in pb::subs(&m, 3) {
                let k = pb::uint(&me, 1).and_then(|i| keys.get(i as usize).cloned()).unwrap_or_default();
                entries.insert(k, pb::sub(&me, 2).map(|v| oid(&v)).unwrap_or(Oid::None));
            }
            Entry::Map(ty, entries)
        } else if let Some(os) = pb::sub(&e, 16) {
            let ordering = pb::sub(&os, 1).unwrap_or_default();
            let array = pb::sub(&ordering, 1).unwrap_or_default();
            let order = pb::subs(&array, 2).iter().filter_map(|a| pb::bytes(a, 2)).collect();
            let contents = pb::sub(&ordering, 2).map(|d| dict(&d)).unwrap_or_default();
            Entry::Ordered(order, contents)
        } else if let Some(d) = pb::sub(&e, 6) {
            Entry::Dict(dict(&d))
        } else if let Some(t) = pb::sub(&e, 10) {
            Entry::Text(t)
        } else if let Some(r) = pb::sub(&e, 1) {
            Entry::Register(pb::sub(&r, 2).map(|v| oid(&v)).unwrap_or(Oid::None))
        } else {
            Entry::Other
        };
        objs.push(o);
    }
    Ok(Graph { uuids, objs })
}

/// ZMERGEABLEDATA1 of a com.apple.notes.table attachment -> grid.
pub fn decode_table(blob: &[u8]) -> Result<Table> {
    let g = parse_graph(blob)?;
    let root = g
        .objs
        .iter()
        .find_map(|o| match o {
            Entry::Map(t, e) if t == "com.apple.notes.ICTable" => Some(e),
            _ => None,
        })
        .ok_or_else(|| crate::Error::Format("no ICTable object".into()))?;
    let get = |k: &str| root.get(k).cloned().unwrap_or(Oid::None);
    let rows = g.axis(&get("crRows"));
    let cols = g.axis(&get("crColumns"));
    let mut cells: HashMap<(Vec<u8>, Vec<u8>), Body> = HashMap::new();
    if let Some(Entry::Dict(cc)) = g.obj(&get("cellColumns")) {
        for (ck, cv) in cc {
            let Some(cu) = g.uuid_of(ck) else { continue };
            let Some(Entry::Dict(rs)) = g.obj(cv) else { continue };
            for (rk, rv) in rs {
                let Some(ru) = g.uuid_of(rk) else { continue };
                if let Some(Entry::Text(t)) = g.obj(rv) {
                    cells.insert((ru, cu.clone()), cell_body(t));
                }
            }
        }
    }
    let rtl = match g.obj(&get("crTableColumnDirection")) {
        Some(Entry::Register(v)) => match g.obj(v) {
            Some(Entry::Map(_, e)) => matches!(e.get("self"), Some(Oid::Str(s)) if s.contains("RightToLeft")),
            _ => false,
        },
        _ => false,
    };
    let grid = rows
        .iter()
        .map(|r| cols.iter().map(|c| cells.remove(&(r.clone(), c.clone())).unwrap_or_default()).collect())
        .collect();
    Ok(Table { rows: grid, rtl })
}

/// A cell's topotext; an empty cell is an empty body (no paragraphs).
fn cell_body(t: &Msg) -> Body {
    if pb::string(t, 2).unwrap_or_default().is_empty() {
        return Body::default();
    }
    body::decode_topotext(t)
}

// ------------------------------------------------------------------ encoding

const KEYS: [&str; 7] = ["identity", "crColumns", "UUIDIndex", "crRows", "crTableColumnDirection", "self", "cellColumns"];
const K_IDENTITY: u64 = 0;
const K_COLUMNS: u64 = 1;
const K_UUIDINDEX: u64 = 2;
const K_ROWS: u64 = 3;
const K_DIRECTION: u64 = 4;
const K_SELF: u64 = 5;
const K_CELLS: u64 = 6;
const TYPES: [&str; 17] = [
    "com.apple.CRDT.NSNumber",
    "com.apple.CRDT.NSString",
    "com.apple.CRDT.NSUUID",
    "com.apple.CRDT.CRTuple",
    "com.apple.CRDT.CRRegisterMultiValueLeast",
    "com.apple.CRDT.CRRegisterMultiValue",
    "com.apple.CRDT.CRTree",
    "com.apple.CRDT.CRTreeNode",
    "com.apple.CRDT.NSData",
    "com.apple.CRDT.NSDate",
    "com.apple.notes.ICCloudSyncingObjectActivityEvent",
    "com.apple.notes.ICFastSyncSelectionState",
    "com.apple.notes.ICTTAudioRecording",
    "com.apple.notes.ICTTAudioRecording.Fragment",
    "com.apple.notes.ICTTTranscriptSegment",
    "com.apple.notes.CRTable",
    "com.apple.notes.ICTable",
];
const T_NSSTRING: u64 = 1;
const T_NSUUID: u64 = 2;
const T_ICTABLE: u64 = 16;

fn obj_ref(i: usize) -> Msg {
    pb::of_u(&[(6, i as u64)])
}

/// The CRDT timestamp Notes puts on dictionary elements and registers: {1 {1 replica 0, 2 clock 1}}.
fn elem_ts() -> Msg {
    let mut t = pb::new();
    pb::put_m(&mut t, 1, pb::of_u(&[(1, 0), (2, 1)]));
    t
}

fn map_entry(key: u64, val: Msg) -> Msg {
    let mut e = pb::of_u(&[(1, key)]);
    pb::put_m(&mut e, 2, val);
    e
}

fn custom_map(ty: u64, entries: Vec<Msg>) -> Msg {
    let mut m = pb::of_u(&[(1, ty)]);
    for e in entries {
        pb::put_m(&mut m, 3, e);
    }
    let mut o = pb::new();
    pb::put_m(&mut o, 13, m);
    o
}

fn dict_msg(items: &[(usize, usize)]) -> Msg {
    let mut d = pb::new();
    for &(k, v) in items {
        let mut el = pb::new();
        pb::put_m(&mut el, 1, obj_ref(k));
        pb::put_m(&mut el, 2, obj_ref(v));
        pb::put_m(&mut el, 3, elem_ts());
        pb::put_m(&mut d, 1, el);
    }
    d
}

struct Builder {
    uuids: Vec<[u8; 16]>,
    objs: Vec<Msg>,
    /// replica clock: UTF-16 units typed so far (ordering arrays and cells)
    clock: u64,
    replica: [u8; 16],
}

impl Builder {
    fn push(&mut self, m: Msg) -> usize {
        self.objs.push(m);
        self.objs.len() - 1
    }

    fn uuid_obj(&mut self, u: [u8; 16]) -> usize {
        self.uuids.push(u);
        let idx = self.uuids.len() as u64 - 1;
        self.push(custom_map(T_NSUUID, vec![map_entry(K_UUIDINDEX, pb::of_u(&[(2, idx)]))]))
    }

    /// An ordered set of `n` new items; returns its object index and the stable uuid objects.
    fn ordered_set(&mut self, n: usize) -> (usize, Vec<usize>) {
        let at = self.push(pb::new());
        // per item: the stable uuid (used as cellColumns keys) and its position uuid
        let mut stable = Vec::new();
        let mut position = Vec::new();
        let mut pos_uuids = Vec::new();
        for _ in 0..n {
            stable.push(self.uuid_obj(pb::new_uuid_bytes()));
            let pu = pb::new_uuid_bytes();
            pos_uuids.push(pu);
            position.push(self.uuid_obj(pu));
        }
        // ordering.array: topotext of n U+FFFC + one attachment {index, position uuid} per item
        let text: String = std::iter::repeat_n(crate::model::OBJ, n).collect();
        let mut tt = pb::new();
        pb::put_s(&mut tt, 2, &text);
        for s in body::substrings(n as u64, 1, self.clock) {
            pb::put_m(&mut tt, 3, s);
        }
        for _ in 0..n {
            pb::put_m(&mut tt, 5, pb::of_u(&[(1, 1)]));
        }
        self.clock += n as u64;
        let mut array = pb::new();
        pb::put_m(&mut array, 1, tt);
        for (i, pu) in pos_uuids.iter().enumerate() {
            let mut a = pb::of_u(&[(1, i as u64)]);
            pb::put_b(&mut a, 2, pu);
            pb::put_m(&mut array, 2, a);
        }
        let contents: Vec<(usize, usize)> = position.iter().zip(&stable).map(|(&p, &s)| (p, s)).collect();
        let elements: Vec<(usize, usize)> = position.iter().map(|&p| (p, p)).collect();
        let mut ordering = pb::new();
        pb::put_m(&mut ordering, 1, array);
        pb::put_m(&mut ordering, 2, dict_msg(&contents));
        let mut os = pb::new();
        pb::put_m(&mut os, 1, ordering);
        pb::put_m(&mut os, 2, dict_msg(&elements));
        let mut e = pb::new();
        pb::put_m(&mut e, 16, os);
        self.objs[at] = e;
        (at, stable)
    }

    fn cell(&mut self, b: &Body) -> usize {
        let mut t = body::encode_topotext(b, &self.replica, 1, self.clock, false);
        if b.paragraphs.is_empty() || b.text().is_empty() {
            // empty cells carry no runs
            t.fields.retain(|f| f.num != 5);
        }
        self.clock += b.text().encode_utf16().count() as u64;
        let mut e = pb::new();
        pb::put_m(&mut e, 10, t);
        self.push(e)
    }
}

/// Grid -> ZMERGEABLEDATA1 (gzipped) for a new com.apple.notes.table attachment.
pub fn encode_table(t: &Table) -> Vec<u8> {
    let replica = pb::new_uuid_bytes();
    let mut b = Builder { uuids: vec![replica], objs: Vec::new(), clock: 0, replica };
    let ncols = t.cols();
    let root = b.push(pb::new());
    let (cols_at, col_ids) = b.ordered_set(ncols);
    let (rows_at, row_ids) = b.ordered_set(t.rows.len());
    let dir_str = custom_map(
        T_NSSTRING,
        vec![{
            let mut e = pb::of_u(&[(1, K_SELF)]);
            let s = if t.rtl { "CRTableColumnDirectionRightToLeft" } else { "CRTableColumnDirectionLeftToRight" };
            let mut v = pb::new();
            pb::put_s(&mut v, 4, s);
            pb::put_m(&mut e, 2, v);
            e
        }],
    );
    let reg_at = b.push(pb::new());
    let str_at = b.push(dir_str);
    let mut reg = pb::new();
    pb::put_m(&mut reg, 1, pb::of_u(&[(1, 0), (2, 1)]));
    pb::put_m(&mut reg, 2, obj_ref(str_at));
    let mut reg_e = pb::new();
    pb::put_m(&mut reg_e, 1, reg);
    b.objs[reg_at] = reg_e;
    // cellColumns: column -> (row -> cell)
    let cells_at = b.push(pb::new());
    let mut col_items = Vec::new();
    let empty = Body::default();
    for (ci, &col) in col_ids.iter().enumerate() {
        let col_dict_at = b.push(pb::new());
        let mut row_items = Vec::new();
        for (ri, &row) in row_ids.iter().enumerate() {
            let cell = t.rows[ri].get(ci).unwrap_or(&empty);
            let at = b.cell(cell);
            row_items.push((row, at));
        }
        let mut e = pb::new();
        pb::put_m(&mut e, 6, dict_msg(&row_items));
        b.objs[col_dict_at] = e;
        col_items.push((col, col_dict_at));
    }
    let mut e = pb::new();
    pb::put_m(&mut e, 6, dict_msg(&col_items));
    b.objs[cells_at] = e;
    // root ICTable map
    let mut identity = pb::new();
    pb::put_s(&mut identity, 4, "00000000-0000-0000-0000-000000000000");
    b.objs[root] = custom_map(
        T_ICTABLE,
        vec![
            map_entry(K_IDENTITY, identity),
            map_entry(K_COLUMNS, obj_ref(cols_at)),
            map_entry(K_ROWS, obj_ref(rows_at)),
            map_entry(K_DIRECTION, obj_ref(reg_at)),
            map_entry(K_CELLS, obj_ref(cells_at)),
        ],
    );

    let mut data = pb::new();
    let mut head = pb::new();
    pb::put_m(&mut head, 1, pb::of_u(&[(1, 0), (2, 1)]));
    pb::put_m(&mut data, 1, head);
    pb::put_b(&mut data, 2, &[]);
    for o in b.objs {
        pb::put_m(&mut data, 3, o);
    }
    for k in KEYS {
        pb::put_s(&mut data, 4, k);
    }
    for ty in TYPES {
        pb::put_s(&mut data, 5, ty);
    }
    for u in &b.uuids {
        pb::put_b(&mut data, 6, u);
    }
    pb::put_m(&mut data, 7, body::vector_timestamp(&replica, b.clock));
    let mut obj = pb::of_u(&[(1, 0), (2, 0)]);
    pb::put_m(&mut obj, 3, data);
    let mut root_msg = pb::of_u(&[(1, 0)]);
    pb::put_m(&mut root_msg, 2, obj);
    pb::gzip(&root_msg.encode())
}
