//! Starting on a graphics system that works: when the window can't be created on the one wgpu
//! picked (DirectX 12 under Wine, or with an old driver), the app starts again on the next one,
//! Vulkan and then OpenGL, with the same files.

/// Pass the app's `eframe::run_native` result through this.
pub fn retry_other_backend(r: eframe::Result) -> eframe::Result {
    let Err(e) = &r else { return r };
    let next = match std::env::var("WGPU_BACKEND").ok().as_deref() {
        None => "vulkan",
        Some("vulkan") => "gl",
        _ => return r,
    };
    eprintln!("couldn't start the window ({e}); trying {next}");
    let Ok(exe) = std::env::current_exe() else { return r };
    match std::process::Command::new(exe).args(std::env::args_os().skip(1)).env("WGPU_BACKEND", next).status() {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(_) => r,
    }
}
