//! Apple's drawings: the PaperKit bundle of a com.apple.paper attachment, read into [`Drawing`] and
//! written from it (research round NO3, `drawings/DRAWINGS-FORMAT.txt`).
//!
//! The bundle is Accounts/<acct>/Paper/Bundles/<attachment id>.bundle/Database/data.sqlite3: a
//! Reference table of CRDT records (Id, Version, RetainCount, ChildRetainCounts, Data) and an
//! (unused here) Assets table. Data is `"crdt"` + u32 LE 6 + a protobuf `{1: value, 6: {1: UUID
//! table (16 bytes each), 2: string table}}`; timestamps and clocks index the UUID table, map keys
//! and interned strings index the string table. Ids: 0xFF the root (`{3: ref "\x01default", 5:
//! version vector, 6: replicas, 8: change id}`), `"\x01default"` the canvas, 0x02 + 16 bytes the rest.
//!
//! ```text
//! canvas   map { bounds: rect, frame: rect, drawing: some(ref), subelements: array of case 5(ref shape) }
//! drawing  map { canvasBounds: rect, strokes: array of ref element }        (id 0x02 + attachment UUID)
//! element  register case 0(ref) -> a stroke wrapper, or a group {6: {2: ref element…}} (eraser pieces)
//! wrapper  map { inherited: ref (ink, or another wrapper), properties: struct {0: seed, 2, 3: ref
//!          stroke data, 4: mask {10: visible outline f32 x,y…, 12: f32 point-index range lo hi…}, 9} }
//! ink      map { ink: some({4: {1: rgba f32, 2: "com.apple.ink.pen", 3: 3, 4: variant, 8: weight}}) }
//! stroke   {10: {1: {1: clock, 2: {1 uuid, 2 created, 3 count, 4 attribute mask, 5 defaults mask,
//!          6 defaults, 7 points, 8: 1}}}}
//! shape    map { frame, fillColor, strokeColor, strokeWidth, type: case 0 rect | case 2 path(points
//!          in the frame, 0..1), opacity, startLineEnd, endLineEnd, secondaryProperties: map { text:
//!          attributed string, emptyStringAttributes, resizeBehavior } }
//! ```
//!
//! Values: 2 f64, 3 interned string, 4 bytes (rects and points are big-endian f64, colours big-endian
//! f32), 5 int, 8 ref, 9 some, 11 dictionary, 12 enum case, 14 struct, 15 f32. Map entries: 1 a
//! register `{1: timestamp, 2: value}`, 4 a nested map, 5 an attributed string, 9 an array (the
//! sequence CRDT of Notes' text: start/content/end substrings over item ids).
//!
//! Stroke points: per-point attributes by mask bit (0 x,y f32 · 1 t f32 · 2 width f32 · 3..8 u16 ·
//! 9, 10 f32 · 11 u16); attributes not stored per point take the defaults blob, in bit order.
//!
//! Written here: strokes (any ink, with masks), rectangles, lines/arrows and text boxes, as a fresh
//! single-replica bundle shaped like the ones Notes 4.13 writes. Other Markup kinds have no sample
//! yet; they stay in our own copy of the drawing ([`crate::paper`]), which also keeps the bundle's
//! root so a bundle changed elsewhere is read instead.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use kn_format::{Msg, Val};

use crate::model::*;
use crate::pb;
use crate::Result;

const ROOT: &[u8] = &[0xff];
const CANVAS: &[u8] = b"\x01default";
/// attribute sizes in a stroke point, by mask bit
const SIZES: [usize; 12] = [8, 4, 4, 2, 2, 2, 2, 2, 2, 4, 4, 2];
/// Notes' arrow line end
const ARROW_END: u64 = 2;

/// The bundle folder of a drawing (Accounts/<acct>/ given).
pub fn bundle_dir(account_dir: &Path, attachment_id: &str) -> PathBuf {
    account_dir.join("Paper").join("Bundles").join(format!("{attachment_id}.bundle"))
}

fn db_path(bundle: &Path) -> PathBuf {
    bundle.join("Database").join("data.sqlite3")
}

// ------------------------------------------------------------------ reading

/// The drawing in a bundle; None when there is no bundle or it can't be read.
pub fn read(bundle: &Path) -> Option<Drawing> {
    decode(&records(bundle)?)
}

/// The root record's data: changes whenever the drawing is saved, here or by Notes.
pub fn fingerprint(bundle: &Path) -> Option<Vec<u8>> {
    // a database with no write-ahead log (ours) is read in place, read-only; else from a copy
    let db = db_path(bundle);
    if db.is_file() && !db.with_file_name("data.sqlite3-wal").exists() {
        let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
        if let Ok(c) = rusqlite::Connection::open_with_flags(&db, flags) {
            if let Ok(d) = c.query_row("SELECT Data FROM Reference WHERE Id = ?1", [ROOT], |r| r.get::<_, Vec<u8>>(0)) {
                return Some(d);
            }
        }
    }
    records(bundle)?.remove(ROOT)
}

/// Every record's Data by id. The database (and its WAL) is copied to a private folder first, so a
/// bundle handed to us is never opened for writing.
fn records(bundle: &Path) -> Option<HashMap<Vec<u8>, Vec<u8>>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let db = db_path(bundle);
    if !db.is_file() {
        return None;
    }
    let tmp = std::env::temp_dir().join(format!("bramble-paper-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).ok()?;
    let copy = tmp.join("data.sqlite3");
    let out = (|| {
        std::fs::copy(&db, &copy).ok()?;
        let wal = db.with_file_name("data.sqlite3-wal");
        if wal.is_file() {
            std::fs::copy(&wal, tmp.join("data.sqlite3-wal")).ok()?;
        }
        let conn = rusqlite::Connection::open(&copy).ok()?;
        let mut st = conn.prepare("SELECT Id, Data FROM Reference").ok()?;
        let rows = st.query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))).ok()?;
        Some(rows.flatten().collect::<HashMap<_, _>>())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    out
}

/// A record's value and string table.
struct Obj {
    top: Msg,
    keys: Vec<String>,
}

fn obj(data: &[u8]) -> Option<Obj> {
    let b = if data.starts_with(b"crdt") && data.len() >= 8 { &data[8..] } else { data };
    let m = pb::parse(b).ok()?;
    let keys = pb::sub(&m, 6)
        .map(|t| t.fields.iter().filter(|f| f.num == 2).filter_map(|f| bytes_of(&f.val)).map(|b| String::from_utf8_lossy(&b).into_owned()).collect())
        .unwrap_or_default();
    Some(Obj { top: pb::sub(&m, 1).unwrap_or_default(), keys })
}

fn bytes_of(v: &Val) -> Option<Vec<u8>> {
    match v {
        Val::Bytes(b) => Some(b.clone()),
        Val::Msg(m) => Some(m.encode()),
        _ => None,
    }
}

/// A map's entries by key (`top` holds the map in field 4).
fn map(top: &Msg, keys: &[String]) -> HashMap<String, Msg> {
    let Some(mp) = pb::sub(top, 4) else { return HashMap::new() };
    pb::uints(&mp, 1).into_iter().zip(pb::subs(&mp, 2)).filter_map(|(k, e)| Some((keys.get(k as usize)?.clone(), e))).collect()
}

impl Obj {
    fn map(&self) -> HashMap<String, Msg> {
        map(&self.top, &self.keys)
    }
}

/// A register entry's value.
fn reg(entry: &Msg) -> Option<Msg> {
    pb::sub(&pb::sub(entry, 1)?, 2)
}

fn ref_id(v: &Msg) -> Option<Vec<u8>> {
    pb::bytes(&pb::sub(&pb::sub(v, 8)?, 5)?, 2)
}

fn some(v: &Msg) -> Option<Msg> {
    pb::sub(&pb::sub(v, 9)?, 1)
}

fn case(v: &Msg) -> Option<(u64, Msg)> {
    let e = pb::sub(v, 12)?;
    Some((pb::uint(&e, 1).unwrap_or(0), pb::sub(&e, 2).unwrap_or_default()))
}

/// A struct's (index, value) fields (no index list: 0, 1, 2…).
fn fields(v: &Msg) -> Vec<(u64, Msg)> {
    let Some(s) = pb::sub(v, 14) else { return Vec::new() };
    let vals = pb::subs(&s, 2);
    if !pb::has(&s, 1) {
        return (0..).zip(vals).collect();
    }
    pb::uints(&s, 1).into_iter().zip(vals).collect()
}

/// Ints (value 5) are zigzag-encoded.
fn unzig(v: u64) -> i64 {
    (v >> 1) as i64 ^ -((v & 1) as i64)
}

fn zig(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// An array entry's item values, in order.
fn items(entry: &Msg) -> Vec<Msg> {
    let Some(head) = pb::sub(entry, 9).and_then(|a| pb::sub(&a, 1)) else { return Vec::new() };
    pb::subs(&head, 4).iter().filter_map(|it| pb::sub(it, 1)).collect()
}

fn be64(b: &[u8]) -> Vec<f64> {
    b.chunks_exact(8).map(|c| f64::from_be_bytes(c.try_into().unwrap())).collect()
}

fn be32(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_be_bytes(c.try_into().unwrap())).collect()
}

fn le32(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

fn rect(v: &Msg) -> Option<[f64; 4]> {
    be64(&pb::bytes(v, 4)?).try_into().ok()
}

fn rgba(v: &Msg) -> Option<[f32; 4]> {
    be32(&pb::bytes(v, 4)?).try_into().ok()
}

fn uuid_of(id: &[u8]) -> String {
    if id.len() == 17 { pb::uuid_str(&id[1..]) } else { id.iter().map(|b| format!("{b:02x}")).collect() }
}

/// The drawing in a bundle's records.
pub fn decode(recs: &HashMap<Vec<u8>, Vec<u8>>) -> Option<Drawing> {
    let root = pb::parse(recs.get(ROOT)?).ok()?;
    let canvas_id = pb::sub(&root, 3).and_then(|r| pb::sub(&r, 5)).and_then(|r| pb::bytes(&r, 2)).unwrap_or_else(|| CANVAS.to_vec());
    let canvas = obj(recs.get(&canvas_id)?)?;
    let cm = canvas.map();
    let size = ["bounds", "frame"].iter().find_map(|k| cm.get(*k).and_then(reg).and_then(|v| rect(&v))).unwrap_or([0.0, 0.0, 768.0, 0.0]);
    let mut d = Drawing { width: size[2] as f32, height: size[3] as f32, ..Default::default() };
    let drawing = cm.get("drawing").and_then(reg).and_then(|v| some(&v)).and_then(|v| ref_id(&v)).and_then(|id| recs.get(&id)).and_then(|b| obj(b));
    if let Some(dr) = drawing {
        for v in dr.map().get("strokes").map(items).unwrap_or_default() {
            if let Some(id) = ref_id(&v) {
                element(recs, &id, &mut d.strokes, 0);
            }
        }
    }
    for v in cm.get("subelements").map(items).unwrap_or_default() {
        let Some(id) = case(&v).and_then(|(_, p)| ref_id(&p)) else { continue };
        if let Some(s) = recs.get(&id).and_then(|b| obj(b)).and_then(|o| shape(&id, &o)) {
            d.shapes.push(s);
        }
    }
    if d.height <= 0.0 {
        d.height = d.strokes.iter().flat_map(|s| s.points.iter().map(|p| p.y + p.size)).fold(0.0, f32::max).max(1.0);
    }
    Some(d)
}

/// A drawing element: a stroke, or a group of them (what the pixel eraser leaves).
fn element(recs: &HashMap<Vec<u8>, Vec<u8>>, id: &[u8], out: &mut Vec<Stroke>, depth: u32) {
    let Some(o) = (depth < 12).then(|| recs.get(id).and_then(|b| obj(b))).flatten() else { return };
    if let Some(next) = pb::sub(&o.top, 1).and_then(|r| pb::sub(&r, 2)).and_then(|v| case(&v)).and_then(|(_, p)| ref_id(&p)) {
        return element(recs, &next, out, depth + 1);
    }
    if let Some(g) = pb::sub(&o.top, 6) {
        for v in pb::subs(&g, 2) {
            if let Some(next) = ref_id(&v) {
                element(recs, &next, out, depth + 1);
            }
        }
        return;
    }
    out.extend(stroke(recs, id, o));
}

/// A stroke wrapper: its properties and ink, following `inherited` up to the ink.
fn stroke(recs: &HashMap<Vec<u8>, Vec<u8>>, id: &[u8], o: Obj) -> Vec<Stroke> {
    let mut props: HashMap<u64, Msg> = HashMap::new();
    let mut ink = None;
    let mut cur = o;
    for _ in 0..12 {
        let m = cur.map();
        if let Some(p) = m.get("properties").and_then(reg) {
            for (i, v) in fields(&p) {
                props.entry(i).or_insert(v);
            }
        }
        if ink.is_none() {
            ink = m.get("ink").and_then(reg).and_then(|v| some(&v)).and_then(|v| pb::sub(&v, 4));
        }
        match m.get("inherited").and_then(reg).and_then(|v| ref_id(&v)).and_then(|r| recs.get(&r)).and_then(|b| obj(b)) {
            Some(n) => cur = n,
            None => break,
        }
    }
    let Some(sm) = props
        .get(&3)
        .and_then(ref_id)
        .and_then(|r| recs.get(&r))
        .and_then(|b| obj(b))
        .and_then(|o| pb::sub(&o.top, 10))
        .and_then(|v| pb::sub(&v, 1))
        .and_then(|v| pb::sub(&v, 2))
    else {
        return Vec::new();
    };
    let ink = ink.unwrap_or_else(pb::new);
    let mask = props.get(&4).and_then(|v| pb::sub(v, 4));
    build_stroke(uuid_of(id), &sm, &ink, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], mask.as_ref())
}

/// A stroke from its point data, ink (`{1: rgba, 2: id, 4: variant}`), transform and pixel-eraser
/// mask (`{10: visible outline, 12: visible point-index ranges}`, split into pieces).
fn build_stroke(id: String, sm: &Msg, ink: &Msg, transform: [f32; 6], mask: Option<&Msg>) -> Vec<Stroke> {
    let Some((mut points, width)) = points(sm) else { return Vec::new() };
    let kind = ink_kind(&pb::string(ink, 2).unwrap_or_default(), pb::string(ink, 4).as_deref());
    let color = pb::sub(ink, 1).map(|c| [1, 2, 3, 4].map(|n| pb::f32_(&c, n).unwrap_or(if n == 4 { 1.0 } else { 0.0 }))).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    for p in &mut points {
        p.size = drawn_width(kind, p.size);
    }
    let width = drawn_width(kind, width);
    let base = Stroke { id, ink: kind, color, width, points, transform, mask: None };
    let Some(mask) = mask else { return vec![base] };
    let outline: Vec<[f32; 2]> = le32(&pb::bytes(mask, 10).unwrap_or_default()).chunks_exact(2).map(|p| [p[0], p[1]]).collect();
    let ranges: Vec<f32> = mask.fields.iter().filter(|f| f.num == 12).filter_map(|f| if let Val::I32(v) = f.val { Some(f32::from_bits(v)) } else { None }).collect();
    let mask = (!outline.is_empty()).then_some(outline);
    if ranges.len() < 2 {
        return vec![Stroke { mask, ..base }];
    }
    let pieces = ranges.len() / 2;
    ranges
        .chunks_exact(2)
        .enumerate()
        .map(|(k, r)| Stroke {
            id: if pieces == 1 { base.id.clone() } else { format!("{}-{k}", base.id) },
            points: slice(&base.points, r[0], r[1]),
            mask: mask.clone(),
            ..base.clone()
        })
        .collect()
}

/// A legacy drawing (com.apple.drawing / .2, PencilKit's PKDrawing data in ZMERGEABLEDATA1:
/// `"wrd"` + protobuf `{4: ink {1: rgba, 2: id, 3: version}…, 5: stroke {1: id, 4: ink index,
/// 5: point data (as PaperKit's), 6: bounds, 7: transform a b c d tx ty, 11: pixel-eraser mask}…,
/// 6: bounds}`; strokes without point data were erased). The canvas size comes from the attachment.
pub fn decode_legacy(b: &[u8]) -> Option<Drawing> {
    let m = pb::parse(b.strip_prefix(b"wrd")?).ok()?;
    let inks = pb::subs(&m, 4);
    let mut d = Drawing::default();
    for (k, s) in pb::subs(&m, 5).iter().enumerate() {
        let Some(sm) = pb::sub(s, 5) else { continue };
        let ink = pb::uint(s, 4).and_then(|i| inks.get(i as usize)).cloned().unwrap_or_default();
        let tr = pb::sub(s, 7).map(|t| [1, 2, 3, 4, 5, 6].map(|n| pb::f32_(&t, n).unwrap_or(0.0))).filter(|t| t[0] != 0.0 || t[1] != 0.0).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let id = pb::bytes(s, 1).filter(|b| b.len() == 16).map(|b| pb::uuid_str(&b)).unwrap_or_else(|| format!("stroke-{k}"));
        d.strokes.extend(build_stroke(id, &sm, &ink, tr, pb::sub(s, 11).as_ref()));
    }
    if let Some(bounds) = pb::sub(&m, 6).map(|r| [1, 2, 3, 4].map(|n| pb::f32_(&r, n).unwrap_or(0.0))) {
        d.width = (bounds[0] + bounds[2]).max(0.0);
        d.height = (bounds[1] + bounds[3]).max(0.0);
    }
    Some(d)
}

/// The points between two fractional point indices.
fn slice(p: &[InkPoint], lo: f32, hi: f32) -> Vec<InkPoint> {
    if p.is_empty() {
        return Vec::new();
    }
    let at = |f: f32| {
        let f = f.clamp(0.0, (p.len() - 1) as f32);
        let (i, t) = (f.floor() as usize, f.fract());
        let (a, b) = (p[i], p[(i + 1).min(p.len() - 1)]);
        let l = |x: f32, y: f32| x + (y - x) * t;
        InkPoint { x: l(a.x, b.x), y: l(a.y, b.y), t: l(a.t, b.t), size: l(a.size, b.size), force: l(a.force, b.force), azimuth: a.azimuth, altitude: a.altitude, opacity: l(a.opacity, b.opacity) }
    };
    let mut out = vec![at(lo)];
    out.extend(p.iter().enumerate().filter(|(i, _)| (*i as f32) > lo && (*i as f32) < hi).map(|(_, q)| *q));
    out.push(at(hi));
    out
}

/// A stroke's points and nominal width.
fn points(sm: &Msg) -> Option<(Vec<InkPoint>, f32)> {
    let n = pb::uint(sm, 3)? as usize;
    let mask = pb::uint(sm, 4).unwrap_or(3);
    let rest = pb::uint(sm, 5).unwrap_or(4095 ^ mask);
    let blob = pb::bytes(sm, 6).unwrap_or_default();
    let data = pb::bytes(sm, 7).unwrap_or_default();
    let stride: usize = (0..12).filter(|b| mask >> b & 1 == 1).map(|b| SIZES[b]).sum();
    if mask & 1 != 1 || data.len() < n * stride {
        return None;
    }
    let mut defaults: [Option<&[u8]>; 12] = [None; 12];
    let mut off = 0;
    for b in (0..12).filter(|b| rest >> b & 1 == 1) {
        if let Some(s) = blob.get(off..off + SIZES[b]) {
            defaults[b] = Some(s);
        }
        off += SIZES[b];
    }
    let f32_at = |s: &[u8]| f32::from_le_bytes(s[..4].try_into().unwrap());
    let u16_at = |s: &[u8]| u16::from_le_bytes([s[0], s[1]]);
    let mut out = Vec::with_capacity(n);
    for chunk in data.chunks_exact(stride.max(1)).take(n) {
        let mut attr: [Option<&[u8]>; 12] = defaults;
        let mut o = 0;
        for b in (0..12).filter(|b| mask >> b & 1 == 1) {
            attr[b] = Some(&chunk[o..o + SIZES[b]]);
            o += SIZES[b];
        }
        let xy = attr[0].unwrap();
        out.push(InkPoint {
            x: f32_at(xy),
            y: f32_at(&xy[4..]),
            t: attr[1].map(f32_at).unwrap_or(0.0),
            size: attr[2].map(f32_at).unwrap_or(1.0),
            force: attr[9].map(f32_at).unwrap_or(1.0),
            azimuth: 0.0,
            altitude: std::f32::consts::FRAC_PI_2,
            // 0x7fff = opaque (the pen's default); pencil and marker store it per point
            opacity: attr[8].map(|s| (f32::from(u16_at(s)) / 32767.0).min(1.0)).unwrap_or(1.0),
        });
    }
    let width = match defaults[2] {
        Some(s) if mask & 4 == 0 => f32_at(s),
        _ => out.iter().map(|p| p.size).fold(0.0, f32::max),
    };
    Some((out, width))
}

fn ink_kind(id: &str, variant: Option<&str>) -> InkKind {
    match id.strip_prefix("com.apple.ink.").unwrap_or(id) {
        "pen" if variant == Some("fixed-width") => InkKind::Monoline,
        "marker" => InkKind::Marker,
        "pencil" => InkKind::Pencil,
        "monoline" => InkKind::Monoline,
        "fountainpen" => InkKind::Fountain,
        "watercolor" => InkKind::Watercolor,
        "crayon" => InkKind::Crayon,
        "reed" => InkKind::Reed,
        _ => InkKind::Pen,
    }
}

/// A Markup object; None for kinds not decoded yet.
fn shape(id: &[u8], o: &Obj) -> Option<Shape> {
    let m = o.map();
    let get = |k: &str| m.get(k).and_then(reg);
    let f = get("frame").and_then(|v| rect(&v))?;
    let opacity = get("opacity").and_then(|v| pb::f64_(&v, 2)).unwrap_or(1.0) as f32;
    let colour = |k: &str| get(k).and_then(|v| some(&v)).and_then(|v| rgba(&v)).map(|c| [c[0], c[1], c[2], c[3] * opacity]).unwrap_or([0.0; 4]);
    let mut s = Shape {
        id: uuid_of(id),
        kind: ShapeKind::Rect,
        frame: f.map(|x| x as f32),
        stroke: colour("strokeColor"),
        fill: colour("fillColor"),
        line_width: get("strokeWidth").and_then(|v| pb::f64_(&v, 2)).unwrap_or(0.0) as f32,
        ..Default::default()
    };
    let (kind, payload) = get("type").and_then(|v| case(&v)).unwrap_or((0, pb::new()));
    match kind {
        0 => {}
        2 => {
            let pts: Vec<[f32; 2]> = fields(&payload)
                .iter()
                .filter_map(|(_, v)| pb::bytes(v, 4))
                .map(|b| be64(&b))
                .filter(|p| p.len() == 2)
                .map(|p| [(f[0] + p[0] * f[2]) as f32, (f[1] + p[1] * f[3]) as f32])
                .collect();
            if pts.len() < 2 {
                return None;
            }
            s.points = vec![pts[0], pts[pts.len() - 1]];
            let end = |k: &str| get(k).and_then(|v| pb::uint(&v, 5)).unwrap_or(0) != 0;
            s.arrows = u8::from(end("startLineEnd")) | u8::from(end("endLineEnd")) << 1;
            s.kind = if s.arrows != 0 { ShapeKind::Arrow } else { ShapeKind::Line };
        }
        _ => return None,
    }
    if let Some(sec) = m.get("secondaryProperties").and_then(|e| pb::sub(e, 4)) {
        let mut wrap = pb::new();
        pb::put_m(&mut wrap, 4, sec);
        if let Some(t) = map(&wrap, &o.keys).get("text").and_then(|e| pb::sub(e, 5)) {
            s.text = pb::string(&t, 1).unwrap_or_default();
            if let Some(run) = pb::subs(&t, 6).first() {
                for a in pb::subs(run, 2) {
                    let (Some(k), Some(v)) = (pb::uint(&a, 1).and_then(|k| o.keys.get(k as usize)), pb::sub(&a, 2)) else { continue };
                    match k.as_str() {
                        "font" => {
                            for (i, fv) in fields(&v) {
                                match i {
                                    0 => s.font = pb::uint(&fv, 3).and_then(|k| o.keys.get(k as usize)).cloned().unwrap_or_default(),
                                    1 => s.font_size = pb::f32_(&fv, 15).unwrap_or(0.0),
                                    _ => {}
                                }
                            }
                        }
                        "color" => s.text_color = rgba(&v).unwrap_or([0.0, 0.0, 0.0, 1.0]),
                        "alignment" => s.align = align_of(pb::uint(&v, 5).unwrap_or(0)),
                        _ => {}
                    }
                }
            }
            if !s.text.is_empty() && kind == 0 {
                s.kind = ShapeKind::Text;
            }
        }
    }
    Some(s)
}

/// Text alignment (zigzag; left 0, centre 1, right 2, justified 3, natural 4, as ours but natural).
fn align_of(raw: u64) -> u8 {
    match unzig(raw) {
        a @ 1..=3 => a as u8,
        _ => 0,
    }
}

fn ns_align(a: u8) -> u64 {
    zig(i64::from(a.min(3)))
}

// ------------------------------------------------------------------ writing

/// One Reference row.
pub struct Record {
    pub id: Vec<u8>,
    pub version: Option<Vec<u8>>,
    pub retain: i64,
    pub children: Vec<u8>,
    pub data: Vec<u8>,
}

/// Whether Notes can show this drawing from a bundle we write (strokes or shapes it knows).
pub fn writable(d: &Drawing) -> bool {
    !d.strokes.is_empty() || d.shapes.iter().any(shape_writable)
}

fn shape_writable(s: &Shape) -> bool {
    match s.kind {
        ShapeKind::Rect | ShapeKind::Text => true,
        ShapeKind::Line | ShapeKind::Arrow => s.points.len() >= 2,
        _ => false,
    }
}

/// Write the drawing as the bundle (replacing what was there); returns its [`fingerprint`].
pub fn write(bundle: &Path, attachment_id: &str, d: &Drawing) -> Result<Vec<u8>> {
    let recs = encode(attachment_id, d);
    let db = db_path(bundle);
    std::fs::create_dir_all(db.parent().unwrap())?;
    let tmp = db.with_file_name("data.sqlite3.new");
    let _ = std::fs::remove_file(&tmp);
    {
        let conn = rusqlite::Connection::open(&tmp)?;
        conn.execute_batch(
            "PRAGMA application_id = 12345; PRAGMA user_version = 65535;
             CREATE TABLE Reference(\nId BLOB PRIMARY KEY NOT NULL,\nVersion BLOB,\nRetainCount INT,\nChildRetainCounts BLOB,\nData BLOB NOT NULL);
             CREATE TABLE Assets(\nId BLOB PRIMARY KEY NOT NULL,\nRetainCount INT,\nData BLOB);",
        )?;
        let tx = conn.unchecked_transaction()?;
        for r in &recs {
            tx.execute("INSERT INTO Reference (Id, Version, RetainCount, ChildRetainCounts, Data) VALUES (?1, ?2, ?3, ?4, ?5)", rusqlite::params![r.id, r.version, r.retain, r.children, r.data])?;
        }
        tx.commit()?;
    }
    for ext in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(db.with_file_name(format!("data.sqlite3{ext}")));
    }
    std::fs::rename(&tmp, &db)?;
    Ok(recs.into_iter().find(|r| r.id == ROOT).map(|r| r.data).unwrap_or_default())
}

/// Remove a drawing's bundle (e.g. when its note is locked: the bundle isn't encrypted).
pub fn remove(bundle: &Path) {
    let _ = std::fs::remove_dir_all(bundle);
}

/// ZMETADATADATA for the attachment (JSON, as Notes writes it).
pub fn metadata(d: &Drawing) -> String {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in d.strokes.iter().flat_map(|s| s.points.iter()) {
        let r = p.size / 2.0;
        (x0, y0, x1, y1) = (x0.min(p.x - r), y0.min(p.y - r), x1.max(p.x + r), y1.max(p.y + r));
    }
    for s in d.shapes.iter().filter(|s| shape_writable(s)) {
        (x0, y0, x1, y1) = (x0.min(s.frame[0]), y0.min(s.frame[1]), x1.max(s.frame[0] + s.frame[2]), y1.max(s.frame[1] + s.frame[3]));
    }
    let new_inks = d.strokes.iter().any(|s| !matches!(s.ink, InkKind::Pen | InkKind::Marker | InkKind::Pencil));
    let mut v = serde_json::json!({ "hasEnhancedCanvasKey": true, "hasMathKey": false, "hasGraphKey": false });
    if x1 >= x0 {
        v["paperContentBoundsOriginXKey"] = serde_json::json!(x0.floor());
        v["paperContentBoundsOriginYKey"] = serde_json::json!(y0.floor());
        v["paperContentBoundsWidthKey"] = serde_json::json!((x1 - x0).ceil());
        v["paperContentBoundsHeightKey"] = serde_json::json!((y1 - y0).ceil());
    }
    if new_inks {
        v["hasNewInksKey"] = serde_json::json!(true);
        v["hasNewInks2023Key"] = serde_json::json!(true);
    }
    v.to_string()
}

/// A record being built: its UUID and string tables.
#[derive(Default)]
struct Rec {
    uuids: Vec<[u8; 16]>,
    keys: Vec<String>,
}

impl Rec {
    fn uuid(&mut self, u: [u8; 16]) -> u64 {
        self.uuids.push(u);
        (self.uuids.len() - 1) as u64
    }

    fn key(&mut self, k: &str) -> u64 {
        match self.keys.iter().position(|x| x == k) {
            Some(i) => i as u64,
            None => {
                self.keys.push(k.to_string());
                (self.keys.len() - 1) as u64
            }
        }
    }

    fn data(self, top: Msg) -> Vec<u8> {
        let mut m = pb::new();
        pb::put_m(&mut m, 1, top);
        let mut t = pb::new();
        pb::put_b(&mut t, 1, &self.uuids.concat());
        for k in &self.keys {
            pb::put_s(&mut t, 2, k);
        }
        pb::put_m(&mut m, 6, t);
        let mut out = b"crdt".to_vec();
        out.extend(6u32.to_le_bytes());
        out.extend(m.encode());
        out
    }
}

fn packed(v: &[u64]) -> Vec<u8> {
    let mut out = Vec::new();
    for &x in v {
        let mut x = x;
        loop {
            let b = (x & 0x7f) as u8;
            x >>= 7;
            if x == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
    }
    out
}

fn wrap(n: u32, inner: Msg) -> Msg {
    let mut m = pb::new();
    pb::put_m(&mut m, n, inner);
    m
}

fn v_bytes(b: &[u8]) -> Msg {
    let mut m = pb::new();
    pb::put_b(&mut m, 4, b);
    m
}

fn v_f64(x: f64) -> Msg {
    let mut m = pb::new();
    pb::put_f64(&mut m, 2, x);
    m
}

fn v_int(x: u64) -> Msg {
    pb::of_u(&[(5, x)])
}

fn v_ref(id: &[u8]) -> Msg {
    let mut r = pb::new();
    pb::put_b(&mut r, 2, id);
    wrap(8, wrap(5, r))
}

fn v_some(inner: Msg) -> Msg {
    wrap(9, wrap(1, inner))
}

fn v_case(c: u64, payload: Msg) -> Msg {
    let mut e = pb::new();
    if c != 0 {
        pb::put_u(&mut e, 1, c);
    }
    pb::put_m(&mut e, 2, payload);
    wrap(12, e)
}

fn v_struct(f: Vec<(u64, Msg)>) -> Msg {
    let mut s = pb::new();
    if !f.is_empty() {
        pb::put_b(&mut s, 1, &packed(&f.iter().map(|x| x.0).collect::<Vec<_>>()));
        for (_, v) in f {
            pb::put_m(&mut s, 2, v);
        }
    }
    let mut m = pb::new();
    pb::put_b(&mut m, 14, &s.encode());
    m
}

fn ts(idx: u64, counter: u64) -> Msg {
    let mut m = pb::new();
    if idx != 0 {
        pb::put_u(&mut m, 1, idx);
    }
    if counter != 0 {
        pb::put_u(&mut m, 2, counter);
    }
    m
}

/// A register map entry.
fn entry(t: Msg, v: Msg) -> Msg {
    let mut r = pb::new();
    pb::put_m(&mut r, 1, t);
    pb::put_m(&mut r, 2, v);
    wrap(1, r)
}

fn map_of(keys: &[u64], entries: Vec<Msg>) -> Msg {
    let mut m = pb::new();
    pb::put_b(&mut m, 1, &packed(keys));
    for e in entries {
        pb::put_m(&mut m, 2, e);
    }
    wrap(4, m)
}

fn clock(idx: u64, ranges: &[u64]) -> Msg {
    let mut m = pb::new();
    pb::put_b(&mut m, 1, &packed(&[idx]));
    let mut r = pb::new();
    pb::put_b(&mut r, 2, &packed(ranges));
    pb::put_m(&mut m, 2, r);
    m
}

fn char_id(idx: u64, clock: u64) -> Msg {
    ts(idx, clock)
}

/// The start, content and end substrings of a fresh sequence of `n` (plus its clocks).
fn substrings(m: &mut Msg, zero: u64, chars: u64, stamps: u64, n: u64) {
    let mut s = pb::new();
    pb::put_m(&mut s, 1, char_id(zero, 0));
    pb::put_b(&mut s, 4, &packed(&[1]));
    pb::put_m(m, 3, s);
    let mut s = pb::new();
    pb::put_m(&mut s, 1, char_id(chars, 0));
    pb::put_u(&mut s, 2, n);
    pb::put_m(&mut s, 3, char_id(stamps, 0));
    pb::put_b(&mut s, 4, &packed(&[2]));
    pb::put_m(m, 3, s);
    pb::put_m(m, 3, wrap(1, char_id(zero, 0xffff_ffff)));
    pb::put_m(m, 4, clock(chars, &[0, n]));
    pb::put_m(m, 5, clock(stamps, &[0, n]));
}

/// An array map entry holding `values` in order.
fn array(rec: &mut Rec, values: Vec<Msg>) -> Msg {
    let zero = rec.uuid([0; 16]);
    let chars = rec.uuid(replica(4, &[]));
    let stamps = rec.uuid(replica(0, &[0x1b, 0x03, 0x48, 0x31]));
    let me = rec.uuid(v7());
    let item = rec.uuid(replica(6, &[]));
    let n = values.len() as u64;
    let mut head = pb::new();
    pb::put_u(&mut head, 1, me);
    pb::put_m(&mut head, 2, clock(item, &[0, n]));
    for (k, v) in values.into_iter().enumerate() {
        let mut it = pb::new();
        pb::put_m(&mut it, 1, v);
        pb::put_m(&mut it, 3, clock(item, &[k as u64, 1]));
        pb::put_m(&mut head, 4, it);
    }
    let mut ord = pb::new();
    substrings(&mut ord, zero, chars, stamps, n);
    let mut a = pb::new();
    pb::put_m(&mut a, 1, head);
    pb::put_m(&mut a, 2, ord);
    for _ in 0..n {
        let u = rec.uuid(v7());
        pb::put_m(&mut a, 3, pb::of_u(&[(1, u)]));
    }
    wrap(9, a)
}

/// A UUIDv7 (Notes' timestamp ids: 48-bit Unix milliseconds first).
fn v7() -> [u8; 16] {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let mut u: [u8; 16] = crate::lock::random();
    u[..6].copy_from_slice(&ms.to_be_bytes()[2..]);
    u[6] = 0x70 | (u[6] & 0x0f);
    u[8] = 0x80 | (u[8] & 0x3f);
    u
}

/// A replica id like Notes': `zeros` leading zero bytes (or `prefix`), then random.
fn replica(zeros: usize, prefix: &[u8]) -> [u8; 16] {
    let mut u: [u8; 16] = crate::lock::random();
    u[..zeros].fill(0);
    u[..prefix.len()].copy_from_slice(prefix);
    u
}

fn new_id(time: bool) -> Vec<u8> {
    let mut id = vec![0x02];
    id.extend(if time { v7() } else { crate::lock::random::<16>() });
    id
}

fn be64s(v: &[f64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

fn be32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

fn le32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn version(replica_id: &[u8; 16], n: u64) -> Vec<u8> {
    let mut m = pb::new();
    pb::put_m(&mut m, 2, wrap(7, clock(0, &[0, n])));
    let mut r = pb::new();
    pb::put_b(&mut r, 1, replica_id);
    pb::put_m(&mut m, 6, r);
    m.encode()
}

fn children(ids: &[&[u8]]) -> Vec<u8> {
    if ids.is_empty() {
        return Vec::new();
    }
    let mut m = pb::new();
    for id in ids {
        let mut c = pb::new();
        pb::put_b(&mut c, 2, id);
        pb::put_m(&mut m, 2, c);
    }
    pb::put_b(&mut m, 3, &packed(&vec![1; ids.len()]));
    m.encode()
}

fn ink_id(k: InkKind) -> (&'static str, Option<&'static str>) {
    match k {
        InkKind::Pen => ("com.apple.ink.pen", None),
        InkKind::Monoline => ("com.apple.ink.pen", Some("fixed-width")),
        InkKind::Marker => ("com.apple.ink.marker", Some("linear")),
        InkKind::Pencil => ("com.apple.ink.pencil", None),
        InkKind::Fountain => ("com.apple.ink.fountainpen", None),
        InkKind::Watercolor => ("com.apple.ink.watercolor", None),
        InkKind::Crayon => ("com.apple.ink.crayon", None),
        InkKind::Reed => ("com.apple.ink.reed", None),
    }
}

/// PencilKit's stored ink size -> the width Notes draws (points), measured on the NO3 samples'
/// fallback pictures: pen piecewise linear through its presets, Monoline x0.4, marker x1.2, other
/// inks as stored until there are samples. Our model's widths are drawn widths.
const PEN_WIDTHS: [(f32, f32); 4] = [(3.084, 2.2), (3.173, 2.35), (3.29, 2.6), (4.0, 4.0)];

fn piecewise(table: &[(f32, f32)], x: f32) -> f32 {
    let i = table.windows(2).position(|w| x <= w[1].0).unwrap_or(table.len() - 2);
    let (a, b) = (table[i], table[i + 1]);
    a.1 + (x - a.0) * (b.1 - a.1) / (b.0 - a.0)
}

pub fn drawn_width(k: InkKind, stored: f32) -> f32 {
    match k {
        InkKind::Pen => piecewise(&PEN_WIDTHS, stored).max(0.05),
        InkKind::Monoline => stored * 0.4,
        InkKind::Marker => stored * 1.2,
        _ => stored,
    }
}

/// The inverse of [`drawn_width`], for writing.
pub fn stored_width(k: InkKind, drawn: f32) -> f32 {
    match k {
        InkKind::Pen => piecewise(&PEN_WIDTHS.map(|(a, b)| (b, a)), drawn).max(0.05),
        InkKind::Monoline => drawn / 0.4,
        InkKind::Marker => drawn / 1.2,
        _ => drawn,
    }
}

/// The picker weight (-1 thinnest … 1 thickest) nearest a pen width (Notes' pen presets).
fn weight(k: InkKind, width: f32) -> f64 {
    if k != InkKind::Pen {
        return if k == InkKind::Monoline { 0.5 } else { 0.0 };
    }
    [(-1.0, 3.084), (-0.5, 3.173), (0.0, 3.290), (1.0, 4.0)].iter().min_by(|a, b| (a.1 - width).abs().total_cmp(&(b.1 - width).abs())).map(|w| w.0).unwrap_or(0.0)
}

/// Seconds since 2001-01-01 (Core Data time).
fn core_now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0) - 978_307_200.0
}

fn stroke_msg(s: &Stroke) -> (Msg, usize) {
    let [a, b, c, d, tx, ty] = s.transform;
    let at = |x: f32, y: f32| (a * x + c * y + tx, b * x + d * y + ty);
    let varies = s.points.iter().any(|p| (p.size - s.width).abs() > 1e-4);
    let translucent = s.points.iter().any(|p| p.opacity < 1.0);
    let mask: u64 = if varies { 7 } else { 3 } | if translucent { 256 } else { 0 };
    let mut pts = Vec::with_capacity(s.points.len() * 16);
    for p in &s.points {
        let (x, y) = at(p.x, p.y);
        pts.extend(x.to_le_bytes());
        pts.extend(y.to_le_bytes());
        pts.extend(p.t.to_le_bytes());
        if varies {
            pts.extend(p.size.to_le_bytes());
        }
        if translucent {
            pts.extend(((p.opacity.clamp(0.0, 1.0) * 32767.0).round() as u16).to_le_bytes());
        }
    }
    let seed: [u8; 2] = crate::lock::random();
    let mut blob = Vec::new();
    if !varies {
        blob.extend(s.width.to_le_bytes());
    }
    blob.extend(1000u16.to_le_bytes());
    blob.extend([0, 0, 0, 0]);
    blob.extend(seed);
    blob.extend([0, 0]);
    if !translucent {
        blob.extend(0x7fffu16.to_le_bytes());
    }
    blob.extend(1.0f32.to_le_bytes());
    blob.extend(0.0f32.to_le_bytes());
    blob.extend([0, 0]);
    let mut m = pb::new();
    pb::put_b(&mut m, 1, &crate::lock::random::<16>());
    pb::put_f64(&mut m, 2, core_now());
    pb::put_u(&mut m, 3, s.points.len() as u64);
    pb::put_u(&mut m, 4, mask);
    pb::put_u(&mut m, 5, 4095 ^ mask);
    pb::put_b(&mut m, 6, &blob);
    pb::put_b(&mut m, 7, &pts);
    pb::put_u(&mut m, 8, 1);
    (m, s.points.len())
}

/// The records of a fresh single-replica bundle for the drawing.
pub fn encode(attachment_id: &str, d: &Drawing) -> Vec<Record> {
    let me: [u8; 16] = replica(7, &[]);
    let rec = |id: Vec<u8>, retain: i64, kids: &[&[u8]], data: Vec<u8>, n: u64| Record { id, version: Some(version(&me, n)), retain, children: children(kids), data };
    let mut out = Vec::new();
    // strokes: element -> wrapper -> (ink, stroke data)
    let mut elements = Vec::new();
    for s in &d.strokes {
        // in PencilKit's units
        let mut s = s.clone();
        s.width = stored_width(s.ink, s.width);
        for p in &mut s.points {
            p.size = stored_width(s.ink, p.size);
        }
        let s = &s;
        let (sm, n) = stroke_msg(s);
        let data_id = new_id(false);
        let mut r = Rec::default();
        let u = r.uuid(crate::lock::random());
        let mut inner = pb::new();
        pb::put_m(&mut inner, 1, clock(u, &[0, n as u64]));
        pb::put_m(&mut inner, 2, sm);
        out.push(rec(data_id.clone(), 1, &[], r.data(wrap(10, wrap(1, inner))), 2));

        let ink_rec = new_id(false);
        let (iid, variant) = ink_id(s.ink);
        let mut im = pb::new();
        let mut col = pb::new();
        for (k, c) in s.color.iter().enumerate() {
            pb::put_f32(&mut col, k as u32 + 1, *c);
        }
        pb::put_m(&mut im, 1, col);
        pb::put_s(&mut im, 2, iid);
        pb::put_u(&mut im, 3, 3);
        if let Some(v) = variant {
            pb::put_s(&mut im, 4, v);
        }
        pb::put_f64(&mut im, 8, weight(s.ink, s.width));
        let mut r = Rec::default();
        r.uuid(v7());
        let k = r.key("ink");
        let top = map_of(&[k], vec![entry(ts(0, 1), v_some(v_bytes(&im.encode())))]);
        out.push(rec(ink_rec.clone(), 1, &[], r.data(top), 2));

        let wrapper = new_id(false);
        let mut r = Rec::default();
        let (ki, kp) = (r.key("inherited"), r.key("properties"));
        r.uuid(v7());
        let up = r.uuid(v7());
        let seed = 0x1000_0000_0000u64 | (u64::from_le_bytes(crate::lock::random()) & 0xfff_ffff_ffff);
        let mut props = vec![(0, pb::of_u(&[(1, seed)])), (2, v_int(0)), (3, v_ref(&data_id))];
        if let Some(mask) = &s.mask {
            let [a, b, c, dd, tx, ty] = s.transform;
            let outline: Vec<f32> = mask.iter().flat_map(|p| [a * p[0] + c * p[1] + tx, b * p[0] + dd * p[1] + ty]).collect();
            let mut mm = pb::new();
            pb::put_b(&mut mm, 10, &le32s(&outline));
            pb::put_f32(&mut mm, 12, 0.0);
            pb::put_f32(&mut mm, 12, s.points.len().saturating_sub(1) as f32);
            props.push((4, v_bytes(&mm.encode())));
        }
        props.push((9, v_int(0)));
        let top = map_of(&[ki, kp], vec![entry(ts(0, 0), v_ref(&ink_rec)), entry(ts(up, 3), v_struct(props))]);
        out.push(rec(wrapper.clone(), 1, &[&ink_rec, &data_id], r.data(top), 2));

        let element_id = new_id(false);
        let mut r = Rec::default();
        r.uuid(v7());
        let mut reg = pb::new();
        pb::put_m(&mut reg, 1, ts(0, 0));
        pb::put_m(&mut reg, 2, v_case(0, v_ref(&wrapper)));
        out.push(rec(element_id.clone(), 1, &[&wrapper], r.data(wrap(1, reg)), 2));
        elements.push(element_id);
    }
    // the drawing: canvas bounds (unset) + the stroke array
    let mut drawing_id = vec![0x02];
    drawing_id.extend(pb::uuid_bytes(attachment_id).unwrap_or_else(|| crate::lock::random()));
    let mut r = Rec::default();
    let (kc, ks) = (r.key("canvasBounds"), r.key("strokes"));
    r.uuid(v7());
    let null = be64s(&[f64::INFINITY, f64::INFINITY, 0.0, 0.0]);
    let strokes = array(&mut r, elements.iter().map(|e| v_ref(e)).collect());
    let top = map_of(&[kc, ks], vec![entry(ts(0, 0), v_bytes(&null)), strokes]);
    let kids: Vec<&[u8]> = elements.iter().map(|e| e.as_slice()).collect();
    out.push(rec(drawing_id.clone(), 1, &kids, r.data(top), 2));
    // shapes
    let mut shapes = Vec::new();
    for s in d.shapes.iter().filter(|s| shape_writable(s)) {
        let id = new_id(true);
        out.push(rec(id.clone(), 1, &[], shape_data(s), 2));
        shapes.push(id);
    }
    // the canvas
    let mut r = Rec::default();
    let mut keys = vec![r.key("bounds"), r.key("frame"), r.key("drawing")];
    let (u0, u1, u2) = (r.uuid(v7()), r.uuid(v7()), r.uuid(v7()));
    let size = be64s(&[0.0, 0.0, f64::from(d.width), f64::from(d.height)]);
    let mut entries = vec![entry(ts(u0, 2), v_bytes(&size)), entry(ts(u1, 1), v_bytes(&size)), entry(ts(u2, 1), v_some(v_ref(&drawing_id)))];
    if !shapes.is_empty() {
        keys.push(r.key("subelements"));
        entries.push(array(&mut r, shapes.iter().map(|id| v_case(5, v_ref(id))).collect()));
    }
    let mut kids: Vec<&[u8]> = vec![&drawing_id];
    kids.extend(shapes.iter().map(|s| s.as_slice()));
    out.push(Record { id: CANVAS.to_vec(), version: Some(version(&me, 3)), retain: 0, children: children(&kids), data: r.data(map_of(&keys, entries)) });
    // the root
    let mut root = pb::new();
    let mut c = pb::new();
    pb::put_b(&mut c, 2, CANVAS);
    pb::put_m(&mut root, 3, wrap(5, c));
    pb::put_m(&mut root, 5, clock(0, &[0, 3]));
    let mut rp = pb::new();
    pb::put_b(&mut rp, 1, &me);
    pb::put_m(&mut root, 6, rp);
    pb::put_b(&mut root, 8, &v7());
    out.push(Record { id: ROOT.to_vec(), version: None, retain: 0, children: Vec::new(), data: root.encode() });
    out
}

fn shape_data(s: &Shape) -> Vec<u8> {
    let mut r = Rec::default();
    let mut keys = Vec::new();
    let mut entries = Vec::new();
    let mut put = |r: &mut Rec, k: &str, v: Msg| {
        keys.push(r.key(k));
        let u = r.uuid(v7());
        entries.push(entry(ts(u, 1), v));
    };
    let mut frame = s.frame.map(f64::from);
    let line = matches!(s.kind, ShapeKind::Line | ShapeKind::Arrow);
    if line {
        let (a, b) = (s.points[0], s.points[s.points.len() - 1]);
        let (x0, y0) = (a[0].min(b[0]), a[1].min(b[1]));
        let (w, h) = ((a[0] - b[0]).abs().max(1.0), (a[1] - b[1]).abs().max(1.0));
        frame = [f64::from(x0), f64::from(y0), f64::from(w), f64::from(h)];
    }
    put(&mut r, "frame", v_bytes(&be64s(&frame)));
    if s.fill[3] > 0.0 && !line {
        put(&mut r, "fillColor", v_some(v_bytes(&be32s(&s.fill))));
    }
    put(&mut r, "strokeColor", v_some(v_bytes(&be32s(&s.stroke))));
    put(&mut r, "strokeWidth", v_f64(f64::from(s.line_width)));
    let kind = if line {
        let norm = |p: [f32; 2]| v_bytes(&be64s(&[(f64::from(p[0]) - frame[0]) / frame[2], (f64::from(p[1]) - frame[1]) / frame[3]]));
        let (a, b) = (s.points[0], s.points[s.points.len() - 1]);
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        v_case(2, v_struct(vec![(0, norm(a)), (1, norm(mid)), (2, norm(b))]))
    } else {
        v_case(0, v_struct(Vec::new()))
    };
    put(&mut r, "type", kind);
    put(&mut r, "opacity", v_f64(1.0));
    if line {
        put(&mut r, "startLineEnd", v_int(if s.arrows & 1 != 0 { ARROW_END } else { 0 }));
        put(&mut r, "endLineEnd", v_int(if s.arrows & 2 != 0 { ARROW_END } else { 0 }));
    }
    if s.kind == ShapeKind::Text || !s.text.is_empty() {
        // the text box's attributed string: one run with the shape's font, colour and alignment
        let font = if s.font.is_empty() { "Helvetica" } else { s.font.as_str() };
        let size = if s.font_size > 0.0 { s.font_size } else { 24.0 };
        let font_v = |r: &mut Rec| {
            let mut sz = pb::new();
            pb::put_f32(&mut sz, 15, size);
            v_struct(vec![(0, pb::of_u(&[(3, r.key(font))])), (1, sz), (2, v_int(0)), (4, v_int(0))])
        };
        let attr = |k: u64, v: Msg| {
            let mut a = pb::new();
            pb::put_u(&mut a, 1, k);
            pb::put_m(&mut a, 2, v);
            a
        };
        let kf = r.key("font");
        let fv = font_v(&mut r);
        let kc = r.key("color");
        let ka = r.key("alignment");
        let kd = r.key("direction");
        let ksec = r.key("secondaryProperties");
        let (kt, ke, kr) = (r.key("text"), r.key("emptyStringAttributes"), r.key("resizeBehavior"));
        let colour = v_bytes(&be32s(&s.text_color));
        let n: u64 = s.text.encode_utf16().count() as u64;
        let mut t = pb::new();
        pb::put_s(&mut t, 1, &s.text);
        let zero = r.uuid([0; 16]);
        let chars = r.uuid(replica(4, &[]));
        let stamps = r.uuid(replica(0, &[0x1b, 0x03, 0x48, 0x31]));
        substrings(&mut t, zero, chars, stamps, n);
        let mut run = pb::new();
        pb::put_u(&mut run, 1, n);
        pb::put_m(&mut run, 2, attr(kf, fv.clone()));
        pb::put_m(&mut run, 2, attr(kc, colour.clone()));
        pb::put_m(&mut run, 2, attr(ka, v_int(ns_align(s.align))));
        pb::put_m(&mut run, 2, attr(kd, v_int(zig(-1))));
        let ru = r.uuid(v7());
        pb::put_m(&mut run, 3, pb::of_u(&[(1, ru), (2, 1)]));
        pb::put_m(&mut t, 6, run);
        let mut dict = pb::new();
        for (k, v) in [(kd, v_int(zig(-1))), (kc, colour), (kf, fv), (ka, v_int(ns_align(s.align)))] {
            pb::put_m(&mut dict, 1, pb::of_u(&[(3, k)]));
            pb::put_m(&mut dict, 1, v);
        }
        let ue = r.uuid(v7());
        let ur = r.uuid(v7());
        let mut sec = pb::new();
        pb::put_b(&mut sec, 1, &packed(&[kt, ke, kr]));
        pb::put_m(&mut sec, 2, wrap(5, t));
        pb::put_m(&mut sec, 2, entry(ts(ue, 1), v_some(wrap(11, dict))));
        pb::put_m(&mut sec, 2, entry(ts(ur, 1), v_int(14)));
        keys.push(ksec);
        entries.push(wrap(4, sec));
    }
    r.data(map_of(&keys, entries))
}

// ------------------------------------------------------------------ PDF documents

/// A PDF kept as a PaperKit document (com.apple.paper.doc.pdf): the original file is an asset of the
/// bundle (Assets.bundle/<base64 SHA-256>, its Assets row Id the raw hash), and the root points at
/// a document record `map { pages: array of case 2(ref page), pdf: some({13: {2: sha}}) }`; each
/// page is `map { bounds: rect, cBounds: some(case 0(rect)), orientation: 1, pdfPage: some(struct
/// {0: page index, 1: {13: {2: sha}}}), annoIDs: (Markup annotations) }`.
pub struct PdfDoc {
    pub pages: Vec<PdfPage>,
    /// the original PDF in the bundle
    pub file: Option<PathBuf>,
}

/// A PDF document bundle's pages and original file.
pub fn read_document(bundle: &Path) -> Option<PdfDoc> {
    let recs = records(bundle)?;
    let root = pb::parse(recs.get(ROOT)?).ok()?;
    let doc_id = pb::sub(&root, 3).and_then(|r| pb::sub(&r, 5)).and_then(|r| pb::bytes(&r, 2))?;
    let doc = obj(recs.get(&doc_id)?)?;
    let m = doc.map();
    let sha = m.get("pdf").and_then(reg).and_then(|v| some(&v)).and_then(|v| pb::sub(&v, 13)).and_then(|v| pb::bytes(&v, 2));
    let mut pages = Vec::new();
    for v in m.get("pages").map(items).unwrap_or_default() {
        let Some(p) = case(&v).and_then(|(_, p)| ref_id(&p)).and_then(|id| recs.get(&id)).and_then(|b| obj(b)) else { continue };
        let pm = p.map();
        let bounds = pm.get("bounds").and_then(reg).and_then(|v| rect(&v)).unwrap_or([0.0, 0.0, 612.0, 792.0]);
        let crop = pm.get("cBounds").and_then(reg).and_then(|v| some(&v)).and_then(|v| case(&v)).and_then(|(_, r)| rect(&r)).unwrap_or(bounds);
        let index = pm
            .get("pdfPage")
            .and_then(reg)
            .and_then(|v| some(&v))
            .and_then(|v| fields(&v).into_iter().find(|(i, _)| *i == 0))
            .and_then(|(_, v)| pb::uint(&v, 5))
            .map(unzig)
            .unwrap_or(pages.len() as i64);
        pages.push(PdfPage { index: index.max(0) as u32, width: bounds[2] as f32, height: bounds[3] as f32, crop: crop.map(|x| x as f32) });
    }
    let file = sha.and_then(|h| asset_file(bundle, &h));
    Some(PdfDoc { pages, file })
}

/// An asset's file: Assets.bundle/<base64 of its hash> ('/' can't be in a file name, so a few
/// spellings are tried; a lone asset is taken as is).
fn asset_file(bundle: &Path, sha: &[u8]) -> Option<PathBuf> {
    let dir = bundle.join("Assets.bundle");
    let b = crate::lock::base64(sha);
    for name in [b.clone(), b.replace('/', "_"), b.replace('/', "-"), b.replace('/', ":")] {
        let p = dir.join(&name);
        if p.is_file() {
            return Some(p);
        }
    }
    let files: Vec<PathBuf> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    (files.len() == 1).then(|| files[0].clone())
}

fn asset_name(sha: &[u8]) -> String {
    crate::lock::base64(sha).replace('/', "_")
}

/// Write a PDF as a PaperKit document bundle (replacing what was there); returns the original's path
/// in it.
pub fn write_document(bundle: &Path, pdf: &[u8], pages: &[PdfPage]) -> Result<PathBuf> {
    use sha2::Digest;
    let sha: Vec<u8> = sha2::Sha256::digest(pdf).to_vec();
    let me: [u8; 16] = replica(6, &[]);
    let rec = |id: Vec<u8>, retain: i64, kids: &[&[u8]], data: Vec<u8>| {
        let mut c = if kids.is_empty() { pb::new() } else { pb::parse(&children(kids)).unwrap_or_default() };
        pb::put_b(&mut c, 4, &sha);
        pb::put_b(&mut c, 5, &packed(&[1]));
        Record { id, version: Some(version(&me, 1)), retain, children: c.encode(), data }
    };
    let asset = || {
        let mut a = pb::new();
        pb::put_b(&mut a, 2, &sha);
        wrap(13, a)
    };
    let mut out = Vec::new();
    let mut page_ids = Vec::new();
    for p in pages {
        let id = new_id(true);
        let mut r = Rec::default();
        let keys = [r.key("bounds"), r.key("cBounds"), r.key("orientation"), r.key("pdfPage"), r.key("annoIDs")];
        let u: Vec<u64> = (0..5).map(|_| r.uuid(v7())).collect();
        let bounds = be64s(&[0.0, 0.0, f64::from(p.width), f64::from(p.height)]);
        let crop = if p.crop[2] > 0.0 && p.crop[3] > 0.0 { be64s(&p.crop.map(f64::from)) } else { bounds.clone() };
        let mut pdf_page = pb::new();
        pb::put_m(&mut pdf_page, 2, v_int(zig(i64::from(p.index))));
        pb::put_m(&mut pdf_page, 2, asset());
        let mut st = pb::new();
        pb::put_b(&mut st, 14, &pdf_page.encode());
        let mut anno = pb::new();
        pb::put_b(&mut anno, 11, &[]);
        let entries = vec![
            entry(ts(u[0], 1), v_bytes(&bounds)),
            entry(ts(u[1], 1), v_some(v_case(0, v_bytes(&crop)))),
            entry(ts(u[2], 1), v_int(zig(1))),
            entry(ts(u[3], 1), v_some(st)),
            entry(ts(u[4], 1), anno),
        ];
        out.push(rec(id.clone(), 1, &[], r.data(map_of(&keys, entries))));
        page_ids.push(id);
    }
    let doc_id = new_id(true);
    let mut r = Rec::default();
    let keys = [r.key("pages"), r.key("pdf")];
    let list = array(&mut r, page_ids.iter().map(|id| v_case(2, v_ref(id))).collect());
    let u = r.uuid(v7());
    let top = map_of(&keys, vec![list, entry(ts(u, 1), v_some(asset()))]);
    let kids: Vec<&[u8]> = page_ids.iter().map(|i| i.as_slice()).collect();
    out.push(rec(doc_id.clone(), 0, &kids, r.data(top)));
    let mut root = pb::new();
    let mut c = pb::new();
    pb::put_b(&mut c, 2, &doc_id);
    pb::put_m(&mut root, 3, wrap(5, c));
    pb::put_m(&mut root, 5, clock(0, &[0, 1]));
    let mut rp = pb::new();
    pb::put_b(&mut rp, 1, &me);
    pb::put_m(&mut root, 6, rp);
    pb::put_b(&mut root, 8, &v7());
    out.push(Record { id: ROOT.to_vec(), version: None, retain: 0, children: Vec::new(), data: root.encode() });

    let _ = std::fs::remove_dir_all(bundle);
    let assets = bundle.join("Assets.bundle");
    std::fs::create_dir_all(&assets)?;
    let file = assets.join(asset_name(&sha));
    std::fs::write(&file, pdf)?;
    let db = db_path(bundle);
    std::fs::create_dir_all(db.parent().unwrap())?;
    let conn = rusqlite::Connection::open(&db)?;
    conn.execute_batch(
        "PRAGMA application_id = 12345; PRAGMA user_version = 65535;
         CREATE TABLE Reference(\nId BLOB PRIMARY KEY NOT NULL,\nVersion BLOB,\nRetainCount INT,\nChildRetainCounts BLOB,\nData BLOB NOT NULL);
         CREATE TABLE Assets(\nId BLOB PRIMARY KEY NOT NULL,\nRetainCount INT,\nData BLOB);",
    )?;
    let tx = conn.unchecked_transaction()?;
    for r in &out {
        tx.execute("INSERT INTO Reference (Id, Version, RetainCount, ChildRetainCounts, Data) VALUES (?1, ?2, ?3, ?4, ?5)", rusqlite::params![r.id, r.version, r.retain, r.children, r.data])?;
    }
    tx.execute("INSERT INTO Assets (Id, RetainCount, Data) VALUES (?1, ?2, NULL)", rusqlite::params![sha, pages.len() as i64 + 1])?;
    tx.commit()?;
    Ok(file)
}

/// The pages of a PDF file (pdfinfo), or one Letter page when they can't be read.
pub fn pdf_pages(pdf: &Path) -> Vec<PdfPage> {
    let out = std::process::Command::new("pdfinfo").args(["-f", "1", "-l", "100000"]).arg(pdf).output();
    let mut pages = Vec::new();
    if let Some(o) = out.ok().filter(|o| o.status.success()) {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            // "Page    3 size: 612 x 792 pts (letter)"
            let Some(rest) = line.strip_prefix("Page") else { continue };
            let Some((n, size)) = rest.split_once(" size:") else { continue };
            let nums: Vec<f32> = size.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            if let (Ok(n), [w, h, ..]) = (n.trim().parse::<u32>(), nums.as_slice()) {
                pages.push(PdfPage { index: n.saturating_sub(1), width: *w, height: *h, crop: [0.0, 0.0, *w, *h] });
            }
        }
    }
    if pages.is_empty() {
        pages.push(PdfPage { index: 0, width: 612.0, height: 792.0, crop: [0.0, 0.0, 612.0, 792.0] });
    }
    pages
}

/// ZMERGEABLEPREFERREDVIEWSIZE: `{1: f64 time, 2: {1: zigzag 0 Large, 1 Small, 2 Medium}}`.
pub fn view_size_of(b: &[u8]) -> ViewSize {
    match pb::parse(b).ok().and_then(|m| pb::sub(&m, 2)).and_then(|v| pb::uint(&v, 1)).map(unzig) {
        Some(1) => ViewSize::Small,
        Some(2) => ViewSize::Medium,
        _ => ViewSize::Large,
    }
}

pub fn view_size_bytes(v: ViewSize) -> Vec<u8> {
    let n = match v {
        ViewSize::Large => 0,
        ViewSize::Small => 1,
        ViewSize::Medium => 2,
    };
    let mut m = pb::new();
    pb::put_f64(&mut m, 1, core_now());
    pb::put_m(&mut m, 2, pb::of_u(&[(1, zig(n))]));
    m.encode()
}
