#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> eframe::Result {
    // the plug-in scanner: Studio runs itself to look at one plug-in file (lg_host::scan)
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--scan-plugin") {
        lg_host::scan::child_main(&args[2..]);
    }
    lg_app::run()
}
