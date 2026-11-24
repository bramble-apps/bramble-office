//! Async browser HTTP fetch helpers (matched Apple fonts, Keydeck themes, Content Hub).

use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::{spawn_local, JsFuture};

/// Percent-encode a single query parameter value for URLs.
pub fn encode_query_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Fetch raw bytes from `url` asynchronously using the browser's Fetch API.
pub async fn fetch_bytes_async(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or_else(|| "no browser window".to_string())?;
    let opts = web_sys::RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(web_sys::RequestMode::Cors);
    let resp_val = JsFuture::from(window.fetch_with_str_and_init(url, &opts))
        .await
        .map_err(|e| format!("fetch failed: {e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "invalid fetch response".to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let buf_promise = resp.array_buffer().map_err(|e| format!("array_buffer failed: {e:?}"))?;
    let buf = JsFuture::from(buf_promise).await.map_err(|e| format!("reading body failed: {e:?}"))?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// Spawn a browser fetch task for raw bytes and invoke `on_done` when it completes.
pub fn fetch_bytes(url: String, on_done: impl FnOnce(Result<Vec<u8>, String>) + 'static) {
    spawn_local(async move {
        let res = fetch_bytes_async(&url).await;
        on_done(res);
        crate::web::request_repaint();
    });
}
