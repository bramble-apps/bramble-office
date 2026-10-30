//! Print whether each edge of block r0 c0 r1 c1 has a line (first sheet, first table).
//! usage: edges file.numbers r0 c0 r1 c1
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let n: Vec<u32> = a[2..6].iter().map(|x| x.parse().unwrap()).collect();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    let d = wb.table_data(t).unwrap();
    let h = |r: u32, c: u32| d.h_strokes.get((r * d.cols + c) as usize).cloned().flatten().is_some();
    let v = |r: u32, c: u32| d.v_strokes.get((r * (d.cols + 1) + c) as usize).cloned().flatten().is_some();
    println!("top {:?}", (n[1]..=n[3]).map(|c| h(n[0], c)).collect::<Vec<_>>());
    println!("mid {:?}", (n[1]..=n[3]).map(|c| h(n[0] + 1, c)).collect::<Vec<_>>());
    println!("bottom {:?}", (n[1]..=n[3]).map(|c| h(n[2] + 1, c)).collect::<Vec<_>>());
    println!("left {:?} right {:?}", (n[0]..=n[2]).map(|r| v(r, n[1])).collect::<Vec<_>>(), (n[0]..=n[2]).map(|r| v(r, n[3] + 1)).collect::<Vec<_>>());
    println!("inner v {:?}", (n[1] + 1..=n[3]).map(|c| v(n[0], c)).collect::<Vec<_>>());
}
