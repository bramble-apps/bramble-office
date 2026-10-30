//! Round-11 files for checking in Keynote: T31 slide size 4:3, T32 object links to slides.
//! cargo run --release -p kn-model --example round11 -- outdir
use kn_model::edit::History;
use kn_model::Document;

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let mut h = History::default();
    // T31: 16:9 -> 4:3 (objects recentred)
    let mut d = Document::open(base.join("Keynote-Feature-Catalog.key")).unwrap();
    d.begin(&mut h, "Slide Size");
    d.set_slide_size(&mut h, 1024.0, 768.0);
    d.end(&mut h);
    d.save_as(out.join("T31-slide-size-4x3.key")).unwrap();
    // T32: links on the first slide's objects: next, previous, first, last, slide 3, exit
    let mut d = Document::open(base.join("Keynote-Feature-Catalog.key")).unwrap();
    let s = d.slides();
    let sc = d.scene(&s, 1).unwrap();
    let ids: Vec<_> = sc.drawables.iter().filter(|x| !x.from_template).map(|x| x.id).collect();
    let urls = ["?slide=next", "?slide=previous", "?slide=first", "?slide=last", "?slide=3", "?exit"];
    d.begin(&mut h, "Links");
    for (id, u) in ids.iter().zip(urls) {
        d.set_hyperlink(&mut h, *id, Some(u));
        println!("object #{id} -> {u}");
    }
    d.end(&mut h);
    d.save_as(out.join("T32-slide-links-slide2.key")).unwrap();
    // T33: the first table: 2 header rows, 1 header column, 1 footer, name shown, alternating
    // rows, rows distributed evenly
    let mut d = Document::open(base.join("Keynote-Feature-Catalog.key")).unwrap();
    let table = d.file.objects_of("TST.TableInfoArchive").map(|(i, _)| i).next().unwrap();
    use kn_model::tablerows::HeaderKind;
    d.begin(&mut h, "Table");
    d.set_table_header_count(&mut h, table, HeaderKind::Rows, 2).unwrap();
    d.set_table_header_count(&mut h, table, HeaderKind::Columns, 1).unwrap();
    d.set_table_header_count(&mut h, table, HeaderKind::Footer, 1).unwrap();
    d.set_table_name_shown(&mut h, table, true);
    d.set_table_banded(&mut h, table, true);
    d.distribute_table(&mut h, table, true);
    d.end(&mut h);
    println!("table #{table}");
    d.save_as(out.join("T33-table-options.key")).unwrap();
}
