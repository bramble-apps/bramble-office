//! Two processes opening one library: the second waits for the first (its own test binary, since
//! what opening is doing is the process's).
mod common;

use std::time::{Duration, Instant};

use nt_core::native::{self, Opening};
use nt_core::store::Store;

#[test]
fn a_second_opener_waits_with_a_message() {
    let dir = common::scratch("native-lock");
    let lib = dir.join("bramble-notes");
    drop(Store::open_library(&lib).unwrap());
    // another process holds the library (a lock on its own open file, as another process's is)
    let held = std::fs::OpenOptions::new().write(true).open(lib.join(".lock")).unwrap();
    held.lock().unwrap();
    let l = lib.clone();
    let opener = std::thread::spawn(move || Store::open_library(&l).map(|_| ()));
    let start = Instant::now();
    while native::opening() != Opening::Waiting && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(native::opening(), Opening::Waiting);
    assert!(native::opening().message().unwrap().contains("Waiting"));
    std::thread::sleep(Duration::from_millis(200));
    assert!(!opener.is_finished(), "it waits while the library is held");
    held.unlock().unwrap();
    opener.join().unwrap().unwrap();
    assert_eq!(native::opening(), Opening::Idle);
    assert_eq!(Opening::Converting { done: 120, total: 1186 }.message().unwrap(), "Updating your library… 120 of 1186");
}
