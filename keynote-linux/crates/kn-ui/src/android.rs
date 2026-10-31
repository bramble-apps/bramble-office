//! Android: the apps run in app.bramble.BrambleActivity (tools/android), a NativeActivity with a
//! few Java helpers for what Rust can't reach on its own: the system file picker, "Open with",
//! web links and the on-screen keyboard.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use jni::objects::{JObject, JString, JValue};
pub use winit::platform::android::activity::AndroidApp;

static APP: OnceLock<AndroidApp> = OnceLock::new();
/// The file picker is open: keep drawing now and then so its answer shows up.
static PICKING: AtomicBool = AtomicBool::new(false);
/// An app's own text editor took typing in the last frame.
static TYPING: AtomicBool = AtomicBool::new(false);
/// The on-screen keyboard was asked for.
static KEYBOARD: AtomicBool = AtomicBool::new(false);

/// At the start of android_main: logcat, the app, and HOME and the XDG folders (Android sets
/// none) in the app's own storage, where settings, recent files and caches go.
pub fn init(app: &AndroidApp, tag: &'static str) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag(tag));
    std::panic::set_hook(Box::new(|p| log::error!("{p}")));
    let _ = APP.set(app.clone());
    if let Some(dir) = app.internal_data_path() {
        let set = |k: &str, v: PathBuf| {
            let _ = std::fs::create_dir_all(&v);
            // (at the start of android_main, before the app starts any thread)
            unsafe { std::env::set_var(k, v) };
        };
        set("HOME", dir.clone());
        set("XDG_CONFIG_HOME", dir.join("config"));
        set("XDG_DATA_HOME", dir.join("data"));
        set("XDG_CACHE_HOME", dir.join("cache"));
        set("TMPDIR", dir.join("tmp"));
    }
    // (automated checks: KEY=VALUE lines, e.g. the apps' snapshot hooks, from a file adb can
    // write: /sdcard/Android/data/<app id>/files/env)
    if let Some(env) = app.external_data_path().and_then(|d| std::fs::read_to_string(d.join("env")).ok()) {
        for (k, v) in env.lines().filter_map(|l| l.split_once('=')) {
            unsafe { std::env::set_var(k.trim(), v.trim()) };
        }
    }
}

/// The app, for eframe's NativeOptions.
pub fn app() -> Option<AndroidApp> {
    APP.get().cloned()
}

/// Calls into the activity (from the app's thread, which stays attached to Java).
fn call<T>(f: impl FnOnce(&mut jni::JNIEnv, &JObject) -> jni::errors::Result<T>) -> Option<T> {
    let app = APP.get()?;
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) }.ok()?;
    let mut env = vm.attach_current_thread_permanently().ok()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let r = f(&mut env, &activity);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    r.map_err(|e| log::warn!("java call: {e}")).ok()
}

fn call_str(method: &str, arg: &str) -> bool {
    call(|env, act| {
        let s = env.new_string(arg)?;
        env.call_method(act, method, "(Ljava/lang/String;)V", &[JValue::Object(&s)])?;
        Ok(())
    })
    .is_some()
}

/// Opens a web address in the browser.
pub fn open_url(url: &str) -> bool {
    call_str("openUrl", url)
}

/// Files picked or opened with the app since the last call (copies in the app's folder).
fn take_picked() -> Vec<PathBuf> {
    let s = call(|env, act| {
        let o = env.call_method(act, "takeResults", "()Ljava/lang/String;", &[])?.l()?;
        if o.is_null() {
            return Ok(String::new());
        }
        Ok(env.get_string(&JString::from(o))?.into())
    });
    s.unwrap_or_default().lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect()
}

pub(crate) fn frame(ctx: &egui::Context) {
    test_snapshots(ctx);
    let picked = take_picked();
    if !picked.is_empty() {
        PICKING.store(false, Ordering::Relaxed);
        ctx.input_mut(|i| i.raw.dropped_files.extend(picked.into_iter().map(|p| std::sync::Arc::new(Picked(p)) as egui::DroppedFileHandle)));
        ctx.request_repaint();
    } else if PICKING.load(Ordering::Relaxed) {
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
    }
    // the on-screen keyboard while text is being typed (a text field, or an app's own editor)
    let want = ctx.text_edit_focused() || TYPING.swap(false, Ordering::Relaxed);
    if want != KEYBOARD.swap(want, Ordering::Relaxed)
        && let Some(app) = APP.get()
    {
        if want { app.show_soft_input(true) } else { app.hide_soft_input(true) }
    }
}

/// An app's own text editor was open last frame: the on-screen keyboard shows.
pub(crate) fn typing(_ctx: &egui::Context) {
    TYPING.store(true, Ordering::Relaxed);
}

/// A file from the system picker, handed to the app as a dropped file.
#[derive(Debug)]
struct Picked(PathBuf);

impl egui::DroppedFile for Picked {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Android's stand-in for rfd's FileDialog, with the same calls. Picking opens the system
/// picker and returns nothing yet: the files arrive as dropped files once chosen. Saving
/// returns a path in the app's folder at once; the system picker asks where it goes, and the
/// activity copies the file there (again whenever the app saves it).
#[derive(Default)]
pub struct FileDialog {
    exts: Vec<String>,
    name: Option<String>,
}

impl FileDialog {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_filter(mut self, _name: impl Into<String>, extensions: &[impl ToString]) -> Self {
        self.exts.extend(extensions.iter().map(|e| e.to_string().to_lowercase()));
        self
    }
    pub fn set_title(self, _title: impl Into<String>) -> Self {
        self
    }
    pub fn set_file_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
    pub fn set_directory(self, _dir: impl AsRef<Path>) -> Self {
        self
    }
    pub fn pick_file(self) -> Option<PathBuf> {
        self.open(false);
        None
    }
    pub fn pick_files(self) -> Option<Vec<PathBuf>> {
        self.open(true);
        None
    }
    pub fn save_file(self) -> Option<PathBuf> {
        let mut name = self.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Untitled".into()).replace('/', "-");
        if let Some(ext) = self.exts.first()
            && Path::new(&name).extension().is_none()
        {
            name = format!("{name}.{ext}");
        }
        let path = out_dir()?.join(name);
        call_str("pickSave", &path.to_string_lossy()).then_some(path)
    }
    pub fn pick_folder(self) -> Option<PathBuf> {
        let dir = out_dir()?;
        call_str("pickFolder", &dir.to_string_lossy()).then_some(dir)
    }

    fn open(&self, multiple: bool) {
        const PICTURES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "heic", "tif", "tiff", "bmp", "svg"];
        let mime = if !self.exts.is_empty() && self.exts.iter().all(|e| PICTURES.contains(&e.as_str())) { "image/*" } else { "*/*" };
        let ok = call(|env, act| {
            let m = env.new_string(mime)?;
            env.call_method(act, "pickOpen", "(Ljava/lang/String;Z)V", &[JValue::Object(&m), JValue::Bool(multiple.into())])?;
            Ok(())
        });
        PICKING.store(ok.is_some(), Ordering::Relaxed);
    }
}

/// A new folder for one save, in the app's own storage.
fn out_dir() -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    let dir = PathBuf::from(std::env::var_os("HOME")?).join("out").join(format!("{stamp:x}"));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Automated checks (Waydroid's own screenshots come out black): with KN_UI_SNAPDIR set (in the
/// env file), the app saves what it shows as snap.png there whenever a file snap-now appears.
fn test_snapshots(ctx: &egui::Context) {
    let Some(dir) = std::env::var_os("KN_UI_SNAPDIR").map(PathBuf::from) else { return };
    let trigger = dir.join("snap-now");
    if trigger.exists() {
        let _ = std::fs::remove_file(&trigger);
        let out = dir.join("snap.png");
        let cb = egui::ScreenshotCallback::new(move |img: std::sync::Arc<egui::ColorImage>| {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                let _ = buf.save(&out);
            }
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
        ctx.request_repaint();
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
}
