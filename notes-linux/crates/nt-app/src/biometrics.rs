//! Opening locked notes with the system's biometrics (Notes' "Use Touch ID", named here after what
//! the system has): fprintd's fingerprint on Linux, Windows Hello, Touch ID on macOS.
//!
//! As Apple's: turning it on keeps the account's notes password in the system's secret store
//! (Secret Service / GNOME Keyring, the Windows Credential Manager, the macOS Keychain); a
//! successful biometric check releases it to open the note the same way the typed password does.
//! The password always still works, and turning it off deletes the stored password.

use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

/// The option's name in Settings.
pub fn label() -> &'static str {
    if cfg!(target_os = "windows") {
        "Use Windows Hello"
    } else if cfg!(target_os = "macos") {
        "Use Touch ID"
    } else if cfg!(target_os = "linux") {
        "Use Fingerprint"
    } else {
        "Use Biometrics"
    }
}

/// How the option and the lock screen speak of it ("Use your fingerprint to view…").
pub fn your() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows Hello"
    } else if cfg!(target_os = "macos") {
        "Touch ID"
    } else {
        "your fingerprint"
    }
}

/// The greyed option's hint when the system has nothing set up.
pub fn setup_hint() -> &'static str {
    if cfg!(target_os = "windows") {
        "Set up Windows Hello in Settings to use it here."
    } else if cfg!(target_os = "macos") {
        "Set up Touch ID in System Settings to use it here."
    } else {
        "Set up a fingerprint in Settings to use it here."
    }
}

/// Whether the system can check one now: None while that's still being asked (once, in the
/// background), then whether a reader is there with a finger enrolled. Windows and macOS only say
/// so when asked to check, so they count as ready until a check says otherwise.
pub fn available() -> Option<bool> {
    static ASKED: std::sync::Once = std::sync::Once::new();
    ASKED.call_once(|| {
        std::thread::spawn(|| *AVAILABLE.lock().unwrap() = Some(system_has_biometrics()));
    });
    *AVAILABLE.lock().unwrap()
}

static AVAILABLE: Mutex<Option<bool>> = Mutex::new(None);

fn system_has_biometrics() -> bool {
    if std::env::var_os("NT_SNAP").is_some() {
        // (snapshots: as if a reader with a finger were there, never touching the real one)
        return std::env::var_os("NT_NO_BIOMETRICS").is_none();
    }
    #[cfg(target_os = "linux")]
    {
        // fprintd: a reader, and at least one finger enrolled for this user
        let user = std::env::var("USER").unwrap_or_default();
        let out = std::process::Command::new("fprintd-list").arg(&user).stdin(std::process::Stdio::null()).output();
        out.is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim_start().starts_with("- #")))
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        true
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        false
    }
}

/// Asks for the biometric check (fprintd waits for a finger on the reader; Windows and macOS show
/// their own prompt). The answer comes on the receiver: Ok, or why not.
pub fn verify(ctx: &egui::Context, reason: &str) -> Receiver<Result<(), String>> {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    let reason = reason.to_string();
    std::thread::spawn(move || {
        let r = check(&reason);
        if r.as_ref().is_err_and(|e| e == setup_hint()) {
            // (nothing set up after all: the option greys out with its hint)
            *AVAILABLE.lock().unwrap() = Some(false);
        }
        let _ = tx.send(r);
        ctx.request_repaint();
    });
    rx
}

#[cfg(target_os = "linux")]
fn check(_reason: &str) -> Result<(), String> {
    let out = std::process::Command::new("fprintd-verify").stdin(std::process::Stdio::null()).output().map_err(|_| "fprintd isn’t installed.".to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    if text.contains("verify-match") {
        Ok(())
    } else if text.contains("verify-no-match") {
        Err("The fingerprint wasn’t recognized.".into())
    } else if text.contains("No devices available") || text.contains("NoEnrolledPrints") || text.contains("no-enrolled") {
        Err(setup_hint().into())
    } else {
        Err("The fingerprint couldn’t be checked.".into())
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn check(reason: &str) -> Result<(), String> {
    use robius_authentication::{AndroidText, BiometricStrength, Context, PolicyBuilder, Text, WindowsText};
    let policy = PolicyBuilder::new().biometrics(Some(BiometricStrength::Strong)).password(false).build().ok_or("Biometrics aren’t available.")?;
    let text = Text { android: AndroidText { title: "Notes", subtitle: None, description: None }, apple: reason, windows: WindowsText::new_truncated("Notes", reason) };
    let (tx, rx) = channel();
    let cx = Context::new(());
    cx.authenticate(text, &policy, move |r| {
        let _ = tx.send(r);
    })
    .map_err(|e| format!("{e:?}"))?;
    // (the context stays alive until the prompt answers)
    match rx.recv() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(robius_authentication::Error::Unavailable)) => Err(setup_hint().into()),
        Ok(Err(e)) => Err(format!("{e:?}")),
        Err(_) => Err("The check was cancelled.".into()),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn check(_reason: &str) -> Result<(), String> {
    Err("Biometrics aren’t available.".into())
}

fn entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("Bramble Notes", &format!("locked-notes/{account}")).map_err(|e| e.to_string())
}

/// Keeps the account's notes password in the secret store.
pub fn store_key(account: &str, password: &str) -> Result<(), String> {
    if std::env::var_os("NT_SNAP").is_some() {
        return Ok(());
    }
    entry(account)?.set_password(password).map_err(|e| e.to_string())
}

/// The stored notes password (only after a successful check).
pub fn load_key(account: &str) -> Option<String> {
    entry(account).ok()?.get_password().ok()
}

/// Deletes the stored notes password (the option turned off, or the password no longer right).
pub fn forget_key(account: &str) {
    if let Ok(e) = entry(account) {
        let _ = e.delete_credential();
    }
}
