//! cargo run -p pg-core --example mkgallery -- out.pages a.png b.png … : a blank document with
//! an image gallery of the pictures (for testing the Gallery pane)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut p = pg_core::Pages::new_blank(false);
    let mut h = pg_core::kn_model::edit::History::default();
    let pics: Vec<(Vec<u8>, String)> = a[2..].iter().map(|f| (std::fs::read(f).unwrap(), std::path::Path::new(f).file_name().unwrap().to_string_lossy().to_string())).collect();
    let (first, name) = pics[0].clone();
    p.doc.begin(&mut h, "Gallery");
    let id = p.insert_image(&mut h, first, &name, pg_core::objects::Placement::Floating { page: 0, x: 100.0, y: 150.0 }, 400.0).unwrap();
    p.doc.make_gallery(&mut h, id, pics[1..].to_vec()).unwrap();
    p.doc.end(&mut h);
    p.save_as(std::path::Path::new(&a[1])).unwrap();
    println!("gallery {id}");
}
