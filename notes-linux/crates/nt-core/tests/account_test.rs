//! Accounts: a Mac's accounts imported as accounts (not folders), older imports unwrapped.

mod common;

use nt_core::model::*;
use nt_core::store::{self, Store};

fn note(id: &str, folder: &str, text: &str) -> Note {
    Note { id: id.into(), folder: folder.into(), title: text.into(), body: Body::from_lines(text, &["x"]), created: 1.7e9, modified: 1.7e9, ..Default::default() }
}

fn folder(id: &str, account: &str, name: &str, parent: Option<&str>) -> Folder {
    Folder { id: id.into(), account: account.into(), name: name.into(), parent: parent.map(str::to_string), default: id.starts_with("DefaultFolder-"), ..Default::default() }
}

/// A Mac library: an iCloud account (its Notes folder, Work and Work/Sub) and On My Mac (Notes, Home).
fn mac() -> Library {
    Library {
        accounts: vec![Account { id: "ICLOUD-1".into(), name: "iCloud".into(), kind: 1 }, Account { id: "LocalAccount".into(), name: "On My Mac".into(), kind: 3 }],
        folders: vec![
            folder("DefaultFolder-ICLOUD-1", "ICLOUD-1", "Notes", None),
            folder("WORK", "ICLOUD-1", "Work", None),
            folder("SUB", "ICLOUD-1", "Sub", Some("WORK")),
            folder("DefaultFolder-LocalAccount", "LocalAccount", "Notes", None),
            folder("HOME", "LocalAccount", "Home", None),
        ],
        notes: vec![note("N1", "DefaultFolder-ICLOUD-1", "in iCloud Notes"), note("N2", "SUB", "in Sub"), note("N3", "DefaultFolder-LocalAccount", "on my Mac"), note("N4", "HOME", "at home")],
        tags: Vec::new(),
    }
}

#[test]
fn a_macs_accounts_come_in_as_accounts() {
    let dir = common::scratch("accounts-import");
    let mut st = Store::open(&dir).unwrap();
    st.import_library(&mac()).unwrap();
    let lib = st.library().unwrap();
    let names: Vec<&str> = lib.accounts.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"iCloud"), "{names:?}");
    // no wrapper folders
    assert!(!lib.folders.iter().any(|f| f.name == "iCloud" || f.name == "On My Mac"));
    let icloud = lib.accounts.iter().find(|a| a.name == "iCloud").unwrap().id.clone();
    let f = |name: &str| lib.folders.iter().find(|f| f.name == name && f.kind == FolderKind::Normal && !f.default).unwrap().clone();
    let (work, sub, home) = (f("Work"), f("Sub"), f("Home"));
    assert_eq!((work.account.as_str(), work.parent.as_deref()), (icloud.as_str(), None));
    assert_eq!((sub.account.as_str(), sub.parent.as_deref()), (icloud.as_str(), Some(work.id.as_str())));
    assert_eq!((home.account.as_str(), home.parent.as_deref()), (store::ACCOUNT_ID, None));
    // iCloud's Notes folder is a folder of that account; On My Mac's notes are in this library's Notes
    let icloud_notes = lib.folder(&lib.note("N1").unwrap().folder).unwrap();
    assert_eq!((icloud_notes.name.as_str(), icloud_notes.account.as_str()), ("Notes", icloud.as_str()));
    assert_eq!(lib.note("N3").unwrap().folder, store::DEFAULT_FOLDER);
    assert_eq!(lib.note("N2").unwrap().folder, sub.id);
    // a folder made in that account later
    let made = st.create_folder_in(&icloud, "Later").unwrap();
    assert_eq!(st.library().unwrap().folder(&made).unwrap().account, icloud);
    // a sub-folder takes its parent's account
    let child = st.create_folder("Child", Some(&work.id)).unwrap();
    assert_eq!(st.library().unwrap().folder(&child).unwrap().account, icloud);
}

#[test]
fn older_imports_are_unwrapped() {
    let dir = common::scratch("accounts-unwrap");
    let mut st = Store::open(&dir).unwrap();
    // as imports used to make them: an "iCloud" folder and an "On My Mac" folder holding each
    // account's folders
    let ic = st.create_folder("iCloud", None).unwrap();
    let work = st.create_folder("Work", Some(&ic)).unwrap();
    let sub = st.create_folder("Sub", Some(&work)).unwrap();
    let ic_notes = st.create_folder("Notes", Some(&ic)).unwrap();
    let mac = st.create_folder("On My Mac", None).unwrap();
    let mac_notes = st.create_folder("Notes", Some(&mac)).unwrap();
    let home = st.create_folder("Home", Some(&mac)).unwrap();
    let mut ids = Vec::new();
    for (f, t) in [(&sub, "in sub"), (&ic_notes, "in iCloud notes"), (&mac_notes, "on my mac"), (&home, "at home")] {
        let mut b = Body::from_lines(t, &["x"]);
        ids.push(st.create_note(Some(f), &mut b).unwrap().id);
    }
    // a user's own folder that only happens to be called iCloud (it has notes): left alone
    let own = st.create_folder("iCloud", None).unwrap();
    let mut b = Body::from_lines("mine", &["x"]);
    st.create_note(Some(&own), &mut b).unwrap();
    drop(st);

    let st = Store::open(&dir).unwrap();
    let lib = st.library().unwrap();
    let icloud = lib.accounts.iter().find(|a| a.name == "iCloud").expect("an iCloud account").id.clone();
    assert!(lib.folder(&ic).is_none() && lib.folder(&mac).is_none() && lib.folder(&mac_notes).is_none());
    assert_eq!(lib.folder(&work).map(|f| (f.account.clone(), f.parent.clone())), Some((icloud.clone(), None)));
    assert_eq!(lib.folder(&sub).unwrap().parent.as_deref(), Some(work.as_str()));
    assert_eq!(lib.folder(&ic_notes).unwrap().account, icloud);
    assert_eq!(lib.folder(&home).map(|f| (f.account.clone(), f.parent.clone())), Some((store::ACCOUNT_ID.to_string(), None)));
    assert_eq!(lib.folder(&own).unwrap().name, "iCloud");
    // every note kept, where it was (the Mac's own Notes folder: this library's Notes)
    assert_eq!(lib.note(&ids[0]).unwrap().folder, sub);
    assert_eq!(lib.note(&ids[1]).unwrap().folder, ic_notes);
    assert_eq!(lib.note(&ids[2]).unwrap().folder, store::DEFAULT_FOLDER);
    assert_eq!(lib.note(&ids[3]).unwrap().folder, home);
    assert_eq!(lib.notes.len(), 5);
}

#[test]
fn accounts_survive_the_files() {
    let dir = common::scratch("accounts-native");
    let lib_dir = dir.join("lib");
    let mut st = Store::open_library(&lib_dir).unwrap();
    st.import_library(&mac()).unwrap();
    let want = st.library().unwrap();
    drop(st);
    std::fs::remove_dir_all(lib_dir.join("index")).unwrap();
    let st = Store::open_library(&lib_dir).unwrap();
    let got = st.library().unwrap();
    let shape = |l: &Library| {
        let mut v: Vec<String> = l.folders.iter().map(|f| {
            let acct = l.accounts.iter().find(|a| a.id == f.account).map(|a| a.name.clone()).unwrap_or_default();
            format!("{} {} {:?} {}", f.id, f.name, f.parent, acct)
        }).collect();
        v.sort();
        v
    };
    assert_eq!(shape(&got), shape(&want));
}

#[test]
fn an_older_library_json_gains_its_accounts() {
    let dir = common::scratch("accounts-json");
    let lib_dir = dir.join("lib");
    let mut st = Store::open_library(&lib_dir).unwrap();
    st.import_library(&mac()).unwrap();
    drop(st);
    // library.json as written before accounts were kept
    let path = lib_dir.join("library.json");
    let mut j: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    j.as_object_mut().unwrap().remove("accounts");
    std::fs::write(&path, j.to_string()).unwrap();
    let st = Store::open_library(&lib_dir).unwrap();
    let j: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(j["accounts"].as_array().map(|a| a.len()), Some(st.library().unwrap().accounts.len()));
}
