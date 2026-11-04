//! Field-number access on kn-format's lossless [`Msg`] for Notes' protobufs, which ship without
//! descriptors (lite runtime), so everything here goes by field number. Also gzip and UUID helpers.

use std::io::{Read, Write};

use kn_format::{Msg, Val};
use kn_format::msg::Field;

pub type Result<T> = std::result::Result<T, crate::Error>;

pub fn parse(b: &[u8]) -> Result<Msg> {
    Msg::parse(b, None).map_err(|e| crate::Error::Format(e.to_string()))
}

fn last(m: &Msg, n: u32) -> Option<&Val> {
    m.fields.iter().rev().find(|f| f.num == n).map(|f| &f.val)
}

/// The last occurrence of field `n` parsed as a message.
pub fn sub(m: &Msg, n: u32) -> Option<Msg> {
    match last(m, n)? {
        Val::Bytes(b) => parse(b).ok(),
        Val::Msg(m) => Some(m.clone()),
        _ => None,
    }
}

/// Every occurrence of field `n` parsed as a message (unparseable ones skipped).
pub fn subs(m: &Msg, n: u32) -> Vec<Msg> {
    m.fields
        .iter()
        .filter(|f| f.num == n)
        .filter_map(|f| match &f.val {
            Val::Bytes(b) => parse(b).ok(),
            Val::Msg(m) => Some(m.clone()),
            _ => None,
        })
        .collect()
}

pub fn uint(m: &Msg, n: u32) -> Option<u64> {
    match last(m, n)? {
        Val::Varint(v) | Val::I64(v) => Some(*v),
        Val::I32(v) => Some(u64::from(*v)),
        _ => None,
    }
}

/// Every varint of field `n`, packed or not.
pub fn uints(m: &Msg, n: u32) -> Vec<u64> {
    let mut out = Vec::new();
    for f in m.fields.iter().filter(|f| f.num == n) {
        match &f.val {
            Val::Varint(v) => out.push(*v),
            Val::Bytes(b) => out.extend(kn_format::wire::packed_varints(b).unwrap_or_default()),
            _ => {}
        }
    }
    out
}

pub fn int(m: &Msg, n: u32) -> Option<i64> {
    uint(m, n).map(|v| v as i64)
}

pub fn f32_(m: &Msg, n: u32) -> Option<f32> {
    match last(m, n)? {
        Val::I32(v) => Some(f32::from_bits(*v)),
        _ => None,
    }
}

pub fn f64_(m: &Msg, n: u32) -> Option<f64> {
    match last(m, n)? {
        Val::I64(v) => Some(f64::from_bits(*v)),
        _ => None,
    }
}

pub fn bytes(m: &Msg, n: u32) -> Option<Vec<u8>> {
    match last(m, n)? {
        Val::Bytes(b) => Some(b.clone()),
        Val::Msg(m) => Some(m.encode()),
        _ => None,
    }
}

pub fn string(m: &Msg, n: u32) -> Option<String> {
    bytes(m, n).map(|b| String::from_utf8_lossy(&b).into_owned())
}

pub fn has(m: &Msg, n: u32) -> bool {
    m.fields.iter().any(|f| f.num == n)
}

// ------------------------------------------------------------------ building

pub fn new() -> Msg {
    Msg::untyped()
}

pub fn put_u(m: &mut Msg, n: u32, v: u64) {
    m.fields.push(Field { num: n, val: Val::Varint(v) });
}

pub fn put_i(m: &mut Msg, n: u32, v: i64) {
    put_u(m, n, v as u64);
}

pub fn put_f32(m: &mut Msg, n: u32, v: f32) {
    m.fields.push(Field { num: n, val: Val::I32(v.to_bits()) });
}

pub fn put_f64(m: &mut Msg, n: u32, v: f64) {
    m.fields.push(Field { num: n, val: Val::I64(v.to_bits()) });
}

pub fn put_b(m: &mut Msg, n: u32, v: &[u8]) {
    m.fields.push(Field { num: n, val: Val::Bytes(v.to_vec()) });
}

pub fn put_s(m: &mut Msg, n: u32, v: &str) {
    put_b(m, n, v.as_bytes());
}

pub fn put_m(m: &mut Msg, n: u32, v: Msg) {
    m.fields.push(Field { num: n, val: Val::Msg(v) });
}

/// A message from (field, varint) pairs.
pub fn of_u(pairs: &[(u32, u64)]) -> Msg {
    let mut m = new();
    for &(n, v) in pairs {
        put_u(&mut m, n, v);
    }
    m
}

// ------------------------------------------------------------------ gzip

pub fn is_gzip(b: &[u8]) -> bool {
    b.starts_with(&[0x1f, 0x8b])
}

/// Gunzip when gzipped, else return as is.
pub fn gunzip(b: &[u8]) -> Result<Vec<u8>> {
    if !is_gzip(b) {
        return Ok(b.to_vec());
    }
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(b).read_to_end(&mut out)?;
    Ok(out)
}

pub fn gzip(b: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(b).expect("gzip into memory");
    e.finish().expect("gzip into memory")
}

// ------------------------------------------------------------------ uuids

/// 16 bytes -> "8FE99C4C-FEA9-4C9C-AF76-F8A7B48FC5DC".
pub fn uuid_str(b: &[u8]) -> String {
    match uuid::Uuid::from_slice(b) {
        Ok(u) => u.hyphenated().encode_upper(&mut uuid::Uuid::encode_buffer()).to_string(),
        Err(_) => b.iter().map(|x| format!("{x:02X}")).collect(),
    }
}

/// "8FE99C4C-…" (any case, dashes optional) -> 16 bytes; None when it isn't a UUID.
pub fn uuid_bytes(s: &str) -> Option<[u8; 16]> {
    uuid::Uuid::parse_str(s).ok().map(|u| *u.as_bytes())
}

pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().hyphenated().encode_upper(&mut uuid::Uuid::encode_buffer()).to_string()
}

pub fn new_uuid_bytes() -> [u8; 16] {
    *uuid::Uuid::new_v4().as_bytes()
}
