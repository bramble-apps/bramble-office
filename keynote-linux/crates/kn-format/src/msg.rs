//! Lossless, schema-aware protobuf message.
//!
//! Every message keeps its fields as an ordered list of raw wire values. Fields that the
//! schema says are sub-messages are parsed into [`Msg`] eagerly, but only if re-encoding the
//! parsed form reproduces the original bytes exactly; otherwise they stay raw. Unknown fields
//! survive untouched, so read -> write is byte-identical (verified by tests).

use crate::schema::{schema, short_name, FieldDef, Kind, TypeIdx};
use crate::wire::{packed_varints, read_varint, varint_len, write_varint};
use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    /// Varint (wire type 0).
    Varint(u64),
    /// 64-bit fixed (wire type 1).
    I64(u64),
    /// 32-bit fixed (wire type 5).
    I32(u32),
    /// Length-delimited bytes we did not parse (strings, bytes, packed, unknown).
    Bytes(Vec<u8>),
    /// Parsed sub-message (wire type 2).
    Msg(Msg),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub num: u32,
    pub val: Val,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Msg {
    pub ty: Option<TypeIdx>,
    pub fields: Vec<Field>,
}

/// A TSP.Reference / object identifier.
pub type ObjId = u64;

impl Msg {
    pub fn new(type_name: &str) -> Msg {
        Msg { ty: schema().type_idx(type_name), fields: Vec::new() }
    }

    pub fn untyped() -> Msg {
        Msg::default()
    }

    pub fn parse(buf: &[u8], ty: Option<TypeIdx>) -> Result<Msg, Error> {
        let mut fields = Vec::new();
        let mut pos = 0;
        let tdef = ty.map(|t| schema().ty(t));
        while pos < buf.len() {
            let key = read_varint(buf, &mut pos)?;
            let num = (key >> 3) as u32;
            let wt = (key & 7) as u8;
            if num == 0 {
                return Err(Error::Corrupt("field number 0"));
            }
            let val = match wt {
                0 => Val::Varint(read_varint(buf, &mut pos)?),
                1 => {
                    let b = buf.get(pos..pos + 8).ok_or(Error::Corrupt("truncated fixed64"))?;
                    pos += 8;
                    Val::I64(u64::from_le_bytes(b.try_into().unwrap()))
                }
                5 => {
                    let b = buf.get(pos..pos + 4).ok_or(Error::Corrupt("truncated fixed32"))?;
                    pos += 4;
                    Val::I32(u32::from_le_bytes(b.try_into().unwrap()))
                }
                2 => {
                    let len = read_varint(buf, &mut pos)? as usize;
                    let b = buf.get(pos..pos + len).ok_or(Error::Corrupt("truncated bytes"))?;
                    pos += len;
                    let sub_ty = tdef
                        .and_then(|t| t.fields.get(&num))
                        .filter(|f| f.kind == Kind::Message)
                        .and_then(|f| f.type_idx);
                    match sub_ty {
                        Some(st) => match Msg::parse(b, Some(st)) {
                            Ok(m) if m.encoded_len() == b.len() && m.encode() == b => Val::Msg(m),
                            _ => Val::Bytes(b.to_vec()),
                        },
                        None => Val::Bytes(b.to_vec()),
                    }
                }
                _ => return Err(Error::Corrupt("bad wire type")),
            };
            fields.push(Field { num, val });
        }
        Ok(Msg { ty, fields })
    }

    pub fn encoded_len(&self) -> usize {
        self.fields
            .iter()
            .map(|f| {
                varint_len(u64::from(f.num) << 3)
                    + match &f.val {
                        Val::Varint(v) => varint_len(*v),
                        Val::I64(_) => 8,
                        Val::I32(_) => 4,
                        Val::Bytes(b) => varint_len(b.len() as u64) + b.len(),
                        Val::Msg(m) => {
                            let n = m.encoded_len();
                            varint_len(n as u64) + n
                        }
                    }
            })
            .sum()
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.encoded_len());
        self.encode_into(&mut out);
        out
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) {
        for f in &self.fields {
            let wt: u64 = match f.val {
                Val::Varint(_) => 0,
                Val::I64(_) => 1,
                Val::I32(_) => 5,
                Val::Bytes(_) | Val::Msg(_) => 2,
            };
            write_varint(out, (u64::from(f.num) << 3) | wt);
            match &f.val {
                Val::Varint(v) => write_varint(out, *v),
                Val::I64(v) => out.extend_from_slice(&v.to_le_bytes()),
                Val::I32(v) => out.extend_from_slice(&v.to_le_bytes()),
                Val::Bytes(b) => {
                    write_varint(out, b.len() as u64);
                    out.extend_from_slice(b);
                }
                Val::Msg(m) => {
                    write_varint(out, m.encoded_len() as u64);
                    m.encode_into(out);
                }
            }
        }
    }

    // ------------------------------------------------------------------ schema helpers

    pub fn type_name(&self) -> &'static str {
        self.ty.map(short_name).unwrap_or("")
    }

    pub fn field_def(&self, name: &str) -> Option<&'static FieldDef> {
        let t = schema().ty(self.ty?);
        let num = t.by_name.get(name)?;
        t.fields.get(num)
    }

    fn def(&self, name: &str) -> &'static FieldDef {
        self.field_def(name)
            .unwrap_or_else(|| panic!("{} has no field {name:?}", self.type_name()))
    }

    pub fn has_field(&self, name: &str) -> bool {
        self.field_def(name).is_some()
    }

    fn vals(&self, num: u32) -> impl Iterator<Item = &Val> {
        self.fields.iter().filter(move |f| f.num == num).map(|f| &f.val)
    }

    pub fn has(&self, name: &str) -> bool {
        match self.field_def(name) {
            Some(d) => self.fields.iter().any(|f| f.num == d.num),
            None => false,
        }
    }

    // ------------------------------------------------------------------ getters

    /// Last occurrence of a sub-message field (protobuf "last wins" for singular fields).
    pub fn msg(&self, name: &str) -> Option<&Msg> {
        let d = self.field_def(name)?;
        self.vals(d.num).filter_map(|v| if let Val::Msg(m) = v { Some(m) } else { None }).last()
    }

    pub fn msgs(&self, name: &str) -> impl Iterator<Item = &Msg> {
        let num = self.field_def(name).map(|d| d.num).unwrap_or(0);
        self.vals(num).filter_map(|v| if let Val::Msg(m) = v { Some(m) } else { None })
    }

    /// Follow a dotted path of sub-message fields: `msg_path("super.geometry")`.
    pub fn msg_path(&self, path: &str) -> Option<&Msg> {
        let mut m = self;
        for p in path.split('.') {
            m = m.msg(p)?;
        }
        Some(m)
    }

    /// Walk the `super` chain until a message of the given type is found (or self).
    pub fn find_super(&self, type_name: &str) -> Option<&Msg> {
        let mut m = self;
        loop {
            if m.type_name() == type_name {
                return Some(m);
            }
            m = m.msg("super")?;
        }
    }

    pub fn find_super_mut(&mut self, type_name: &str) -> Option<&mut Msg> {
        if self.type_name() == type_name {
            return Some(self);
        }
        self.msg_mut("super")?.find_super_mut(type_name)
    }

    fn scalar_raw(&self, name: &str) -> Option<&Val> {
        let d = self.field_def(name)?;
        self.vals(d.num).last()
    }

    pub fn u64(&self, name: &str) -> Option<u64> {
        match self.scalar_raw(name)? {
            Val::Varint(v) | Val::I64(v) => Some(*v),
            Val::I32(v) => Some(u64::from(*v)),
            _ => None,
        }
    }

    pub fn i64(&self, name: &str) -> Option<i64> {
        let d = self.field_def(name)?;
        let v = self.u64(name)?;
        Some(match d.kind {
            Kind::SInt32 | Kind::SInt64 => ((v >> 1) as i64) ^ -((v & 1) as i64),
            Kind::SFixed32 | Kind::Int32 => v as u32 as i32 as i64,
            _ => v as i64,
        })
    }

    pub fn u32(&self, name: &str) -> Option<u32> {
        self.u64(name).map(|v| v as u32)
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        self.u64(name).map(|v| v != 0)
    }

    pub fn f32(&self, name: &str) -> Option<f32> {
        match self.scalar_raw(name)? {
            Val::I32(v) => Some(f32::from_bits(*v)),
            Val::I64(v) => Some(f64::from_bits(*v) as f32),
            _ => None,
        }
    }

    pub fn f64(&self, name: &str) -> Option<f64> {
        match self.scalar_raw(name)? {
            Val::I64(v) => Some(f64::from_bits(*v)),
            Val::I32(v) => Some(f64::from(f32::from_bits(*v))),
            _ => None,
        }
    }

    pub fn bytes(&self, name: &str) -> Option<&[u8]> {
        match self.scalar_raw(name)? {
            Val::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn str(&self, name: &str) -> Option<&str> {
        self.bytes(name).and_then(|b| std::str::from_utf8(b).ok())
    }

    pub fn strs(&self, name: &str) -> Vec<&str> {
        let num = self.field_def(name).map(|d| d.num).unwrap_or(0);
        self.vals(num)
            .filter_map(|v| if let Val::Bytes(b) = v { std::str::from_utf8(b).ok() } else { None })
            .collect()
    }

    /// Enum value as its schema name (e.g. "kKindTitlePlaceholder").
    pub fn enum_name(&self, name: &str) -> Option<&'static str> {
        let d = self.field_def(name)?;
        let v = self.i64(name)?;
        let t = schema().ty(d.type_idx?);
        t.enum_values.get(&v).map(|s| s.as_str())
    }

    /// Repeated varint-type values, packed or not.
    pub fn u64s(&self, name: &str) -> Vec<u64> {
        let Some(d) = self.field_def(name) else { return Vec::new() };
        let mut out = Vec::new();
        for v in self.vals(d.num) {
            match v {
                Val::Varint(x) | Val::I64(x) => out.push(*x),
                Val::I32(x) => out.push(u64::from(*x)),
                Val::Bytes(b) => match d.kind {
                    Kind::Float | Kind::Fixed32 | Kind::SFixed32 => {
                        out.extend(b.chunks_exact(4).map(|c| u64::from(u32::from_le_bytes(c.try_into().unwrap()))))
                    }
                    Kind::Double | Kind::Fixed64 | Kind::SFixed64 => {
                        out.extend(b.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())))
                    }
                    _ => out.extend(packed_varints(b).unwrap_or_default()),
                },
                Val::Msg(_) => {}
            }
        }
        out
    }

    pub fn f32s(&self, name: &str) -> Vec<f32> {
        let kind = self.field_def(name).map(|d| d.kind);
        self.u64s(name)
            .into_iter()
            .map(|v| if kind == Some(Kind::Double) { f64::from_bits(v) as f32 } else { f32::from_bits(v as u32) })
            .collect()
    }

    /// Identifier of a TSP.Reference sub-message field.
    pub fn ref_id(&self, name: &str) -> Option<ObjId> {
        self.msg(name)?.u64("identifier")
    }

    pub fn ref_ids(&self, name: &str) -> Vec<ObjId> {
        self.msgs(name).filter_map(|m| m.u64("identifier")).collect()
    }

    // ------------------------------------------------------------------ setters

    fn encode_scalar(d: &FieldDef, v: Scalar) -> Val {
        match (d.kind, v) {
            (Kind::Float, Scalar::F(x)) => Val::I32((x as f32).to_bits()),
            (Kind::Double, Scalar::F(x)) => Val::I64(x.to_bits()),
            (Kind::Fixed32 | Kind::SFixed32, Scalar::U(x)) => Val::I32(x as u32),
            (Kind::Fixed64 | Kind::SFixed64, Scalar::U(x)) => Val::I64(x),
            (Kind::SInt32 | Kind::SInt64, Scalar::I(x)) => Val::Varint(((x << 1) ^ (x >> 63)) as u64),
            (_, Scalar::I(x)) => Val::Varint(x as u64),
            (_, Scalar::U(x)) => Val::Varint(x),
            (_, Scalar::F(x)) => Val::Varint(x as u64),
            (_, Scalar::B(b)) => Val::Bytes(b),
        }
    }

    /// Replace all occurrences of a field with `vals`, keeping the position of the first one.
    fn replace(&mut self, num: u32, vals: Vec<Val>) {
        let first = self.fields.iter().position(|f| f.num == num).unwrap_or(self.fields.len());
        let tail: Vec<Field> = self.fields.drain(first..).filter(|f| f.num != num).collect();
        self.fields.extend(vals.into_iter().map(|val| Field { num, val }));
        self.fields.extend(tail);
    }

    fn set_scalar(&mut self, name: &str, v: Scalar) {
        let d = self.def(name);
        let val = Self::encode_scalar(d, v);
        self.replace(d.num, vec![val]);
    }

    pub fn set_u64(&mut self, name: &str, v: u64) {
        self.set_scalar(name, Scalar::U(v));
    }
    pub fn set_i64(&mut self, name: &str, v: i64) {
        self.set_scalar(name, Scalar::I(v));
    }
    pub fn set_bool(&mut self, name: &str, v: bool) {
        self.set_scalar(name, Scalar::U(u64::from(v)));
    }
    pub fn set_f32(&mut self, name: &str, v: f32) {
        self.set_scalar(name, Scalar::F(f64::from(v)));
    }
    pub fn set_f64(&mut self, name: &str, v: f64) {
        self.set_scalar(name, Scalar::F(v));
    }
    pub fn set_str(&mut self, name: &str, v: &str) {
        self.set_scalar(name, Scalar::B(v.as_bytes().to_vec()));
    }
    pub fn set_bytes(&mut self, name: &str, v: &[u8]) {
        self.set_scalar(name, Scalar::B(v.to_vec()));
    }
    pub fn set_enum(&mut self, name: &str, value_name: &str) {
        let d = self.def(name);
        let t = schema().ty(d.type_idx.expect("enum type"));
        let v = *t.enum_names.get(value_name).unwrap_or_else(|| panic!("no enum value {value_name}"));
        self.set_scalar(name, Scalar::I(v));
    }
    /// Set a repeated string field.
    pub fn set_strs(&mut self, name: &str, vals: &[&str]) {
        let d = self.def(name);
        self.replace(d.num, vals.iter().map(|s| Val::Bytes(s.as_bytes().to_vec())).collect());
    }
    /// Set a repeated varint field, one value per field occurrence (not packed).
    pub fn set_u64s(&mut self, name: &str, vals: &[u64]) {
        let d = self.def(name);
        self.replace(d.num, vals.iter().map(|v| Val::Varint(*v)).collect());
    }
    /// Set a repeated varint field, written packed (as Keynote does for object_references).
    pub fn set_u64s_packed(&mut self, name: &str, vals: &[u64]) {
        let d = self.def(name);
        if vals.is_empty() {
            self.replace(d.num, Vec::new());
            return;
        }
        let mut b = Vec::new();
        for v in vals {
            write_varint(&mut b, *v);
        }
        self.replace(d.num, vec![Val::Bytes(b)]);
    }

    pub fn remove(&mut self, name: &str) {
        if let Some(d) = self.field_def(name) {
            let num = d.num;
            self.fields.retain(|f| f.num != num);
        }
    }

    /// Mutable access to the last occurrence of a sub-message field.
    pub fn msg_mut(&mut self, name: &str) -> Option<&mut Msg> {
        let num = self.field_def(name)?.num;
        self.fields
            .iter_mut()
            .rev()
            .filter(|f| f.num == num)
            .find_map(|f| if let Val::Msg(m) = &mut f.val { Some(m) } else { None })
    }

    /// Like [`msg_mut`] but creates an empty sub-message if missing.
    pub fn msg_mut_or_create(&mut self, name: &str) -> &mut Msg {
        if self.msg(name).is_none() {
            let d = self.def(name);
            let m = Msg { ty: d.type_idx, fields: Vec::new() };
            self.fields.push(Field { num: d.num, val: Val::Msg(m) });
        }
        self.msg_mut(name).unwrap()
    }

    pub fn msg_path_mut(&mut self, path: &str) -> Option<&mut Msg> {
        let mut m = self;
        for p in path.split('.') {
            m = m.msg_mut(p)?;
        }
        Some(m)
    }

    pub fn msgs_mut(&mut self, name: &str) -> impl Iterator<Item = &mut Msg> {
        let num = self.field_def(name).map(|d| d.num).unwrap_or(0);
        self.fields
            .iter_mut()
            .filter(move |f| f.num == num)
            .filter_map(|f| if let Val::Msg(m) = &mut f.val { Some(m) } else { None })
    }

    /// Append an empty sub-message (of the field's type) to a repeated field and return it.
    pub fn push_new(&mut self, name: &str) -> &mut Msg {
        let d = self.def(name);
        self.fields.push(Field { num: d.num, val: Val::Msg(Msg { ty: d.type_idx, fields: Vec::new() }) });
        match &mut self.fields.last_mut().unwrap().val {
            Val::Msg(m) => m,
            _ => unreachable!(),
        }
    }

    /// Append a sub-message to a (repeated) field.
    pub fn push_msg(&mut self, name: &str, m: Msg) {
        let num = self.def(name).num;
        self.fields.push(Field { num, val: Val::Msg(m) });
    }

    /// Insert a sub-message as the `index`-th occurrence of a repeated field.
    pub fn insert_msg(&mut self, name: &str, index: usize, m: Msg) {
        let num = self.def(name).num;
        let positions: Vec<usize> =
            self.fields.iter().enumerate().filter(|(_, f)| f.num == num).map(|(i, _)| i).collect();
        let at = if index < positions.len() {
            positions[index]
        } else {
            positions.last().map(|p| p + 1).unwrap_or(self.fields.len())
        };
        self.fields.insert(at, Field { num, val: Val::Msg(m) });
    }

    /// Keep only the sub-messages of a repeated field for which `keep` returns true.
    pub fn retain_msgs(&mut self, name: &str, mut keep: impl FnMut(&Msg) -> bool) {
        let Some(d) = self.field_def(name) else { return };
        let num = d.num;
        self.fields.retain(|f| f.num != num || matches!(&f.val, Val::Msg(m) if keep(m)));
    }

    pub fn count(&self, name: &str) -> usize {
        self.field_def(name).map(|d| self.fields.iter().filter(|f| f.num == d.num).count()).unwrap_or(0)
    }

    /// Set a TSP.Reference field (creates it when missing).
    pub fn set_ref(&mut self, name: &str, id: ObjId) {
        self.msg_mut_or_create(name).set_u64("identifier", id);
    }

    pub fn push_ref(&mut self, name: &str, id: ObjId) {
        self.push_msg(name, reference(id));
    }

    /// Visit every TSP.Reference identifier in this message tree (mutable), via the schema.
    pub fn for_each_ref_mut(&mut self, f: &mut impl FnMut(&mut u64)) {
        let is_ref = self.type_name() == "TSP.Reference";
        for fld in &mut self.fields {
            match &mut fld.val {
                Val::Msg(m) => m.for_each_ref_mut(f),
                Val::Varint(v) if is_ref && fld.num == 1 => f(v),
                _ => {}
            }
        }
    }

    /// Visit every TSP.DataReference identifier (pictures, movies) in this message tree.
    pub fn for_each_data_ref_mut(&mut self, f: &mut impl FnMut(&mut u64)) {
        let is_ref = self.type_name() == "TSP.DataReference";
        for fld in &mut self.fields {
            match &mut fld.val {
                Val::Msg(m) => m.for_each_data_ref_mut(f),
                Val::Varint(v) if is_ref && fld.num == 1 => f(v),
                _ => {}
            }
        }
    }

    pub fn collect_refs(&self, out: &mut Vec<ObjId>) {
        let is_ref = self.type_name() == "TSP.Reference";
        for fld in &self.fields {
            match &fld.val {
                Val::Msg(m) => m.collect_refs(out),
                Val::Varint(v) if is_ref && fld.num == 1 => out.push(*v),
                _ => {}
            }
        }
    }
}

enum Scalar {
    U(u64),
    I(i64),
    F(f64),
    B(Vec<u8>),
}

pub fn reference(id: ObjId) -> Msg {
    let mut m = Msg::new("TSP.Reference");
    m.set_u64("identifier", id);
    m
}
