#![allow(dead_code)]
use std::collections::{HashMap, HashSet};

use kn_format::KeyFile;
use kn_model::Document;

pub fn testdata(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(name)
}

/// Integrity problems Keynote would complain about: references to missing objects and
/// cross-component references the referring component doesn't declare.
pub fn problems(k: &KeyFile) -> HashSet<String> {
    let mut comps: HashMap<String, u64> = HashMap::new();
    let mut declared: HashMap<u64, HashSet<(u64, Option<u64>)>> = HashMap::new();
    for c in k.package_metadata().unwrap().msgs("components") {
        let id = c.u64("identifier").unwrap();
        if let Some(l) = c.str("locator").or(c.str("preferred_locator")) {
            comps.insert(format!("Index/{l}.iwa"), id);
        }
        declared.insert(
            id,
            c.msgs("external_references").map(|x| (x.u64("component_identifier").unwrap_or(0), x.u64("object_identifier"))).collect(),
        );
    }
    let mut out = HashSet::new();
    for (file, a) in k.archives() {
        for i in 0..a.payloads.len() {
            for r in a.object_references(i) {
                let Some(rf) = k.file_of(r) else {
                    out.insert(format!("{file} #{} -> missing #{r}", a.id()));
                    continue;
                };
                let (Some(me), Some(them)) = (comps.get(file), comps.get(rf)) else { continue };
                if me == them {
                    continue;
                }
                let key = (*them, if r == *them { None } else { Some(r) });
                if !declared[me].contains(&key) {
                    out.insert(format!("{file} #{} -> undeclared #{r} in {rf}", a.id()));
                }
            }
            // every reference in the payload must be listed in the archive info (Keynote itself
            // leaves the object container's list empty)
            if a.obj().type_name() == "TSP.ObjectContainer" {
                continue;
            }
            let mut refs = Vec::new();
            a.payloads[i].collect_refs(&mut refs);
            let listed: HashSet<u64> = a.object_references(i).into_iter().collect();
            for r in refs {
                // Keynote never lists the stylesheet itself, nor these back/weak references
                // (seen in its own files: R2-table-values.key, the catalog)
                const UNLISTED: &[(&str, &str)] = &[
                    ("TST.TableInfoArchive", "KN.SlideArchive"),
                    ("TST.CategoryOrderArchive", "TST.TableInfoArchive"),
                    ("TST.SummaryCellVendorArchive", "TST.TableInfoArchive"),
                    ("TST.SummaryModelArchive", "TST.TableInfoArchive"),
                    ("TST.TableModelArchive", "TST.TableStylePresetArchive"),
                    ("TSCE.FormulaOwnerDependenciesArchive", "TST.TableInfoArchive"),
                ];
                if UNLISTED.contains(&(a.obj().type_name(), k.type_name(r))) {
                    continue;
                }
                if !listed.contains(&r) && k.archive(r).is_some() && k.type_name(r) != "TSS.StylesheetArchive" {
                    out.insert(format!("{file} #{} payload ref #{r} not in object_references", a.id()));
                }
            }
        }
    }
    out
}

/// Save to bytes, re-open, check every archive re-reads and no new integrity problems appeared.
pub fn roundtrip(doc: &Document, before: &HashSet<String>) -> Document {
    let bytes = doc.file.to_zip().unwrap();
    let k = KeyFile::from_zip(&bytes).unwrap();
    let after = problems(&k);
    let new: Vec<_> = after.difference(before).collect();
    assert!(new.is_empty(), "new integrity problems: {new:#?}");
    Document::from_file(k)
}
