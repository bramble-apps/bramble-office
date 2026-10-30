//! Regenerates data/basic-shapes.bin from the feature catalog (slide 121 holds Keynote's 12
//! basic shapes; #58355 is a line with arrowheads):
//! cargo run -p kn-model --example gen_shapes -- testdata/Keynote-Feature-Catalog.key
use kn_format::KeyFile;

fn main() {
    let path = std::env::args().nth(1).expect("catalog path");
    let k = KeyFile::open(path).unwrap();
    let shapes = [56146, 56174, 56202, 56230, 56265, 56293, 56321, 56349, 56377, 58079, 58123, 58158];
    let mut out = Vec::new();
    for id in shapes {
        let ps = k.obj(id).unwrap().find_super("TSD.ShapeArchive").unwrap().msg("pathsource").unwrap().encode();
        out.extend((ps.len() as u32).to_le_bytes());
        out.extend(ps);
    }
    let style = k.obj(58355).unwrap();
    let end = style.find_super("TSD.ShapeStyleArchive").unwrap().msg("shape_properties").unwrap().msg("head_line_end").unwrap().encode();
    out.extend((end.len() as u32).to_le_bytes());
    out.extend(end);
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/data/basic-shapes.bin"), out).unwrap();
}
