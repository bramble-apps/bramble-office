//! Browser runtime shell for Bramble Office WebAssembly builds (Keydeck, Values, Letters).
//!
//! Runs the desktop `eframe::App` directly in `eframe::WebRunner` (wgpu: WebGPU with WebGL2
//! fallback), wires asynchronous browser file pickers, drag-and-drop, clipboard image paste,
//! `localStorage` preferences, lazy HTTP fetching of Apple-matched fonts, and the optional
//! `?host=parent` same-origin `<iframe>` `postMessage` bridge.

pub mod host;

#[cfg(target_arch = "wasm32")]
pub mod fetch;
#[cfg(target_arch = "wasm32")]
pub mod files;
#[cfg(target_arch = "wasm32")]
pub mod storage;

#[cfg(target_arch = "wasm32")]
mod runtime {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen_futures::{spawn_local, JsFuture};

    use super::files::WebEvent;
    use super::host::{self, Bridge};

    const MAX_PASTED_BYTES: f64 = (200u32 << 20) as f64;
    const MAX_PASTED_PICTURES: u32 = 20;

    /// Configuration for starting a Bramble Office app in the browser.
    pub struct WebConfig {
        /// Prefix for `?host=parent` postMessage types (`"keydeck"`, `"values"`, `"letters"`).
        pub prefix: &'static str,
        /// Display name for loading/error banners.
        pub title: &'static str,
        /// Version string reported in `<prefix>:ready`.
        pub version: &'static str,
        /// `<canvas id="...">` element id.
        pub canvas_id: &'static str,
        /// Loading overlay element id removed once the app starts.
        pub loading_id: &'static str,
        /// Fallback document filename when `?host=parent` sends an unnamed file.
        pub default_name: &'static str,
    }

    thread_local! {
        static EVENTS: RefCell<Vec<WebEvent>> = const { RefCell::new(Vec::new()) };
        static BRIDGE: RefCell<Option<Bridge>> = const { RefCell::new(None) };
        static CTX: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
        static DIRTY: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        static HOST_AUTHOR: RefCell<Option<String>> = const { RefCell::new(None) };
        static FONTS_BASE: RefCell<String> = RefCell::new("./fonts/matched".to_string());
        static THEMES_BASE: RefCell<String> = RefCell::new("./themes".to_string());
    }

    pub fn push_event(ev: WebEvent) {
        EVENTS.with(|e| e.borrow_mut().push(ev));
    }

    pub fn take_events() -> Vec<WebEvent> {
        EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
    }

    pub fn bridge() -> Option<Bridge> {
        BRIDGE.with(|b| b.borrow().clone())
    }

    pub fn is_hosted() -> bool {
        BRIDGE.with(|b| b.borrow().is_some())
    }

    pub fn host_author() -> Option<String> {
        HOST_AUTHOR.with(|a| a.borrow().clone())
    }

    pub fn fonts_base_url() -> String {
        FONTS_BASE.with(|f| f.borrow().clone())
    }

    pub fn themes_base_url() -> String {
        THEMES_BASE.with(|t| t.borrow().clone())
    }

    pub fn request_repaint() {
        CTX.with(|c| {
            if let Some(ctx) = c.borrow().as_ref() {
                ctx.request_repaint();
            }
        });
    }

    /// Update the unsaved-changes state: controls the standalone `beforeunload` guard and emits
    /// `<prefix>:dirty` to the parent page when in `?host=parent` mode.
    pub fn set_dirty(dirty: bool) {
        DIRTY.with(|d| d.set(dirty));
        BRIDGE.with(|b| {
            if let Some(bridge) = b.borrow().as_ref() {
                bridge.post_dirty(dirty);
            }
        });
    }

    /// Reply to a `?host=parent` `<prefix>:request` message.
    pub fn respond_host(id: &serde_json::Value, res: Result<serde_json::Value, host::HostError>) {
        BRIDGE.with(|b| {
            if let Some(bridge) = b.borrow().as_ref() {
                bridge.post_response(id, res);
            }
        });
    }

    /// Open a URL in a new browser tab (`noopener`).
    pub fn open_url(url: &str) {
        if let Some(window) = web_sys::window() {
            let _ = window.open_with_url_and_target_and_features(url, "_blank", "noopener");
        }
    }

    fn query() -> String {
        web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
    }

    fn query_param(key: &str) -> Option<String> {
        web_sys::UrlSearchParams::new_with_str(&query()).ok()?.get(key)
    }

    /// Start the eframe WebRunner on `#<cfg.canvas_id>`.
    pub fn start<A: eframe::App + 'static>(cfg: WebConfig, make: impl FnOnce(&eframe::CreationContext<'_>) -> A + 'static) {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
        spawn_local(async move {
            let Some(window) = web_sys::window() else {
                log::error!("no window");
                return;
            };
            let Some(document) = window.document() else {
                log::error!("no document");
                return;
            };
            let Some(canvas) = document
                .get_element_by_id(cfg.canvas_id)
                .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            else {
                log::error!("missing <canvas id=\"{}\">", cfg.canvas_id);
                return;
            };

            if let Some(base) = query_param("fonts").filter(|s| !s.trim().is_empty()) {
                FONTS_BASE.with(|f| *f.borrow_mut() = base.trim_end_matches('/').to_string());
            }
            if let Some(base) = query_param("themes").filter(|s| !s.trim().is_empty()) {
                THEMES_BASE.with(|t| *t.borrow_mut() = base.trim_end_matches('/').to_string());
            }

            let bridge = if query_param("host").as_deref() == Some("parent") {
                Bridge::new(cfg.prefix, cfg.default_name)
            } else {
                None
            };
            if bridge.is_some() {
                if let Some(author) = query_param("author").and_then(|a| host::sanitize_author(&a)) {
                    HOST_AUTHOR.with(|h| *h.borrow_mut() = Some(author));
                }
            }
            BRIDGE.with(|b| *b.borrow_mut() = bridge.clone());

            if bridge.is_none() {
                let dirty = DIRTY.with(Rc::clone);
                install_unload_guard(&window, dirty);
            }

            let mut options = eframe::WebOptions::default();
            if query().contains("webgl")
                && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
            {
                create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
            }

            let app_bridge = bridge.clone();
            let result = eframe::WebRunner::new()
                .start(
                    canvas,
                    options,
                    Box::new(move |cc| {
                        CTX.with(|c| *c.borrow_mut() = Some(cc.egui_ctx.clone()));
                        if let Some(rs) = &cc.wgpu_render_state {
                            log::info!("{}: wgpu backend {:?}", cfg.prefix, rs.adapter.get_info().backend);
                        }
                        if let Some(b) = &app_bridge {
                            b.listen(cc.egui_ctx.clone());
                        }
                        let _ = listen_for_pasted_pictures(cc.egui_ctx.clone());
                        let app = make(cc);
                        Ok(Box::new(WebShell { app }))
                    }),
                )
                .await;

            if let Some(b) = &bridge {
                match &result {
                    Ok(()) => b.post_ready(cfg.version),
                    Err(e) => b.post_failed(&format!("{e:?}")),
                }
            }
            if let Some(el) = document.get_element_by_id(cfg.loading_id) {
                match result {
                    Ok(()) => el.remove(),
                    Err(e) => el.set_inner_html(&format!(
                        "<p>{} failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>",
                        cfg.title
                    )),
                }
            }
        });
    }

    /// Registers a matched-font fetcher callback against `kn_render::fonts` (called from each
    /// app's `start_web`, since `kn-ui` does not depend on `kn-render`).
    pub fn fetch_matched_font_asset(rel_path: &'static str, on_bytes: fn(&'static str, Vec<u8>)) {
        let base = fonts_base_url();
        let url = format!("{base}/{rel_path}");
        super::fetch::fetch_bytes(url, move |res| match res {
            Ok(bytes) => on_bytes(rel_path, bytes),
            Err(e) => log::warn!("matched font fetch failed ({rel_path}): {e}"),
        });
    }

    fn install_unload_guard(window: &web_sys::Window, dirty: Rc<Cell<bool>>) {
        let on_beforeunload = Closure::<dyn FnMut(web_sys::BeforeUnloadEvent)>::new(move |ev: web_sys::BeforeUnloadEvent| {
            if dirty.get() {
                ev.prevent_default();
                ev.set_return_value("");
            }
        });
        if window
            .add_event_listener_with_callback("beforeunload", on_beforeunload.as_ref().unchecked_ref())
            .is_ok()
        {
            on_beforeunload.forget();
        }
    }

    fn listen_for_pasted_pictures(ctx: egui::Context) -> Result<(), String> {
        let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
        let on_paste = Closure::<dyn FnMut(web_sys::ClipboardEvent)>::new(move |e: web_sys::ClipboardEvent| {
            let Some(data) = e.clipboard_data() else { return };
            if !data.get_data("text").unwrap_or_default().is_empty() {
                return;
            }
            let Some(files) = data.files() else { return };
            for i in 0..files.length().min(MAX_PASTED_PICTURES) {
                let Some(file) = files.get(i) else { continue };
                if !file.type_().starts_with("image/") {
                    continue;
                }
                if file.size() > MAX_PASTED_BYTES {
                    log::warn!("pasted picture is larger than 200 MB; ignored");
                    continue;
                }
                let ctx = ctx.clone();
                spawn_local(async move {
                    if let Ok(buf) = JsFuture::from(file.array_buffer()).await {
                        let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                        push_event(WebEvent::PastedImage { name: "pasted.png".into(), bytes });
                        ctx.request_repaint();
                    }
                });
            }
        });
        document
            .add_event_listener_with_callback("paste", on_paste.as_ref().unchecked_ref())
            .map_err(|e| format!("{e:?}"))?;
        on_paste.forget();
        Ok(())
    }

    struct WebShell<A: eframe::App> {
        app: A,
    }

    impl<A: eframe::App> eframe::App for WebShell<A> {
        fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
            self.app.raw_input_hook(ctx, raw);
        }

        fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
            self.app.logic(ctx, frame);
        }

        fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
            self.app.clear_color(visuals)
        }

        fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
            if ui.ctx().current_pass_index() == 0 {
                let ctx = ui.ctx().clone();
                let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
                for f in dropped {
                    let ctx = ctx.clone();
                    spawn_local(async move {
                        let name = f
                            .path()
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .filter(|n| !n.is_empty())
                            .unwrap_or_else(|| "dropped".into());
                        match f.bytes_async().await {
                            Ok(bytes) => {
                                push_event(WebEvent::Dropped { name, bytes });
                                ctx.request_repaint();
                            }
                            Err(e) => log::error!("couldn't read dropped file {name}: {e}"),
                        }
                    });
                }
            }
            self.app.ui(ui, frame);
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use runtime::*;
