//! A .logicx package on disk, beyond the song: its project alternatives, Logic's backups of
//! each alternative's ProjectData, and the audio files it holds (format-analysis §1, §18).
//!
//! - Resources/ProjectInformation.plist: VariantNames {"n": name}, VariantNamesV2 {"n": name or
//!   "{PROJECT_NAME}" (alternative 0 follows the package's name)}, ActiveVariant (the open one).
//! - Alternatives/NNN/: ProjectData, MetaData.plist, DisplayState… one folder per alternative;
//!   NNN is the key as three digits.
//! - Alternatives/NNN/Project File Backups/NN/: ProjectData + MetaData.plist, the ten latest
//!   saves (00..09, reused oldest first; E23).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One project alternative.
#[derive(Clone, Debug, PartialEq)]
pub struct Alternative {
    pub key: u32,
    pub name: String,
    pub dir: PathBuf,
}

fn info_path(pkg: &Path) -> PathBuf {
    pkg.join("Resources/ProjectInformation.plist")
}

fn read_info(pkg: &Path) -> plist::Dictionary {
    match plist::Value::from_file(info_path(pkg)) {
        Ok(plist::Value::Dictionary(d)) => d,
        _ => plist::Dictionary::new(),
    }
}

fn write_info(pkg: &Path, d: plist::Dictionary) -> std::io::Result<()> {
    std::fs::create_dir_all(pkg.join("Resources"))?;
    plist::Value::Dictionary(d).to_file_binary(info_path(pkg)).map_err(std::io::Error::other)
}

fn dir_of(pkg: &Path, key: u32) -> PathBuf {
    pkg.join(format!("Alternatives/{key:03}"))
}

/// The package's alternatives, by key, with ProjectData in their folder.
pub fn list(pkg: &Path) -> Vec<Alternative> {
    let d = read_info(pkg);
    let mut out: Vec<Alternative> = Vec::new();
    if let Some(plist::Value::Dictionary(v)) = d.get("VariantNames") {
        for (k, name) in v {
            let Ok(key) = k.parse::<u32>() else { continue };
            let dir = dir_of(pkg, key);
            if dir.join("ProjectData").exists() {
                out.push(Alternative { key, name: name.as_string().unwrap_or("").to_string(), dir });
            }
        }
    }
    if out.is_empty() {
        if let Ok(rd) = std::fs::read_dir(pkg.join("Alternatives")) {
            for e in rd.flatten() {
                let dir = e.path();
                let Some(key) = dir.file_name().and_then(|n| n.to_str()).and_then(|n| n.parse::<u32>().ok()) else { continue };
                if dir.join("ProjectData").exists() {
                    out.push(Alternative { key, name: format!("{key:03}"), dir });
                }
            }
        }
    }
    out.sort_by_key(|a| a.key);
    out
}

/// The alternative open when the project was saved (ActiveVariant), else the first.
pub fn active(pkg: &Path) -> Option<Alternative> {
    let all = list(pkg);
    let want = read_info(pkg).get("ActiveVariant").and_then(|v| v.as_signed_integer().or_else(|| v.as_real().map(|r| r as i64)));
    want.and_then(|k| all.iter().find(|a| a.key as i64 == k).cloned()).or_else(|| all.first().cloned())
}

/// Makes `key` the alternative the package opens with.
pub fn set_active(pkg: &Path, key: u32) -> std::io::Result<()> {
    let mut d = read_info(pkg);
    d.insert("ActiveVariant".into(), plist::Value::Integer((key as i64).into()));
    write_info(pkg, d)
}

fn set_name(d: &mut plist::Dictionary, key: u32, name: Option<&str>) {
    for field in ["VariantNames", "VariantNamesV2"] {
        let mut v = match d.remove(field) {
            Some(plist::Value::Dictionary(v)) => v,
            _ => plist::Dictionary::new(),
        };
        match name {
            Some(n) => {
                v.insert(key.to_string(), plist::Value::String(n.to_string()));
            }
            None => {
                v.remove(&key.to_string());
            }
        }
        d.insert(field.into(), plist::Value::Dictionary(v));
    }
}

/// Copies an alternative's files (not its autosaves, backups or undo data).
fn copy_alternative(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let name = e.file_name();
        let n = name.to_string_lossy();
        if n == "Autosave" || n == "Project File Backups" || n.ends_with(".nosync") || n.ends_with(".bak") || n.ends_with(".before-bramble") {
            continue;
        }
        if e.path().is_dir() {
            copy_dir(&e.path(), &to.join(&name))?;
        } else {
            std::fs::copy(e.path(), to.join(&name))?;
        }
    }
    Ok(())
}

/// Copies a folder and everything in it.
pub fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let q = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &q)?;
        } else {
            std::fs::copy(e.path(), &q)?;
        }
    }
    Ok(())
}

/// File ▸ Project Alternatives ▸ New Alternative…: a copy of alternative `from` under the next
/// free key, named `name`, made the active one.
pub fn new_alternative(pkg: &Path, from: &Path, name: &str) -> std::io::Result<Alternative> {
    let first = list(pkg).iter().map(|a| a.key + 1).max().unwrap_or(0);
    let key = (first..1000).find(|k| !dir_of(pkg, *k).exists()).unwrap_or(999);
    let dir = dir_of(pkg, key);
    copy_alternative(from, &dir)?;
    let mut d = read_info(pkg);
    set_name(&mut d, key, Some(name));
    d.insert("ActiveVariant".into(), plist::Value::Integer((key as i64).into()));
    write_info(pkg, d)?;
    Ok(Alternative { key, name: name.to_string(), dir })
}

/// Edit Alternatives ▸ rename. (Alternative 0 named after the package keeps "{PROJECT_NAME}" in
/// VariantNamesV2 only while its name is the package's.)
pub fn rename(pkg: &Path, key: u32, name: &str) -> std::io::Result<()> {
    let mut d = read_info(pkg);
    set_name(&mut d, key, Some(name));
    let stem = pkg.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if key == 0 && name == stem {
        if let Some(plist::Value::Dictionary(v)) = d.get_mut("VariantNamesV2") {
            v.insert("0".into(), plist::Value::String("{PROJECT_NAME}".into()));
        }
    }
    write_info(pkg, d)
}

/// Edit Alternatives ▸ delete: its folder and names (never the last one, never the open one).
pub fn delete(pkg: &Path, key: u32, open_key: u32) -> std::io::Result<()> {
    let all = list(pkg);
    if all.len() < 2 || key == open_key || !all.iter().any(|a| a.key == key) {
        return Err(std::io::Error::other("the open alternative and a project's last alternative can't be deleted"));
    }
    std::fs::remove_dir_all(dir_of(pkg, key))?;
    let mut d = read_info(pkg);
    set_name(&mut d, key, None);
    write_info(pkg, d)
}

/// Export Alternative as Project…: a new package holding this alternative only (as 000), with
/// the package's Contents, Resources and Media.
pub fn export(pkg: &Path, key: u32, dest: &Path) -> std::io::Result<()> {
    let alt = list(pkg).into_iter().find(|a| a.key == key).ok_or_else(|| std::io::Error::other("no such alternative"))?;
    for part in ["Contents", "Resources", "Media"] {
        if pkg.join(part).is_dir() {
            copy_dir(&pkg.join(part), &dest.join(part))?;
        }
    }
    copy_alternative(&alt.dir, &dest.join("Alternatives/000"))?;
    let stem = dest.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut d = read_info(dest);
    d.remove("VariantNames");
    d.remove("VariantNamesV2");
    set_name(&mut d, 0, Some(&stem));
    if let Some(plist::Value::Dictionary(v)) = d.get_mut("VariantNamesV2") {
        v.insert("0".into(), plist::Value::String("{PROJECT_NAME}".into()));
    }
    d.insert("ActiveVariant".into(), plist::Value::Integer(0.into()));
    write_info(dest, d)
}

/// A package renamed (Project Management ▸ Rename…): the folder moved, alternative 0's name
/// following it when it was the package's.
pub fn rename_package(pkg: &Path, new_name: &str) -> std::io::Result<PathBuf> {
    let dest = pkg.with_file_name(format!("{new_name}.logicx"));
    if dest.exists() {
        return Err(std::io::Error::other(format!("{} exists", dest.display())));
    }
    let old = pkg.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    std::fs::rename(pkg, &dest)?;
    let mut d = read_info(&dest);
    let follows = matches!(d.get("VariantNamesV2"), Some(plist::Value::Dictionary(v)) if v.get("0").and_then(|x| x.as_string()) == Some("{PROJECT_NAME}"));
    let was_named = matches!(d.get("VariantNames"), Some(plist::Value::Dictionary(v)) if v.get("0").and_then(|x| x.as_string()) == Some(old.as_str()));
    if follows || was_named {
        if let Some(plist::Value::Dictionary(v)) = d.get_mut("VariantNames") {
            v.insert("0".into(), plist::Value::String(new_name.to_string()));
        }
        write_info(&dest, d)?;
    }
    Ok(dest)
}

// --- backups -----------------------------------------------------------------------------------

fn backups_dir(alt: &Path) -> PathBuf {
    alt.join("Project File Backups")
}

/// The alternative's backups, newest first: (slot folder, when it was made).
pub fn backups(alt: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut out: Vec<(PathBuf, SystemTime)> = std::fs::read_dir(backups_dir(alt))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("ProjectData").is_file())
        .filter_map(|p| Some((p.clone(), std::fs::metadata(p.join("ProjectData")).ok()?.modified().ok()?)))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

/// Before a save: the alternative's ProjectData and MetaData.plist as they are go into the next
/// backup slot (the first free of 00..`keep`-1, else the oldest).
pub fn make_backup(alt: &Path, keep: usize) -> std::io::Result<PathBuf> {
    let dir = backups_dir(alt);
    let keep = keep.clamp(1, 100);
    let slots: Vec<PathBuf> = (0..keep).map(|k| dir.join(format!("{k:02}"))).collect();
    let slot = match slots.iter().find(|s| !s.join("ProjectData").exists()) {
        Some(s) => s.clone(),
        None => slots.iter().min_by_key(|s| std::fs::metadata(s.join("ProjectData")).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH)).cloned().unwrap_or_else(|| slots[0].clone()),
    };
    std::fs::create_dir_all(&slot)?;
    std::fs::copy(alt.join("ProjectData"), slot.join("ProjectData"))?;
    if alt.join("MetaData.plist").exists() {
        std::fs::copy(alt.join("MetaData.plist"), slot.join("MetaData.plist"))?;
    }
    // (copying keeps the source's times on some systems: the backup is dated now)
    let f = std::fs::File::options().write(true).open(slot.join("ProjectData"))?;
    f.set_modified(SystemTime::now())?;
    Ok(slot)
}

// --- audio files ---------------------------------------------------------------------------------

/// The package's audio folder (Media/Audio Files, or a project folder's Audio Files).
pub fn audio_dir(path: &Path) -> PathBuf {
    let pkg = crate::project::package_of(path);
    let folder = path.join("Audio Files");
    if path != pkg && folder.is_dir() { folder } else { pkg.join("Media/Audio Files") }
}

/// The audio files every alternative uses (their AuFl names).
pub fn used_audio(pkg: &Path) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for a in list(pkg) {
        let Ok(data) = std::fs::read(a.dir.join("ProjectData")) else { continue };
        let Ok(chunks) = crate::chunks::walk(&data) else { continue };
        for c in chunks.iter().filter(|c| c.is("AuFl")) {
            if let Some(n) = crate::project::audio_file_name(c.payload) {
                out.insert(n);
            }
        }
    }
    out
}

/// Clean Up…: the files in the audio folder that no alternative uses (and `also_used`, the files
/// the open project's regions use now, doesn't name).
pub fn unused_audio(path: &Path, also_used: &[String]) -> Vec<PathBuf> {
    let pkg = crate::project::package_of(path);
    let used = used_audio(&pkg);
    let mut out: Vec<PathBuf> = std::fs::read_dir(audio_dir(path))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).is_some_and(|n| !n.starts_with('.') && !used.contains(&n) && !also_used.contains(&n)))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lg-package-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn make_pkg(dir: &Path) -> PathBuf {
        let pkg = dir.join("Song.logicx");
        std::fs::create_dir_all(pkg.join("Alternatives/000")).unwrap();
        std::fs::write(pkg.join("Alternatives/000/ProjectData"), b"data0").unwrap();
        std::fs::write(pkg.join("Alternatives/000/MetaData.plist"), b"meta").unwrap();
        let mut d = plist::Dictionary::new();
        set_name(&mut d, 0, Some("Song"));
        if let Some(plist::Value::Dictionary(v)) = d.get_mut("VariantNamesV2") {
            v.insert("0".into(), plist::Value::String("{PROJECT_NAME}".into()));
        }
        write_info(&pkg, d).unwrap();
        pkg
    }

    #[test]
    fn alternatives_new_rename_switch_delete_export() {
        let dir = scratch("alts");
        let pkg = make_pkg(&dir);
        assert_eq!(active(&pkg).unwrap().key, 0);
        std::fs::create_dir_all(pkg.join("Alternatives/000/Project File Backups/00")).unwrap();
        let a = new_alternative(&pkg, &pkg.join("Alternatives/000"), "Song #1").unwrap();
        assert_eq!((a.key, a.dir.join("ProjectData").exists()), (1, true));
        assert!(!a.dir.join("Project File Backups").exists(), "a new alternative has no backups yet (§18)");
        assert_eq!(active(&pkg).unwrap().key, 1, "the new one is open");
        rename(&pkg, 1, "Bridge idea").unwrap();
        assert_eq!(list(&pkg).iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Song", "Bridge idea"]);
        set_active(&pkg, 0).unwrap();
        assert_eq!(active(&pkg).unwrap().name, "Song");
        assert!(delete(&pkg, 0, 0).is_err(), "not the open one");
        let out = dir.join("Bridge.logicx");
        export(&pkg, 1, &out).unwrap();
        assert_eq!(list(&out).iter().map(|a| (a.key, a.name.clone())).collect::<Vec<_>>(), [(0, "Bridge".to_string())]);
        delete(&pkg, 1, 0).unwrap();
        assert_eq!(list(&pkg).len(), 1);
        assert!(delete(&pkg, 0, 1).is_err(), "not the last one");
        let moved = rename_package(&pkg, "Song 2").unwrap();
        assert_eq!(list(&moved)[0].name, "Song 2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backups_take_the_ten_latest() {
        let dir = scratch("backups");
        let pkg = make_pkg(&dir);
        let alt = pkg.join("Alternatives/000");
        for k in 0..12u8 {
            std::fs::write(alt.join("ProjectData"), [k]).unwrap();
            make_backup(&alt, 10).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        let b = backups(&alt);
        assert_eq!(b.len(), 10);
        assert_eq!(std::fs::read(b[0].0.join("ProjectData")).unwrap(), [11u8], "newest first");
        assert_eq!(std::fs::read(b[9].0.join("ProjectData")).unwrap(), [2u8]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unused_audio_is_what_no_alternative_names() {
        let dir = scratch("audio");
        let pkg = make_pkg(&dir);
        std::fs::create_dir_all(pkg.join("Media/Audio Files")).unwrap();
        std::fs::write(pkg.join("Media/Audio Files/take.wav"), b"x").unwrap();
        std::fs::write(pkg.join("Media/Audio Files/keep.wav"), b"x").unwrap();
        let u = unused_audio(&pkg, &["keep.wav".to_string()]);
        assert_eq!(u, vec![pkg.join("Media/Audio Files/take.wav")]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
