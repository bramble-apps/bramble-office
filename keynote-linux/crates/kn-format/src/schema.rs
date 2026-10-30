//! Apple's protobuf schemas (extracted from Keynote, see research `protos/_schema.json`)
//! and the type registry (numeric message type -> message name).

use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Message,
    Enum,
    String,
    Bytes,
    Bool,
    Float,
    Double,
    Int32,
    Int64,
    UInt32,
    UInt64,
    SInt32,
    SInt64,
    Fixed32,
    Fixed64,
    SFixed32,
    SFixed64,
}

impl Kind {
    fn parse(s: &str) -> Kind {
        match s {
            "message" => Kind::Message,
            "enum" => Kind::Enum,
            "string" => Kind::String,
            "bytes" => Kind::Bytes,
            "bool" => Kind::Bool,
            "float" => Kind::Float,
            "double" => Kind::Double,
            "int32" => Kind::Int32,
            "int64" => Kind::Int64,
            "uint32" => Kind::UInt32,
            "uint64" => Kind::UInt64,
            "sint32" => Kind::SInt32,
            "sint64" => Kind::SInt64,
            "fixed32" => Kind::Fixed32,
            "fixed64" => Kind::Fixed64,
            "sfixed32" => Kind::SFixed32,
            "sfixed64" => Kind::SFixed64,
            other => panic!("unknown schema field type {other}"),
        }
    }

    /// Wire type used for a single (non-packed) value of this kind.
    pub fn wire_type(self) -> u8 {
        match self {
            Kind::Message | Kind::String | Kind::Bytes => 2,
            Kind::Float | Kind::Fixed32 | Kind::SFixed32 => 5,
            Kind::Double | Kind::Fixed64 | Kind::SFixed64 => 1,
            _ => 0,
        }
    }
}

#[derive(Debug)]
pub struct FieldDef {
    pub num: u32,
    pub name: String,
    pub repeated: bool,
    pub kind: Kind,
    /// For message/enum fields: index into `Schema::types`.
    pub type_idx: Option<TypeIdx>,
}

pub type TypeIdx = u32;

#[derive(Debug, Default)]
pub struct TypeDef {
    pub name: String,
    pub fields: HashMap<u32, FieldDef>,
    pub by_name: HashMap<String, u32>,
    /// Enum value -> name (only for enum types).
    pub enum_values: HashMap<i64, String>,
    pub enum_names: HashMap<String, i64>,
}

#[derive(Debug, Default)]
pub struct Schema {
    pub types: Vec<TypeDef>,
    pub by_name: HashMap<String, TypeIdx>,
    /// Registry: message type id -> type index.
    pub registry: HashMap<u32, TypeIdx>,
    pub registry_rev: HashMap<TypeIdx, u32>,
}

static SCHEMA: OnceLock<Schema> = OnceLock::new();
static SOURCE: OnceLock<(&'static str, &'static str)> = OnceLock::new();

/// Use another iWork app's schema and registry (e.g. Numbers: TN.* in place of KN.*) instead of
/// Keynote's. Call before anything reads a file; false if the schema was already loaded.
pub fn use_schema(schema_json: &'static str, registry_json: &'static str) -> bool {
    SCHEMA.get().is_none() && SOURCE.set((schema_json, registry_json)).is_ok()
}

pub fn schema() -> &'static Schema {
    SCHEMA.get_or_init(|| {
        let (s, r) = SOURCE
            .get()
            .copied()
            .unwrap_or((include_str!("../data/schema.json"), include_str!("../data/registry.json")));
        Schema::load(s, r)
    })
}

impl Schema {
    fn load(schema_json: &str, registry_json: &str) -> Schema {
        let raw: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(schema_json).expect("schema.json");
        let mut s = Schema::default();
        // pass 1: names
        for name in raw.keys() {
            let idx = s.types.len() as TypeIdx;
            s.types.push(TypeDef { name: name.clone(), ..Default::default() });
            s.by_name.insert(name.clone(), idx);
        }
        // pass 2: fields / enum values
        for (name, v) in &raw {
            let idx = s.by_name[name] as usize;
            if let Some(fields) = v.get("fields").and_then(|f| f.as_object()) {
                for (num, spec) in fields {
                    let a = spec.as_array().expect("field spec");
                    let num: u32 = num.parse().expect("field number");
                    let fname = a[0].as_str().unwrap().to_string();
                    let kind = Kind::parse(a[2].as_str().unwrap());
                    let tname = a[3].as_str().unwrap_or("");
                    let type_idx = if tname.is_empty() { None } else { s.by_name.get(tname).copied() };
                    let def = FieldDef {
                        num,
                        name: fname.clone(),
                        repeated: a[1].as_str() == Some("repeated"),
                        kind,
                        type_idx,
                    };
                    let t = &mut s.types[idx];
                    t.by_name.insert(fname, num);
                    t.fields.insert(num, def);
                }
            }
            if let Some(vals) = v.get("enum").and_then(|e| e.as_object()) {
                let t = &mut s.types[idx];
                for (num, n) in vals {
                    let num: i64 = num.parse::<i64>().or_else(|_| num.parse::<u64>().map(|v| v as i64)).expect("enum value");
                    let n = n.as_str().unwrap().to_string();
                    t.enum_names.insert(n.clone(), num);
                    t.enum_values.insert(num, n);
                }
            }
        }
        let reg: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(registry_json).expect("registry.json");
        for (id, name) in reg {
            let id: u32 = id.parse().expect("registry id");
            let full = format!(".{}", name.as_str().unwrap());
            if let Some(&idx) = s.by_name.get(&full) {
                s.registry.insert(id, idx);
                s.registry_rev.insert(idx, id);
            }
        }
        s
    }

    pub fn type_idx(&self, name: &str) -> Option<TypeIdx> {
        if let Some(i) = self.by_name.get(name) {
            return Some(*i);
        }
        self.by_name.get(&format!(".{name}")).copied()
    }

    pub fn ty(&self, idx: TypeIdx) -> &TypeDef {
        &self.types[idx as usize]
    }

    /// Type index for a registry message type id.
    pub fn for_type_id(&self, id: u32) -> Option<TypeIdx> {
        self.registry.get(&id).copied()
    }

    pub fn type_id_of(&self, name: &str) -> Option<u32> {
        self.type_idx(name).and_then(|i| self.registry_rev.get(&i).copied())
    }
}

/// Short type name without the leading dot, e.g. "KN.SlideArchive".
pub fn short_name(idx: TypeIdx) -> &'static str {
    schema().ty(idx).name.trim_start_matches('.')
}
