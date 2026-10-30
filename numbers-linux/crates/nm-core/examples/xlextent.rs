//! Each worksheet's extent: cells with values, and cells with a visible style (fill / border).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).unwrap();
    let (sheets, styles) = nm_core::xlsx::read_xlsx_full(&bytes).unwrap();
    for s in &sheets {
        let data = (s.rows.len(), s.rows.iter().map(Vec::len).max().unwrap_or(0));
        let vis: Vec<(u32, u32)> = s.layout.cell_style.iter().filter(|(_, i)| styles.get(**i as usize).is_some_and(|st| st.fill.is_some() || st.borders.iter().any(Option::is_some))).map(|(rc, _)| *rc).collect();
        let mr = vis.iter().map(|x| x.0).max();
        let mc = vis.iter().map(|x| x.1).max();
        println!("{}: data {:?}, styled max row {:?} col {:?} ({} styled cells)", s.name, data, mr, mc, vis.len());
    }
}
