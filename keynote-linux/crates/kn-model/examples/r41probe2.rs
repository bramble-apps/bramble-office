fn main() {
    for f in ["R41-0-start.key", "R41-1-saved-style.key"] {
        let k = kn_format::KeyFile::open(format!("testdata/r41/{f}")).unwrap();
        let (id, th) = k.first_of("KN.ThemeArchive").unwrap();
        let a = k.archive(id).unwrap();
        let mi = a.info.msgs("message_infos").next().unwrap();
        println!("{f}: theme {id} objrefs has preset? {} ; field_infos {:?}", mi.u64s("object_references").contains(&35211), mi.msgs("field_infos").map(|fi| (fi.msg("path").map(|p| p.u64s("path")), fi.enum_name("type").map(str::to_string), fi.u64s("object_references").len())).collect::<Vec<_>>());
        let t = th.find_super("TSS.ThemeArchive").unwrap();
        for fld in t.fields.iter().filter(|f| f.num == 200) {
            println!("  field 200 val kind: {}", match &fld.val { kn_format::Val::Bytes(b) => format!("bytes {}", b.len()), kn_format::Val::Msg(m) => format!("msg {}", m.type_name()), _ => "other".into() });
        }
    }
}
