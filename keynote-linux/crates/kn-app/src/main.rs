//! Keynote for Linux — application entry point.
// Windows: a window app, without a console window behind it
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> eframe::Result {
    kn_app::run()
}
