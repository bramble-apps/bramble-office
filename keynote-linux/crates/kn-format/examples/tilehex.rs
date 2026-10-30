//! Print every non-empty TST.Tile row's cell records in full hex, split per cell.
fn main() {
    let k = kn_format::KeyFile::open(std::env::args().nth(1).unwrap()).unwrap();
    for (id, t) in k.objects_of("TST.Tile") {
        for ri in t.msgs("rowInfos") {
            let (Some(buf), Some(offs)) = (ri.bytes("cell_storage_buffer"), ri.bytes("cell_offsets")) else { continue };
            let offs: Vec<u16> = offs.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let cells: Vec<(usize, usize)> = offs.iter().enumerate().filter(|(_, o)| **o != 0xffff).map(|(c, o)| (c, *o as usize)).collect();
            for (n, (col, start)) in cells.iter().enumerate() {
                let end = cells.get(n + 1).map(|c| c.1).unwrap_or(buf.len());
                let hex: Vec<String> = buf[*start..end].iter().map(|b| format!("{b:02x}")).collect();
                println!("tile {id} row {} col {col}: {}", ri.u32("tile_row_index").unwrap_or(0), hex.join(" "));
            }
        }
    }
    for (id, m) in k.objects_of("TST.TableDataList") {
        let entries: Vec<String> = m.msgs("entries").map(|e| format!("key {} rc {} str {:?} fmt? {}", e.u32("key").unwrap_or(0), e.u32("refcount").unwrap_or(0), e.str("string"), e.has("format"))).collect();
        println!("datalist {id} type {:?}: {:?}", m.enum_name("listType"), entries);
    }
}
