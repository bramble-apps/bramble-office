//! The look of cells of an imported .xlsx. usage: xlcell file.xlsx sheet(1-based) row col [row col…]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::xlsx::import_xlsx(std::path::Path::new(&a[1])).unwrap();
    let s = wb.sheets().remove(a[2].parse::<usize>().unwrap() - 1);
    let d = wb.table_data(wb.tables(&s)[0].info).unwrap();
    for p in a[3..].chunks(2) {
        let (r, c): (u32, u32) = (p[0].parse().unwrap(), p[1].parse().unwrap());
        let st = &d.cell_styles[(r * d.cols + c) as usize];
        println!("{r},{c} {:?}: {} {} bold {} valign {:?}", d.cells[(r * d.cols + c) as usize], st.text.font_name, st.text.size, st.text.bold, st.valign);
    }
}
