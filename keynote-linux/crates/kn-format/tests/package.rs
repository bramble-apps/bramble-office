//! Saving as a package (folder with Index.zip) and reading it back.
use kn_format::KeyFile;

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn package_round_trip() {
    let src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R10-standard.key");
    let k = KeyFile::open(&src).unwrap();
    let dir = std::env::temp_dir().join(format!("kn-package-test-{}.key", std::process::id()));
    k.save_package(&dir).unwrap();
    assert!(dir.join("Index.zip").is_file());
    let k2 = KeyFile::open(&dir).unwrap();
    assert_eq!(k2.objects_of("KN.SlideArchive").count(), k.objects_of("KN.SlideArchive").count());
    // and back to a single file in place
    k2.save(&dir).unwrap();
    assert!(dir.is_file());
    let _ = std::fs::remove_file(&dir);
}
