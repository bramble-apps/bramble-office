//! Drawings (com.apple.paper) as strokes.
//!
//! Apple keeps a drawing's strokes in a PaperKit bundle (Accounts/<id>/Paper/Bundles/<attachment
//! UUID>.bundle/Database/data.sqlite3, format section 4.3), only partly decoded so far; Notes shows
//! everyone else the FallbackImage.png. Until the bundle is decoded (research round NO3), drawings
//! made here keep their strokes in the attachment's ZMERGEABLEDATA1 as this protobuf (Notes doesn't
//! read that column for com.apple.paper), and their FallbackImage.png is what Notes shows:
//!
//! ```text
//! 1: "bramble-ink"  2: version 1  3: width f32  4: height f32
//! 5: stroke { 1: id  2: ink  3: rgba [f32;4] packed  4: width f32  5: transform [f32;6] packed
//!             6: points packed f32 x8 (x y t size force azimuth altitude opacity)
//!             7: eraser mask packed f32 x2 (x y), only when there is one }
//! 6: shape { 1: id  2: kind  3: frame [f32;4] packed  4: rotation f32  5: stroke rgba packed
//!            6: fill rgba packed  7: line width f32  8: dash  9: shadow  10: points packed f32 x2
//!            11: sides  12: arrows  13: text  14: font  15: font size f32  16: text rgba packed
//!            17: align  18: magnify f32  19: signature stroke (as 5), repeated }
//! 7: the root record of the PaperKit bundle written with it ([`crate::paperkit`]): when the bundle's
//!    root differs, Notes changed the drawing and the bundle is read instead
//! ```

use kn_format::Msg;

use crate::model::*;
use crate::pb;

const MAGIC: &str = "bramble-ink";

fn ink_code(k: InkKind) -> u64 {
    match k {
        InkKind::Pen => 0,
        InkKind::Pencil => 1,
        InkKind::Marker => 2,
        InkKind::Monoline => 3,
        InkKind::Fountain => 4,
        InkKind::Watercolor => 5,
        InkKind::Crayon => 6,
        InkKind::Reed => 7,
    }
}

fn ink_of(c: u64) -> InkKind {
    match c {
        1 => InkKind::Pencil,
        2 => InkKind::Marker,
        3 => InkKind::Monoline,
        4 => InkKind::Fountain,
        5 => InkKind::Watercolor,
        6 => InkKind::Crayon,
        7 => InkKind::Reed,
        _ => InkKind::Pen,
    }
}

fn shape_code(k: ShapeKind) -> u64 {
    match k {
        ShapeKind::Rect => 0,
        ShapeKind::RoundRect => 1,
        ShapeKind::Oval => 2,
        ShapeKind::Line => 3,
        ShapeKind::Arrow => 4,
        ShapeKind::Bubble => 5,
        ShapeKind::Star => 6,
        ShapeKind::Polygon => 7,
        ShapeKind::Loupe => 8,
        ShapeKind::Text => 9,
        ShapeKind::Signature => 10,
    }
}

fn shape_of(c: u64) -> ShapeKind {
    match c {
        1 => ShapeKind::RoundRect,
        2 => ShapeKind::Oval,
        3 => ShapeKind::Line,
        4 => ShapeKind::Arrow,
        5 => ShapeKind::Bubble,
        6 => ShapeKind::Star,
        7 => ShapeKind::Polygon,
        8 => ShapeKind::Loupe,
        9 => ShapeKind::Text,
        10 => ShapeKind::Signature,
        _ => ShapeKind::Rect,
    }
}

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn unfloats(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn rgba(m: &Msg, n: u32, or: [f32; 4]) -> [f32; 4] {
    unfloats(&pb::bytes(m, n).unwrap_or_default()).try_into().unwrap_or(or)
}

fn pairs(b: &[u8]) -> Vec<[f32; 2]> {
    unfloats(b).chunks_exact(2).map(|p| [p[0], p[1]]).collect()
}

fn encode_stroke(s: &Stroke) -> Msg {
    let mut sm = pb::new();
    pb::put_s(&mut sm, 1, &s.id);
    pb::put_u(&mut sm, 2, ink_code(s.ink));
    pb::put_b(&mut sm, 3, &floats(&s.color));
    pb::put_f32(&mut sm, 4, s.width);
    pb::put_b(&mut sm, 5, &floats(&s.transform));
    let pts: Vec<f32> = s.points.iter().flat_map(|p| [p.x, p.y, p.t, p.size, p.force, p.azimuth, p.altitude, p.opacity]).collect();
    pb::put_b(&mut sm, 6, &floats(&pts));
    if let Some(mask) = &s.mask {
        pb::put_b(&mut sm, 7, &floats(&mask.iter().flatten().copied().collect::<Vec<_>>()));
    }
    sm
}

fn decode_stroke(sm: &Msg) -> Stroke {
    let tr = unfloats(&pb::bytes(sm, 5).unwrap_or_default());
    let pts = unfloats(&pb::bytes(sm, 6).unwrap_or_default());
    Stroke {
        id: pb::string(sm, 1).unwrap_or_default(),
        ink: ink_of(pb::uint(sm, 2).unwrap_or(0)),
        color: rgba(sm, 3, [0.0, 0.0, 0.0, 1.0]),
        width: pb::f32_(sm, 4).unwrap_or(1.0),
        transform: tr.try_into().unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        points: pts
            .chunks_exact(8)
            .map(|p| InkPoint { x: p[0], y: p[1], t: p[2], size: p[3], force: p[4], azimuth: p[5], altitude: p[6], opacity: p[7] })
            .collect(),
        mask: pb::bytes(sm, 7).map(|b| pairs(&b)),
    }
}

fn encode_shape(s: &Shape) -> Msg {
    let mut m = pb::new();
    pb::put_s(&mut m, 1, &s.id);
    pb::put_u(&mut m, 2, shape_code(s.kind));
    pb::put_b(&mut m, 3, &floats(&s.frame));
    pb::put_f32(&mut m, 4, s.rotation);
    pb::put_b(&mut m, 5, &floats(&s.stroke));
    pb::put_b(&mut m, 6, &floats(&s.fill));
    pb::put_f32(&mut m, 7, s.line_width);
    pb::put_u(&mut m, 8, s.dash.into());
    pb::put_u(&mut m, 9, s.shadow.into());
    pb::put_b(&mut m, 10, &floats(&s.points.iter().flatten().copied().collect::<Vec<_>>()));
    pb::put_u(&mut m, 11, s.sides.into());
    pb::put_u(&mut m, 12, s.arrows.into());
    pb::put_s(&mut m, 13, &s.text);
    pb::put_s(&mut m, 14, &s.font);
    pb::put_f32(&mut m, 15, s.font_size);
    pb::put_b(&mut m, 16, &floats(&s.text_color));
    pb::put_u(&mut m, 17, s.align.into());
    pb::put_f32(&mut m, 18, s.magnify);
    for st in &s.ink {
        pb::put_m(&mut m, 19, encode_stroke(st));
    }
    m
}

fn decode_shape(m: &Msg) -> Shape {
    let small = |n| pb::uint(m, n).unwrap_or(0).min(255) as u8;
    Shape {
        id: pb::string(m, 1).unwrap_or_default(),
        kind: shape_of(pb::uint(m, 2).unwrap_or(0)),
        frame: rgba(m, 3, [0.0; 4]),
        rotation: pb::f32_(m, 4).unwrap_or(0.0),
        stroke: rgba(m, 5, [0.0; 4]),
        fill: rgba(m, 6, [0.0; 4]),
        line_width: pb::f32_(m, 7).unwrap_or(0.0),
        dash: small(8),
        shadow: pb::uint(m, 9).unwrap_or(0) != 0,
        points: pairs(&pb::bytes(m, 10).unwrap_or_default()),
        sides: pb::uint(m, 11).unwrap_or(0).min(u32::MAX.into()) as u32,
        arrows: small(12),
        text: pb::string(m, 13).unwrap_or_default(),
        font: pb::string(m, 14).unwrap_or_default(),
        font_size: pb::f32_(m, 15).unwrap_or(0.0),
        text_color: rgba(m, 16, [0.0, 0.0, 0.0, 1.0]),
        align: small(17),
        magnify: pb::f32_(m, 18).unwrap_or(0.0),
        ink: pb::subs(m, 19).iter().map(decode_stroke).collect(),
    }
}

/// Our strokes and shapes, for ZMERGEABLEDATA1.
pub fn encode_ink(d: &Drawing) -> Vec<u8> {
    encode_ink_for(d, None)
}

/// [`encode_ink`], remembering the root of the bundle written with it.
pub fn encode_ink_for(d: &Drawing, bundle_root: Option<&[u8]>) -> Vec<u8> {
    let mut m = pb::new();
    pb::put_s(&mut m, 1, MAGIC);
    pb::put_u(&mut m, 2, 1);
    pb::put_f32(&mut m, 3, d.width);
    pb::put_f32(&mut m, 4, d.height);
    for s in &d.strokes {
        pb::put_m(&mut m, 5, encode_stroke(s));
    }
    for s in &d.shapes {
        pb::put_m(&mut m, 6, encode_shape(s));
    }
    if let Some(r) = bundle_root {
        pb::put_b(&mut m, 7, r);
    }
    m.encode()
}

/// Drawings written by [`encode_ink`] (without shapes when written before they existed); None for
/// anything else (Apple's data, garbage).
pub fn decode_ink(b: &[u8]) -> Option<Drawing> {
    let m: Msg = pb::parse(b).ok()?;
    if pb::string(&m, 1).as_deref() != Some(MAGIC) {
        return None;
    }
    Some(Drawing {
        width: pb::f32_(&m, 3).unwrap_or(0.0),
        height: pb::f32_(&m, 4).unwrap_or(0.0),
        strokes: pb::subs(&m, 5).iter().map(decode_stroke).collect(),
        shapes: pb::subs(&m, 6).iter().map(decode_shape).collect(),
    })
}

/// The bundle root saved by [`encode_ink_for`].
pub fn ink_bundle_root(b: &[u8]) -> Option<Vec<u8>> {
    let m: Msg = pb::parse(b).ok()?;
    (pb::string(&m, 1).as_deref() == Some(MAGIC)).then(|| pb::bytes(&m, 7)).flatten()
}
