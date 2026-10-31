//! Letters: a word processor and page-layout app for Linux that reads and writes Apple Pages
//! (`.pages`) documents through the suite's iWork crates.
//!
//! Automated checks with no human present (the app screenshots itself and quits):
//! `CS_SNAP=out.png [CS_PAGE=n] [CS_ZOOM=z] [CS_CMD=menu command,…] [CS_SCRIPT="click x y; type …; key …; menu …"] [CS_SAVE=out.pages] letters file.pages`
//! Headless conversion: `letters --pdf|--docx|--rtf|--txt|--epub|--html|--png in out`.
// Windows: a window app, without a console window behind it
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> eframe::Result {
    pg_app::run()
}
