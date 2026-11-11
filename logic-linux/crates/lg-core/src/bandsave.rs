//! Saving a GarageBand (iOS) .band: its `projectData` is an NSKeyedArchiver plist whose "DfDocument
//! logic model" holds a Logic ProjectData (garageband-analysis), so Save writes that ProjectData
//! the way it writes a .logicx's (save.rs, writeback.rs) and puts the bytes back in the archive;
//! `Output/metadata.plist` follows the tempo, key and signature. Nothing is written when nothing
//! changed. The first save keeps the original archive beside it (projectData.before-bramble).

use std::path::Path;

use crate::project::{self, Project};
use crate::save::Saved;

/// The archive object holding the model's bytes: (object index, inside a dictionary's NS.data).
fn model_slot(archive: &plist::Value) -> Option<(usize, bool)> {
    let d = archive.as_dictionary()?;
    let objects = d.get("$objects")?.as_array()?;
    let uid = |v: &plist::Value| -> Option<usize> {
        match v {
            plist::Value::Uid(u) => Some(u.get() as usize),
            plist::Value::Dictionary(d) if d.len() == 1 => d.get("CF$UID")?.as_unsigned_integer().map(|n| n as usize),
            _ => None,
        }
    };
    let top = d.get("$top")?.as_dictionary()?;
    let i = uid(top.get("DfDocument logic model")?)?;
    match objects.get(i)? {
        plist::Value::Data(_) => Some((i, false)),
        plist::Value::Dictionary(m) => {
            let v = m.get("NS.data")?;
            match uid(v) {
                Some(j) => Some((j, false)),
                None => matches!(v, plist::Value::Data(_)).then_some((i, true)),
            }
        }
        _ => None,
    }
}

/// The Logic ProjectData inside a .band's projectData archive.
pub fn model_bytes(pkg: &Path) -> Option<Vec<u8>> {
    project::band_logic_model(&plist::Value::from_file(pkg.join("projectData")).ok()?)
}

/// Writes `p` (opened from the .band at `pkg`) back into it.
pub fn save(p: &Project, pkg: &Path) -> Result<Saved, String> {
    let file = pkg.join("projectData");
    let mut archive = plist::Value::from_file(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let original = project::band_logic_model(&archive).ok_or("no Logic model in the projectData")?;
    // GarageBand's summary gave the tempo and key the app shows; unchanged, they stay as the
    // model has them
    let opened = project::open_band(pkg).map_err(|e| e.to_string())?;
    let inner = project::read_project_data(&original).map_err(|e| e.to_string())?;
    let mut q = p.clone();
    if (q.tempo - opened.tempo).abs() < 1e-9 {
        q.tempo = inner.tempo;
        if q.tempo_map.len() == 1 {
            q.tempo_map[0].1 = inner.tempo;
        }
    }
    if crate::events::key_byte(&q.key) == crate::events::key_byte(&opened.key) {
        q.key = inner.key.clone();
    }
    let saved = crate::save::save(&q, &original).map_err(|e| e.to_string())?;
    if saved.data != original {
        let (i, inline) = model_slot(&archive).ok_or("the Logic model's place in the archive")?;
        let objects = archive.as_dictionary_mut().and_then(|d| d.get_mut("$objects")).and_then(|o| o.as_array_mut()).ok_or("no $objects")?;
        if inline {
            if let Some(plist::Value::Dictionary(m)) = objects.get_mut(i) {
                m.insert("NS.data".into(), plist::Value::Data(saved.data.clone()));
            }
        } else {
            objects[i] = plist::Value::Data(saved.data.clone());
        }
        let backup = pkg.join("projectData.before-bramble");
        if !backup.exists() {
            std::fs::copy(&file, &backup).map_err(|e| e.to_string())?;
        }
        archive.to_file_xml(&file).map_err(|e| e.to_string())?;
    }
    update_metadata(pkg, p, &opened);
    Ok(saved)
}

/// Output/metadata.plist: the summary keys GarageBand shows (tempo, key, signature), when they
/// changed.
fn update_metadata(pkg: &Path, p: &Project, opened: &Project) {
    let path = pkg.join("Output/metadata.plist");
    let Ok(plist::Value::Dictionary(mut d)) = plist::Value::from_file(&path) else { return };
    let k = |s: &str| format!("com_apple_garageband_metadata_{s}");
    let mut changed = false;
    if (p.tempo - opened.tempo).abs() > 1e-9 && d.contains_key(&k("songTempo")) {
        d.insert(k("songTempo"), plist::Value::Real(p.tempo));
        changed = true;
    }
    if crate::events::key_byte(&p.key) != crate::events::key_byte(&opened.key) {
        let mut parts = p.key.split_whitespace();
        if let (Some(root), Some(g)) = (parts.next(), parts.next()) {
            d.insert(k("songKey"), plist::Value::String(root.replace('\u{266d}', "b").replace('\u{266f}', "#")));
            d.insert(k("songGender"), plist::Value::String(g.to_string()));
            changed = true;
        }
    }
    if p.signature != opened.signature {
        d.insert(k("songSignatureNominator"), plist::Value::Integer((p.signature.0 as i64).into()));
        d.insert(k("songSignatureDeNominator"), plist::Value::Integer((p.signature.1 as i64).into()));
        changed = true;
    }
    if changed {
        let _ = plist::Value::Dictionary(d).to_file_xml(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(data: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut o = String::new();
        for c in data.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for k in 0..4 {
                o.push(if k <= c.len() { T[(n >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
            }
        }
        o
    }

    /// A .band made around a sample's ProjectData (the research's E02, when present): notes,
    /// tempo and key edited, saved, opened again; unchanged, the archive isn't touched.
    #[test]
    fn a_band_saves_its_logic_model() {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata-private/sample-files");
        let main = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("CascadeProjects/bramble-office/logic-linux/testdata-private/sample-files");
        let Some(data) = [here, main].iter().map(|d| d.join("E02-4-notes.logicx/Alternatives/000/ProjectData")).find_map(|p| std::fs::read(p).ok()) else { return };
        let dir = std::env::temp_dir().join(format!("lg-band-{}", std::process::id()));
        let pkg = dir.join("Song.band");
        std::fs::create_dir_all(pkg.join("Output")).unwrap();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>$archiver</key><string>NSKeyedArchiver</string>
<key>$objects</key><array><string>$null</string><dict><key>NS.data</key><dict><key>CF$UID</key><integer>2</integer></dict></dict><data>{}</data></array>
<key>$top</key><dict><key>DfDocument logic model</key><dict><key>CF$UID</key><integer>1</integer></dict></dict>
<key>$version</key><integer>100000</integer>
</dict></plist>"#,
            b64(&data)
        );
        std::fs::write(pkg.join("projectData"), &xml).unwrap();
        let meta = r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>com_apple_garageband_metadata_songTempo</key><real>96</real><key>com_apple_garageband_metadata_songKey</key><string>C</string><key>com_apple_garageband_metadata_songGender</key><string>major</string></dict></plist>"#;
        std::fs::write(pkg.join("Output/metadata.plist"), meta).unwrap();
        let p = project::open(&pkg).unwrap();
        assert_eq!(p.tempo, 96.0, "the summary's tempo");
        // unchanged: nothing written
        save(&p, &pkg).unwrap();
        assert_eq!(std::fs::read_to_string(pkg.join("projectData")).unwrap(), xml);
        // edited
        let mut e = p.clone();
        let t = e.tracks.iter().position(|t| !t.regions.is_empty()).unwrap();
        e.tracks[t].regions[0].notes[0].pitch += 2;
        e.tempo = 101.0;
        e.tempo_map = vec![(0, 101.0)];
        e.key = "G major".into();
        let s = save(&e, &pkg).unwrap();
        assert!(s.unsaved.is_empty() && s.left_out.is_empty(), "{:?}", s.left_out);
        assert!(pkg.join("projectData.before-bramble").exists());
        let back = project::open(&pkg).unwrap();
        assert_eq!(back.tracks[t].regions[0].notes, e.tracks[t].regions[0].notes);
        assert_eq!(back.tempo, 101.0);
        assert_eq!(back.key, "G major");
        let inner = project::read_project_data(&project::band_logic_model(&plist::Value::from_file(pkg.join("projectData")).unwrap()).unwrap()).unwrap();
        assert_eq!(inner.tempo, 101.0, "the model's own tempo too");
        assert_eq!(inner.key, "G major");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
