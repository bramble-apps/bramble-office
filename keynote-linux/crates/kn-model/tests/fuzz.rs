//! Random edit sequences on Keynote-made decks (seeded, repeatable), after the Numbers app's
//! stress test: every step succeeds or fails cleanly, undo / redo mixed in, and the result saves
//! and reopens with the same slides, tables readable and every slide's scene building.
//! `KN_FUZZ_SEEDS=200 cargo test --release -p kn-model --test fuzz -- --ignored`

use kn_model::edit::History;
use kn_model::format::FillSpec;
use kn_model::insert::{BasicShape, NewObject};
use kn_model::tablecells::CellInput;
use kn_model::tablerows::{Axis, HeaderKind};
use kn_model::{CellRange, Color, Document, DrawableKind};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { (self.next() % n as u64) as u32 }
    }
}

fn col_name(c: u32) -> String {
    ((b'A' + (c % 26) as u8) as char).to_string()
}

#[test]
#[ignore]
fn fuzz_deck_edits() {
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata"));
    let samples = ["r37/R37-0-before.key", "r37/R37-6-cell-imagefill.key", "ref/R13-merge-0-before.key", "r41/R41-2-applied-saved.key", "r43/R43-effects.key", "05-table.key", "11-two-slides.key", "r33/R33-0-before.key"];
    let seeds: u64 = std::env::var("KN_FUZZ_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(40);
    let out = std::env::temp_dir().join(format!("kd-fuzz-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let mut failures = Vec::new();
    for seed in 1..=seeds {
        let name = samples[(seed as usize) % samples.len()];
        let Ok(mut d) = Document::open(root.join(name)) else { continue };
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) + 1);
        let mut h = History::default();
        let mut log: Vec<String> = Vec::new();
        for _step in 0..40 {
            let slides = d.slides();
            if slides.is_empty() {
                failures.push(format!("seed {seed} {name}: no slides left\n{}", log.join("\n")));
                break;
            }
            let si = rng.below(slides.len() as u32) as usize;
            let slide = slides[si].slide;
            let Some(sc) = d.scene(&slides, si) else {
                failures.push(format!("seed {seed} {name}: slide {si} has no scene\n{}", log.join("\n")));
                break;
            };
            let own: Vec<_> = sc.drawables.iter().filter(|x| !x.from_template).collect();
            let table = own.iter().find_map(|x| match &x.kind {
                DrawableKind::Table(t) => Some((x.id, t.rows, t.cols)),
                _ => None,
            });
            let pick = own.get(rng.below(own.len() as u32) as usize).map(|x| (x.id, x.geom));
            let op = rng.below(20);
            d.begin(&mut h, "fuzz");
            let res: Result<(), String> = match op {
                0 => {
                    log.push(format!("new slide after {si}"));
                    d.new_slide_after(&mut h, si).map(|_| ()).map_err(|e| e.to_string())
                }
                1 => {
                    log.push(format!("duplicate slide {si}"));
                    d.duplicate_slide(&mut h, si).map(|_| ()).map_err(|e| e.to_string())
                }
                2 if slides.len() > 1 => {
                    log.push(format!("delete slide {si}"));
                    d.delete_slides(&mut h, &[si]).map_err(|e| e.to_string())
                }
                3 if slides.len() > 1 => {
                    let to = rng.below(slides.len() as u32) as usize;
                    log.push(format!("move slide {si} -> {to}"));
                    d.move_slide(&mut h, si, to).map_err(|e| e.to_string())
                }
                4 => {
                    let obj = match rng.below(4) {
                        0 => NewObject::TextBox,
                        1 => NewObject::Shape(BasicShape::ALL[rng.below(BasicShape::ALL.len() as u32) as usize]),
                        2 => NewObject::Line { arrow: rng.below(2) == 0 },
                        _ => NewObject::Table { rows: 3 + rng.below(3), cols: 2 + rng.below(3) },
                    };
                    log.push(format!("insert {obj:?}"));
                    d.insert(&mut h, slide, &obj).map(|_| ()).map_err(|e| e.to_string())
                }
                5 => match pick {
                    Some((id, mut g)) => {
                        g.x += rng.below(200) as f32 - 100.0;
                        g.y += rng.below(200) as f32 - 100.0;
                        log.push(format!("move {id}"));
                        d.set_frame(&mut h, id, g);
                        Ok(())
                    }
                    None => Ok(()),
                },
                6 => match pick {
                    Some((id, _)) => {
                        let c = Color { r: rng.below(255) as f32 / 255.0, g: 0.4, b: 0.7, a: 1.0 };
                        log.push(format!("fill {id}"));
                        d.set_fill(&mut h, id, &FillSpec::Color(c));
                        Ok(())
                    }
                    None => Ok(()),
                },
                7 => match pick {
                    Some((id, _)) => {
                        log.push(format!("delete object {id}"));
                        d.delete_drawables(&mut h, slide, &[id]);
                        Ok(())
                    }
                    None => Ok(()),
                },
                8 => {
                    let st = own.iter().find_map(|x| match &x.kind {
                        DrawableKind::Shape(s) => s.text.as_ref().map(|t| t.storage),
                        _ => None,
                    });
                    match st {
                        Some(st) => {
                            log.push(format!("text {st}"));
                            d.set_storage_text(&mut h, st, &format!("Fuzz text {}\nsecond line", rng.below(999)));
                            Ok(())
                        }
                        None => Ok(()),
                    }
                }
                9..=11 => match table {
                    Some((t, rows, cols)) => {
                        let (r, c) = (rng.below(rows), rng.below(cols));
                        let v = match rng.below(3) {
                            0 => CellInput::Number(rng.below(1000) as f64 / 4.0),
                            1 => CellInput::Text(format!("t{}", rng.below(99))),
                            _ => CellInput::Empty,
                        };
                        log.push(format!("cell {r},{c} {v:?}"));
                        d.set_cell(&mut h, t, r, c, &v).map(|_| ()).map_err(|e| e.to_string())
                    }
                    None => Ok(()),
                },
                12 => match table {
                    Some((t, rows, cols)) if rows > 2 => {
                        let (r, c) = (rng.below(rows), rng.below(cols));
                        let src = format!("=SUM(A1:{}{})+1", col_name(rng.below(cols)), rng.below(rows) + 1);
                        log.push(format!("formula {r},{c} {src}"));
                        d.set_formula_text(&mut h, t, r, c, &src).map_err(|e| e.to_string())
                    }
                    _ => Ok(()),
                },
                13 => match table {
                    Some((t, rows, _)) => {
                        let r = rng.below(rows);
                        log.push(format!("insert row {r}"));
                        d.table_insert(&mut h, t, Axis::Row, r, r).map_err(|e| e.to_string())
                    }
                    None => Ok(()),
                },
                14 => match table {
                    Some((t, _, cols)) if cols > 2 => {
                        let c = rng.below(cols);
                        log.push(format!("delete col {c}"));
                        d.table_delete(&mut h, t, Axis::Column, c).map_err(|e| e.to_string())
                    }
                    _ => Ok(()),
                },
                15 => match table {
                    Some((t, rows, cols)) if rows > 2 && cols > 2 => {
                        let (r, c) = (rng.below(rows - 1), rng.below(cols - 1));
                        log.push(format!("merge {r},{c}"));
                        d.merge_cells(&mut h, t, CellRange { row: r, col: c, rows: 2, cols: 2 }).map_err(|e| e.to_string())
                    }
                    _ => Ok(()),
                },
                16 => match table {
                    Some((t, _, cols)) => {
                        let c = rng.below(cols);
                        log.push(format!("sort {c}"));
                        d.sort_table_rows(&mut h, t, c, rng.below(2) == 0).map_err(|e| e.to_string())
                    }
                    None => Ok(()),
                },
                17 => match table {
                    Some((t, _, _)) => {
                        let n = rng.below(3);
                        log.push(format!("header rows {n}"));
                        d.set_table_header_count(&mut h, t, HeaderKind::Rows, n).map_err(|e| e.to_string())
                    }
                    None => Ok(()),
                },
                // a table past 256 rows: its cells go in a second tile
                18 => match table {
                    Some((t, rows, cols)) if rows < 256 => {
                        let target = 258 + rng.below(40);
                        log.push(format!("grow to {target} rows"));
                        let mut res = Ok(());
                        for n in rows..target {
                            let at = rng.below(n + 1);
                            if let Err(e) = d.table_insert(&mut h, t, Axis::Row, at, at.min(n - 1)) {
                                res = Err(e.to_string());
                                break;
                            }
                        }
                        res.and_then(|_| {
                            let (r, c, del) = (256 + rng.below(target - 256), rng.below(cols), rng.below(target));
                            log.push(format!("set {r},{c} then delete row {del}"));
                            d.set_cell(&mut h, t, r, c, &CellInput::Number(f64::from(r))).map_err(|e| e.to_string())?;
                            d.table_delete(&mut h, t, Axis::Row, del).map_err(|e| e.to_string())
                        })
                    }
                    _ => Ok(()),
                },
                _ => Ok(()),
            };
            d.end(&mut h);
            if let Err(e) = res {
                log.push(format!("  -> error: {e}"));
                if std::env::var_os("KN_FUZZ_VERBOSE").is_some() {
                    eprintln!("seed {seed} {name}:\n{}", log.join("\n"));
                }
            }
            if rng.below(6) == 0 {
                d.undo(&mut h);
                log.push("undo".into());
                if rng.below(2) == 0 {
                    d.redo(&mut h);
                    log.push("redo".into());
                }
            }
        }
        let path = out.join(format!("fuzz-{seed:03}.key"));
        let n = d.slides().len();
        match d.save_as(&path).map_err(|e| e.to_string()).and_then(|_| Document::open(&path).map_err(|e| e.to_string())) {
            Ok(back) => {
                let s = back.slides();
                if s.len() != n {
                    failures.push(format!("seed {seed} {name}: {n} slides, {} after reopen\n{}", s.len(), log.join("\n")));
                }
                for i in 0..s.len() {
                    match back.scene(&s, i) {
                        Some(sc) => {
                            for x in &sc.drawables {
                                if let DrawableKind::Table(t) = &x.kind {
                                    if t.cells.len() != (t.rows * t.cols) as usize {
                                        failures.push(format!("seed {seed} {name}: table {} cells {} for {}x{}\n{}", x.id, t.cells.len(), t.rows, t.cols, log.join("\n")));
                                    }
                                }
                            }
                        }
                        None => failures.push(format!("seed {seed} {name}: slide {i} unreadable after reopen\n{}", log.join("\n"))),
                    }
                }
            }
            Err(e) => failures.push(format!("seed {seed} {name}: save/reopen failed: {e}\n{}", log.join("\n"))),
        }
        if std::env::var_os("KN_FUZZ_KEEP").is_none() {
            let _ = std::fs::remove_file(&path);
        }
    }
    if let Some(keep) = std::env::var_os("KN_FUZZ_KEEP") {
        let _ = std::fs::create_dir_all(&keep);
        for e in std::fs::read_dir(&out).unwrap().flatten() {
            let _ = std::fs::rename(e.path(), std::path::Path::new(&keep).join(e.file_name()));
        }
    }
    let _ = std::fs::remove_dir_all(&out);
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n\n"));
}
