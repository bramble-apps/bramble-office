//! Browser file picking, drag-and-drop / paste inbox events, and file download / host-save delivery.

use std::path::{Path, PathBuf};

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::{spawn_local, JsFuture};

/// Asynchronous file events delivered into an app on WebAssembly.
#[derive(Debug)]
pub enum WebEvent {
    /// Document opened via File > Open, Welcome > Open…, or `?host=parent` `<app>:open`.
    OpenDoc { name: String, bytes: Vec<u8> },
    /// Picture picked via Insert > Picture / Choose Image.
    InsertImage { name: String, bytes: Vec<u8> },
    /// Media file picked via Insert > Choose….
    InsertMedia { name: String, bytes: Vec<u8> },
    /// Multiple pictures picked for an image gallery.
    InsertGallery(Vec<(String, Vec<u8>)>),
    /// File dropped onto the browser canvas (routed by extension).
    Dropped { name: String, bytes: Vec<u8> },
    /// Picture pasted from the browser clipboard (`paste` event).
    PastedImage { name: String, bytes: Vec<u8> },
    /// Control-protocol request from a `?host=parent` iframe host (`<app>:request`).
    HostRequest(super::host::HostRequest),
}

/// Open the browser's file picker (`<input type="file">`) and invoke `on_files` with all chosen
/// `(filename, bytes)` pairs.
pub fn pick_files(extensions: &[&str], multiple: bool, on_files: impl FnOnce(Vec<(String, Vec<u8>)>) + 'static) {
    let Some(window) = web_sys::window() else { return };
    let Some(document) = window.document() else { return };
    let Ok(el) = document.create_element("input") else { return };
    let Ok(input) = el.dyn_into::<web_sys::HtmlInputElement>() else { return };
    input.set_type("file");
    if multiple {
        input.set_multiple(true);
    }
    if !extensions.is_empty() {
        let accept: String = extensions
            .iter()
            .map(|e| format!(".{}", e.trim_start_matches('.')))
            .collect::<Vec<_>>()
            .join(",");
        input.set_accept(&accept);
    }
    let _ = input.set_attribute("style", "display:none");
    if let Some(body) = document.body() {
        let _ = body.append_child(&input);
    }

    let input_cb = input.clone();
    let on_files = std::cell::RefCell::new(Some(on_files));
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_ev: web_sys::Event| {
        let Some(cb) = on_files.borrow_mut().take() else { return };
        let Some(list) = input_cb.files() else {
            input_cb.remove();
            return;
        };
        let mut files = Vec::with_capacity(list.length() as usize);
        for i in 0..list.length() {
            if let Some(f) = list.get(i) {
                files.push(f);
            }
        }
        input_cb.remove();
        spawn_local(async move {
            let mut out = Vec::with_capacity(files.len());
            for f in files {
                let name = f.name();
                if let Ok(buf) = JsFuture::from(f.array_buffer()).await {
                    let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                    out.push((name, bytes));
                }
            }
            if !out.is_empty() {
                cb(out);
                crate::web::request_repaint();
            }
        });
    });
    let _ = input.add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref());
    on_change.forget();
    input.click();
}

/// Convenience helper: open the browser picker and enqueue a single `WebEvent`.
pub fn pick_open_doc(extensions: &[&str], multiple: bool) {
    pick_files(extensions, multiple, |files| {
        for (name, bytes) in files {
            crate::web::push_event(WebEvent::OpenDoc { name, bytes });
        }
    });
}

pub fn pick_insert_image(extensions: &[&str]) {
    pick_files(extensions, false, |files| {
        if let Some((name, bytes)) = files.into_iter().next() {
            crate::web::push_event(WebEvent::InsertImage { name, bytes });
        }
    });
}

pub fn pick_insert_media(extensions: &[&str]) {
    pick_files(extensions, false, |files| {
        if let Some((name, bytes)) = files.into_iter().next() {
            crate::web::push_event(WebEvent::InsertMedia { name, bytes });
        }
    });
}

pub fn pick_insert_gallery(extensions: &[&str]) {
    pick_files(extensions, true, |files| {
        if !files.is_empty() {
            crate::web::push_event(WebEvent::InsertGallery(files));
        }
    });
}

/// Deliver a saved document: to the `?host=parent` iframe bridge when hosted, or as a browser
/// Blob download (`<a download>`) when standalone.
pub fn deliver_file(name: &str, bytes: &[u8], save_as: bool) -> Result<(), String> {
    if let Some(bridge) = crate::web::bridge() {
        return bridge.post_save(name, bytes, save_as);
    }
    download_file(name, bytes)
}

/// Trigger a browser file download (`<a download>`) regardless of whether hosted in an iframe.
/// Used for "Export To…" outputs (PDF, HTML, images ZIP, etc.).
pub fn download_file(name: &str, bytes: &[u8]) -> Result<(), String> {
    let window = web_sys::window().ok_or_else(|| "no browser window".to_string())?;
    let document = window.document().ok_or_else(|| "no document".to_string())?;
    let basename = Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Untitled".to_string());
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::of1(&array.buffer());
    let props = web_sys::BlobPropertyBag::new();
    props.set_type("application/octet-stream");
    let blob = web_sys::Blob::new_with_buffer_source_sequence_and_options(&parts, &props)
        .map_err(|e| format!("couldn't create Blob: {e:?}"))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|e| format!("couldn't create object URL: {e:?}"))?;
    let el = document.create_element("a").map_err(|e| format!("create <a> failed: {e:?}"))?;
    let anchor: web_sys::HtmlAnchorElement = el.dyn_into().map_err(|_| "not an anchor element".to_string())?;
    anchor.set_href(&url);
    anchor.set_download(&basename);
    let _ = anchor.set_attribute("style", "display:none");
    if let Some(body) = document.body() {
        let _ = body.append_child(&anchor);
    }
    anchor.click();
    anchor.remove();
    // Revoke on a delayed callback so the browser finishes starting the download first.
    let revoke = Closure::once(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.as_ref().unchecked_ref(), 10_000);
    revoke.forget();
    Ok(())
}

/// Stand-in for `rfd::FileDialog` on WebAssembly so legacy desktop dialog code paths compile
/// cleanly; interactive web paths use [`pick_files`] and [`deliver_file`] directly.
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
        let refs: Vec<&str> = self.exts.iter().map(String::as_str).collect();
        pick_files(&refs, false, |files| {
            for (name, bytes) in files {
                crate::web::push_event(WebEvent::Dropped { name, bytes });
            }
        });
        None
    }
    pub fn pick_files(self) -> Option<Vec<PathBuf>> {
        let refs: Vec<&str> = self.exts.iter().map(String::as_str).collect();
        pick_files(&refs, true, |files| {
            for (name, bytes) in files {
                crate::web::push_event(WebEvent::Dropped { name, bytes });
            }
        });
        None
    }
    pub fn save_file(self) -> Option<PathBuf> {
        None
    }
    pub fn pick_folder(self) -> Option<PathBuf> {
        None
    }
}
