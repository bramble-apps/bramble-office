//! File dialogs: the system's own on desktops (rfd). On Android the system file picker answers
//! later, after the app has gone on drawing: picked files arrive as dropped files, and a save
//! goes to a copy in the app's own folder that is written to the chosen place (android.rs).

#[cfg(target_os = "android")]
pub use crate::android::FileDialog;
#[cfg(not(target_os = "android"))]
pub use rfd::FileDialog;
