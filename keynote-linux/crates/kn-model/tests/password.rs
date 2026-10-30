//! Password-protected presentations: Keynote's file opens with its password, and ours round-trip.
use kn_model::Document;

fn deck() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R12-password.key")
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn opens_keynote_protected_file() {
    match Document::open(deck()) {
        Err(kn_format::Error::NeedsPassword(h)) => assert_eq!(h, "hint"),
        _ => panic!("should need a password"),
    }
    assert!(Document::open_with_password(deck(), "wrong", "").is_err());
    let d = Document::open_with_password(deck(), "test", "hint").unwrap();
    let s = d.slides();
    assert!(!s.is_empty());
    assert!(d.scene(&s, 0).is_some());
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn saves_protected() {
    let mut d = Document::open_with_password(deck(), "test", "hint").unwrap();
    let out = std::env::temp_dir().join(format!("kn-pw-{}.key", std::process::id()));
    d.password = Some(("secret".into(), "the hint".into()));
    d.save_as(&out).unwrap();
    assert!(matches!(Document::open(&out), Err(kn_format::Error::NeedsPassword(h)) if h == "the hint"));
    let d2 = Document::open_with_password(&out, "secret", "the hint").unwrap();
    assert_eq!(d2.slides().len(), d.slides().len());
    let _ = std::fs::remove_file(out);
}
