//! r41probe: what Keynote registered for a saved table style (R41-0 vs R41-1).
fn main() {
    let dir = "testdata/r41";
    let a = kn_format::KeyFile::open(format!("{dir}/R41-0-start.key")).unwrap();
    let b = kn_format::KeyFile::open(format!("{dir}/R41-1-saved-style.key")).unwrap();
    let ss = |k: &kn_format::KeyFile| k.obj(32834).unwrap().clone();
    let (sa, sb) = (ss(&a), ss(&b));
    let styles = |m: &kn_format::Msg| m.ref_ids("styles");
    let new: Vec<u64> = styles(&sb).into_iter().filter(|i| !styles(&sa).contains(i)).collect();
    println!("styles added: {new:?}");
    let ids = |m: &kn_format::Msg| m.msgs("identifier_to_style_map").map(|e| (e.str("identifier").unwrap_or_default().to_string(), e.ref_id("style"))).collect::<Vec<_>>();
    let (ia, ib) = (ids(&sa), ids(&sb));
    println!("identifier map added: {:?}", ib.iter().filter(|e| !ia.contains(e)).collect::<Vec<_>>());
    let p2c = |m: &kn_format::Msg| m.msgs("parent_to_children_style_map").map(|e| (e.ref_id("parent"), e.ref_ids("children"))).collect::<Vec<_>>();
    let (pa, pb) = (p2c(&sa), p2c(&sb));
    println!("parent map changed: {:?}", pb.iter().filter(|e| !pa.contains(e)).collect::<Vec<_>>());
    for id in [35192u64, 35193, 35203, 35208] {
        let m = b.obj(id).unwrap();
        let sup = m.find_super("TSS.StyleArchive");
        println!("{id} {} parent={:?} variation={:?} ident={:?} fields={:?}", b.type_name(id), sup.and_then(|s| s.ref_id("parent")), sup.and_then(|s| s.bool("is_variation")), sup.and_then(|s| s.str("style_identifier")), m.fields.iter().map(|f| f.num).collect::<Vec<_>>());
    }
    // theme presets field
    let th = b.first_of("KN.ThemeArchive").unwrap().1.clone();
    println!("theme super fields: {:?}", th.find_super("TSS.ThemeArchive").map(|t| t.fields.iter().map(|f| f.num).collect::<Vec<_>>()));
    // archive infos of new objects
    for id in [35192u64, 35211, 35250] {
        let ar = b.archive(id).unwrap();
        println!("{id} file {:?} objrefs {:?}", b.file_of(id), ar.info.msgs("message_infos").next().map(|mi| mi.u64s("object_references")));
    }
    let net = b.obj(35250).unwrap();
    println!("network fields {:?}", net.fields.iter().map(|f| f.num).collect::<Vec<_>>());
}
