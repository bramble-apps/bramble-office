//! The saved window state of a Numbers document. usage: cargo run -p nm-core --example uistate -- file.numbers
fn main() {
    let wb = nm_core::Workbook::open(std::env::args().nth(1).unwrap()).unwrap();
    println!("{:?}", nm_core::ui_state(wb.file()));
    for s in wb.sheets() {
        println!("{} {}", s.id, s.name);
    }
}
