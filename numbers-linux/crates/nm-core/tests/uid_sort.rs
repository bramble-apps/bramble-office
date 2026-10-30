//! kn-format's sort_uid_map gives the row / column uid maps Numbers rebuilds (Mac N3), and
//! leaves Numbers' own maps as they are.
fn maps(k: &nm_core::kn_format::KeyFile) -> Vec<(nm_core::kn_format::ObjId, nm_core::kn_format::Msg)> {
    k.objects_of("TST.ColumnRowUIDMapArchive").map(|(i, m)| (i, m.clone())).collect()
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn uid_maps_match_numbers() {
    nm_core::init();
    let home = std::env::var("HOME").unwrap();
    let ours = format!("{home}/CascadeProjects/numbers-research-src/out-N3/N15-insert-table-and-sheet.numbers");
    let resaved = format!("{home}/CascadeProjects/numbers-research-src/out-N3/resaved/N15-insert-table-and-sheet-resaved-by-Numbers.numbers");
    let (Ok(a), Ok(b)) = (nm_core::kn_format::KeyFile::open(&ours), nm_core::kn_format::KeyFile::open(&resaved)) else { return };
    let bm = maps(&b);
    let mut checked = 0;
    for (id, mut m) in maps(&a) {
        let Some((_, want)) = bm.iter().find(|(i, _)| *i == id) else { continue };
        nm_core::kn_format::tables::sort_uid_map(&mut m);
        for f in ["sorted_row_uids", "sorted_column_uids"] {
            let u = |x: &nm_core::kn_format::Msg| x.msgs(f).map(|u| (u.u64("upper"), u.u64("lower"))).collect::<Vec<_>>();
            assert_eq!(u(&m), u(want), "#{id} {f}");
        }
        for f in ["row_index_for_uid", "row_uid_for_index", "column_index_for_uid", "column_uid_for_index"] {
            assert_eq!(m.u64s(f), want.u64s(f), "#{id} {f}");
        }
        checked += 1;
    }
    assert!(checked > 0);
    // Numbers' own maps are already in this order
    for f in ["29-base-data", "02-second-table", "N-functions"] {
        let k = nm_core::kn_format::KeyFile::open(format!("{}/../../testdata/{f}.numbers", env!("CARGO_MANIFEST_DIR"))).unwrap();
        for (id, m) in maps(&k) {
            let mut s = m.clone();
            nm_core::kn_format::tables::sort_uid_map(&mut s);
            for g in ["row_index_for_uid", "row_uid_for_index", "column_index_for_uid", "column_uid_for_index"] {
                assert_eq!(s.u64s(g), m.u64s(g), "{f} #{id} {g}");
            }
            for g in ["sorted_row_uids", "sorted_column_uids"] {
                let u = |x: &nm_core::kn_format::Msg| x.msgs(g).map(|u| (u.u64("upper"), u.u64("lower"))).collect::<Vec<_>>();
                assert_eq!(u(&s), u(&m), "{f} #{id} {g}");
            }
        }
    }
}
