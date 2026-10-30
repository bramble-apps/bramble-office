//! cargo run -p pg-core --example docx2pages -- in.docx out.pages : a Word document as Letters
//! builds it, saved as a Pages document (to inspect what the import made).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut p = pg_core::import::import_file(std::path::Path::new(&a[1])).expect("import");
    p.doc.save_as(std::path::Path::new(&a[2])).expect("save");
}
