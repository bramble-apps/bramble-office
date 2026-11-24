//! The `postMessage` bridge to a same-origin parent page (`?host=parent`), for pages that embed
//! Keydeck, Values or Letters in an `<iframe>` and store their documents (for example a file
//! server or groupware app).
//!
//! Each message is a plain JS object with a `type` prefixed by the app name (`keydeck`, `values`
//! or `letters`):
//! - `<app>:ready` (to parent): `{ version }`, sent once `WebRunner` is up and listening.
//! - `<app>:failed` (to parent): `{ error }`, sent if startup fails (e.g. no WebGPU / WebGL2).
//! - `<app>:open` (from parent): `{ name, bytes }` (`ArrayBuffer` or `Uint8Array`).
//! - `<app>:save` (to parent): `{ name, saveAs, bytes }` (transferred `ArrayBuffer`).
//! - `<app>:dirty` (to parent): `{ dirty }` whenever unsaved changes appear or clear.
//! - `<app>:request` (from parent): `{ id, method, params }` for host control commands
//!   (`engine.execute`, `engine.commands`, `document.inspect`).
//! - `<app>:response` (to parent): `{ id, reply: { ok: true, result } | { ok: false, error, commandRejected } }`.

use std::path::Path;

pub use serde_json;

/// Maximum length accepted from `?author=`.
pub const MAX_AUTHOR_CHARS: usize = 200;

/// Format a namespaced host message type, e.g. `"keydeck:save"`.
pub fn message_type(prefix: &str, kind: &str) -> String {
    format!("{prefix}:{kind}")
}

/// Sanitize a host-supplied `?author=` query parameter (trim whitespace, clamp length).
pub fn sanitize_author(raw: &str) -> Option<String> {
    let cleaned: String = raw.trim().chars().take(MAX_AUTHOR_CHARS).collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// A control-protocol request received from a `?host=parent` page (`<app>:request`).
#[derive(Debug, Clone)]
pub struct HostRequest {
    pub id: serde_json::Value,
    pub method: String,
    pub params: serde_json::Value,
}

/// Session-only light/dark appearance requested by the embedding page via `ui.hostTheme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostAppearance {
    Light,
    Dark,
}

/// Parsed control-protocol call from `<app>:request`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCall {
    /// `engine.commands` — list the supported host commands.
    Commands,
    /// `document.inspect` — report whether a document is open, its filename, and its dirty flag.
    Inspect,
    /// `engine.execute` -> `file.new`
    New { template: Option<String> },
    /// `engine.execute` -> `file.save`
    Save { path: Option<String>, keep_source: bool },
    /// `engine.execute` -> `file.saveAs`
    SaveAs { path: String, keep_source: bool },
    /// `engine.execute` -> `file.info`
    Info,
    /// `engine.execute` -> `ui.hostTheme` (`None` resets to the app's own setting).
    HostTheme { appearance: Option<HostAppearance> },
}

/// Error reply for `<app>:response`.
///
/// `command_rejected` is `true` when the request was refused before execution (unknown method,
/// unknown command, or invalid parameters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    pub message: String,
    pub command_rejected: bool,
}

impl HostError {
    pub fn rejected(message: impl Into<String>) -> Self {
        Self { message: message.into(), command_rejected: true }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self { message: message.into(), command_rejected: false }
    }
}

/// Extract a clean single filename component from a host-supplied path (rejecting empty names,
/// `.`, `..`, and path separators).
pub fn sanitize_filename(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return None;
    }
    let name = Path::new(trimmed).file_name()?.to_str()?.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return None;
    }
    Some(name.to_string())
}

/// Parse an incoming `<app>:request` into a typed [`HostCall`].
pub fn parse_host_call(method: &str, params: &serde_json::Value) -> Result<HostCall, HostError> {
    match method {
        "engine.commands" => Ok(HostCall::Commands),
        "document.inspect" => Ok(HostCall::Inspect),
        "engine.execute" => {
            let obj = params.as_object().ok_or_else(|| HostError::rejected("engine.execute parameters must be a JSON object"))?;
            let command = obj
                .get("command")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| HostError::rejected("engine.execute requires a non-empty command string"))?;
            let cmd_params = match obj.get("params") {
                None | Some(serde_json::Value::Null) => serde_json::Map::new(),
                Some(serde_json::Value::Object(map)) => map.clone(),
                Some(_) => return Err(HostError::rejected("command params must be a JSON object")),
            };
            match command {
                "file.new" => {
                    let template = match cmd_params.get("template") {
                        None | Some(serde_json::Value::Null) => None,
                        Some(serde_json::Value::String(s)) => {
                            let trimmed = s.trim();
                            (!trimmed.is_empty()).then(|| trimmed.to_string())
                        }
                        Some(_) => return Err(HostError::rejected("file.new template must be a string")),
                    };
                    Ok(HostCall::New { template })
                }
                "file.save" => {
                    let keep_source = match cmd_params.get("keepSource") {
                        None | Some(serde_json::Value::Null) => false,
                        Some(serde_json::Value::Bool(b)) => *b,
                        Some(_) => return Err(HostError::rejected("file.save keepSource must be a boolean")),
                    };
                    let path = match cmd_params.get("path") {
                        None | Some(serde_json::Value::Null) => None,
                        Some(serde_json::Value::String(s)) => {
                            let clean = sanitize_filename(s).ok_or_else(|| HostError::rejected("file.save path must be a valid filename"))?;
                            Some(clean)
                        }
                        Some(_) => return Err(HostError::rejected("file.save path must be a string")),
                    };
                    Ok(HostCall::Save { path, keep_source })
                }
                "file.saveAs" => {
                    let keep_source = match cmd_params.get("keepSource") {
                        None | Some(serde_json::Value::Null) => false,
                        Some(serde_json::Value::Bool(b)) => *b,
                        Some(_) => return Err(HostError::rejected("file.saveAs keepSource must be a boolean")),
                    };
                    let raw = cmd_params
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| HostError::rejected("file.saveAs requires a path string"))?;
                    let path = sanitize_filename(raw).ok_or_else(|| HostError::rejected("file.saveAs path must be a valid filename"))?;
                    Ok(HostCall::SaveAs { path, keep_source })
                }
                "file.info" => Ok(HostCall::Info),
                "ui.hostTheme" => {
                    let appearance = match cmd_params.get("appearance") {
                        None | Some(serde_json::Value::Null) => None,
                        Some(serde_json::Value::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
                            "" | "auto" | "system" => None,
                            "light" => Some(HostAppearance::Light),
                            "dark" => Some(HostAppearance::Dark),
                            other => return Err(HostError::rejected(format!("unsupported appearance: {other}"))),
                        },
                        Some(_) => return Err(HostError::rejected("ui.hostTheme appearance must be a string")),
                    };
                    Ok(HostCall::HostTheme { appearance })
                }
                other => Err(HostError::rejected(format!("unknown command: {other}"))),
            }
        }
        other => Err(HostError::rejected(format!("unknown method: {other}"))),
    }
}

/// Metadata catalog returned by `engine.commands`.
pub fn supported_commands_json() -> serde_json::Value {
    serde_json::json!([
        {
            "id": "file.new",
            "label": "Create a new blank document",
            "params": {
                "type": "object",
                "properties": {
                    "template": { "type": "string" }
                }
            }
        },
        {
            "id": "file.save",
            "label": "Save the open document",
            "params": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "keepSource": { "type": "boolean" }
                }
            }
        },
        {
            "id": "file.saveAs",
            "label": "Save or export the open document under a target filename",
            "params": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "keepSource": { "type": "boolean" }
                },
                "required": ["path"]
            }
        },
        {
            "id": "file.info",
            "label": "Return the open document name and unsaved state",
            "params": { "type": "object", "properties": {} }
        },
        {
            "id": "ui.hostTheme",
            "label": "Apply a session light or dark appearance from the host page",
            "params": {
                "type": "object",
                "properties": {
                    "appearance": { "type": "string", "enum": ["light", "dark", "system"] }
                }
            }
        }
    ])
}

/// Resolve the effective `dark` flag for `ui.hostTheme` without writing user preferences.
/// Captures the app's original `current_dark` on the first host override so `{}` restores it.
pub fn resolve_host_appearance(current_dark: bool, appearance: Option<HostAppearance>) -> bool {
    thread_local! {
        static ORIGINAL_DARK: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    }
    ORIGINAL_DARK.with(|orig| match appearance {
        Some(HostAppearance::Light) => {
            if orig.get().is_none() {
                orig.set(Some(current_dark));
            }
            false
        }
        Some(HostAppearance::Dark) => {
            if orig.get().is_none() {
                orig.set(Some(current_dark));
            }
            true
        }
        None => orig.take().unwrap_or(current_dark),
    })
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::cell::Cell;
    use std::rc::Rc;

    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast as _, JsValue};

    use super::{message_type, HostError, HostRequest};
    use crate::web::files::WebEvent;

    #[derive(Clone)]
    pub struct Bridge {
        window: web_sys::Window,
        parent: web_sys::Window,
        origin: String,
        prefix: &'static str,
        default_name: &'static str,
        reported_dirty: Rc<Cell<Option<bool>>>,
    }

    impl Bridge {
        /// Connect to a same-origin parent window. Returns `None` if not framed, if the origin is
        /// opaque (`"null"`), or if the parent is cross-origin.
        pub fn new(prefix: &'static str, default_name: &'static str) -> Option<Self> {
            let window = web_sys::window()?;
            let parent = window.parent().ok().flatten()?;
            if js_sys::Object::is(&parent, &window) {
                return None;
            }
            let origin = window.location().origin().ok()?;
            if origin == "null" {
                return None;
            }
            if parent.location().origin().ok()? != origin {
                return None;
            }
            Some(Self {
                window,
                parent,
                origin,
                prefix,
                default_name,
                reported_dirty: Rc::new(Cell::new(None)),
            })
        }

        /// Listen for `<prefix>:open` and `<prefix>:request` messages from the same-origin parent
        /// window in arrival order.
        pub fn listen(&self, ctx: egui::Context) {
            let parent = self.parent.clone();
            let origin = self.origin.clone();
            let open_type = message_type(self.prefix, "open");
            let request_type = message_type(self.prefix, "request");
            let default_name = self.default_name;
            let bridge = self.clone();
            let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
                if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                    return;
                }
                let data = event.data();
                let Some(ty) = string(&data, "type") else { return };
                if ty == open_type {
                    let name = string(&data, "name")
                        .filter(|n| !n.trim().is_empty())
                        .unwrap_or_else(|| default_name.to_string());
                    let Some(bytes) = bytes(&data) else { return };
                    crate::web::push_event(WebEvent::OpenDoc { name, bytes });
                    ctx.request_repaint();
                } else if ty == request_type {
                    let id = json_field(&data, "id").unwrap_or(serde_json::Value::Null);
                    let Some(method) = string(&data, "method") else {
                        bridge.post_response(&id, Err(HostError::rejected("request is missing a method string")));
                        return;
                    };
                    let params = match js_sys::Reflect::get(&data, &JsValue::from_str("params")) {
                        Ok(v) if v.is_undefined() || v.is_null() => serde_json::json!({}),
                        Ok(v) => match js_to_json(&v) {
                            Some(parsed) => parsed,
                            None => {
                                bridge.post_response(&id, Err(HostError::rejected("request params could not be serialized as JSON")));
                                return;
                            }
                        },
                        Err(_) => serde_json::json!({}),
                    };
                    crate::web::push_event(WebEvent::HostRequest(HostRequest { id, method, params }));
                    ctx.request_repaint();
                }
            });
            if self.window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref()).is_ok() {
                on_message.forget();
            }
        }

        pub fn post_ready(&self, version: &str) {
            let ty = message_type(self.prefix, "ready");
            self.send(&[("type", ty.into()), ("version", version.into())]);
        }

        pub fn post_failed(&self, error: &str) {
            let ty = message_type(self.prefix, "failed");
            self.send(&[("type", ty.into()), ("error", error.into())]);
        }

        pub fn post_dirty(&self, dirty: bool) {
            if self.reported_dirty.get() == Some(dirty) {
                return;
            }
            let ty = message_type(self.prefix, "dirty");
            if self.send(&[("type", ty.into()), ("dirty", dirty.into())]) {
                self.reported_dirty.set(Some(dirty));
            }
        }

        pub fn post_save(&self, path: &str, bytes: &[u8], save_as: bool) -> Result<(), String> {
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string());
            let buffer = js_sys::Uint8Array::from(bytes).buffer();
            let ty = message_type(self.prefix, "save");
            let msg = message(&[
                ("type", ty.into()),
                ("name", name.as_str().into()),
                ("saveAs", save_as.into()),
                ("bytes", buffer.clone().into()),
            ])
            .ok_or_else(|| "couldn't build host save message".to_string())?;
            self.parent
                .post_message_with_transfer(&msg, &self.origin, &js_sys::Array::of1(&buffer))
                .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
        }

        pub fn post_response(&self, id: &serde_json::Value, res: Result<serde_json::Value, HostError>) {
            let ty = message_type(self.prefix, "response");
            let reply = match res {
                Ok(result) => {
                    let js_result = json_to_js(&result).unwrap_or(JsValue::NULL);
                    message(&[("ok", JsValue::TRUE), ("result", js_result)])
                }
                Err(err) => message(&[
                    ("ok", JsValue::FALSE),
                    ("error", err.message.into()),
                    ("commandRejected", err.command_rejected.into()),
                ]),
            };
            let Some(reply) = reply else { return };
            let js_id = json_to_js(id).unwrap_or(JsValue::NULL);
            self.send(&[("type", ty.into()), ("id", js_id), ("reply", reply.into())]);
        }

        fn send(&self, fields: &[(&str, JsValue)]) -> bool {
            message(fields).is_some_and(|m| self.parent.post_message(&m, &self.origin).is_ok())
        }
    }

    fn message(fields: &[(&str, JsValue)]) -> Option<js_sys::Object> {
        let m = js_sys::Object::new();
        for (key, value) in fields {
            js_sys::Reflect::set(&m, &JsValue::from_str(key), value).ok()?;
        }
        Some(m)
    }

    fn string(data: &JsValue, key: &str) -> Option<String> {
        js_sys::Reflect::get(data, &JsValue::from_str(key)).ok()?.as_string()
    }

    fn js_to_json(val: &JsValue) -> Option<serde_json::Value> {
        let text = js_sys::JSON::stringify(val).ok()?.as_string()?;
        serde_json::from_str(&text).ok()
    }

    fn json_field(data: &JsValue, key: &str) -> Option<serde_json::Value> {
        let val = js_sys::Reflect::get(data, &JsValue::from_str(key)).ok()?;
        if val.is_undefined() || val.is_null() {
            return Some(serde_json::Value::Null);
        }
        js_to_json(&val)
    }

    fn json_to_js(val: &serde_json::Value) -> Option<JsValue> {
        let text = serde_json::to_string(val).ok()?;
        js_sys::JSON::parse(&text).ok()
    }

    fn bytes(data: &JsValue) -> Option<Vec<u8>> {
        let val = js_sys::Reflect::get(data, &JsValue::from_str("bytes")).ok()?;
        if let Some(arr) = val.dyn_ref::<js_sys::Uint8Array>() {
            Some(arr.to_vec())
        } else if val.is_instance_of::<js_sys::ArrayBuffer>() {
            Some(js_sys::Uint8Array::new(&val).to_vec())
        } else {
            None
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use imp::Bridge;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn host_message_types_use_app_prefix() {
        assert_eq!(message_type("keydeck", "ready"), "keydeck:ready");
        assert_eq!(message_type("values", "open"), "values:open");
        assert_eq!(message_type("letters", "save"), "letters:save");
        assert_eq!(message_type("letters", "dirty"), "letters:dirty");
        assert_eq!(message_type("keydeck", "failed"), "keydeck:failed");
        assert_eq!(message_type("keydeck", "request"), "keydeck:request");
        assert_eq!(message_type("keydeck", "response"), "keydeck:response");
    }

    #[test]
    fn sanitize_author_trims_and_clamps() {
        assert_eq!(sanitize_author("   "), None);
        assert_eq!(sanitize_author("  Ada Lovelace "), Some("Ada Lovelace".into()));
        let long = "x".repeat(MAX_AUTHOR_CHARS + 50);
        assert_eq!(sanitize_author(&long).unwrap().len(), MAX_AUTHOR_CHARS);
    }

    #[test]
    fn parse_host_call_accepts_supported_methods_and_commands() {
        assert_eq!(parse_host_call("engine.commands", &json!({})), Ok(HostCall::Commands));
        assert_eq!(parse_host_call("document.inspect", &json!({})), Ok(HostCall::Inspect));
        assert_eq!(
            parse_host_call("engine.execute", &json!({ "command": "file.new" })),
            Ok(HostCall::New { template: None })
        );
        assert_eq!(
            parse_host_call("engine.execute", &json!({ "command": "file.new", "params": { "template": "blank" } })),
            Ok(HostCall::New { template: Some("blank".into()) })
        );
        assert_eq!(
            parse_host_call("engine.execute", &json!({ "command": "file.save", "params": {} })),
            Ok(HostCall::Save { path: None, keep_source: false })
        );
        assert_eq!(
            parse_host_call("engine.execute", &json!({ "command": "file.save", "params": { "path": "deck.key", "keepSource": true } })),
            Ok(HostCall::Save { path: Some("deck.key".into()), keep_source: true })
        );
        assert_eq!(
            parse_host_call("engine.execute", &json!({ "command": "file.saveAs", "params": { "path": "/folder/export.pdf", "keepSource": true } })),
            Ok(HostCall::SaveAs { path: "export.pdf".into(), keep_source: true })
        );
        assert_eq!(
            parse_host_call("engine.execute", &json!({ "command": "file.saveAs", "params": { "path": "copy.pages" } })),
            Ok(HostCall::SaveAs { path: "copy.pages".into(), keep_source: false })
        );
        assert_eq!(parse_host_call("engine.execute", &json!({ "command": "file.info" })), Ok(HostCall::Info));
        assert_eq!(
            parse_host_call("engine.execute", &json!({ "command": "ui.hostTheme", "params": { "appearance": "Light" } })),
            Ok(HostCall::HostTheme { appearance: Some(HostAppearance::Light) })
        );
        assert_eq!(
            parse_host_call("engine.execute", &json!({ "command": "ui.hostTheme", "params": { "appearance": "dark" } })),
            Ok(HostCall::HostTheme { appearance: Some(HostAppearance::Dark) })
        );
        assert_eq!(
            parse_host_call("engine.execute", &json!({ "command": "ui.hostTheme", "params": {} })),
            Ok(HostCall::HostTheme { appearance: None })
        );
    }

    #[test]
    fn parse_host_call_rejects_invalid_requests() {
        assert!(parse_host_call("unknown.method", &json!({})).unwrap_err().command_rejected);
        assert!(parse_host_call("engine.execute", &json!({ "command": "nope" })).unwrap_err().command_rejected);
        assert!(parse_host_call("engine.execute", &json!({ "command": "file.saveAs", "params": {} })).unwrap_err().command_rejected);
        assert!(parse_host_call("engine.execute", &json!({ "command": "file.saveAs", "params": { "path": "  " } })).unwrap_err().command_rejected);
        assert!(parse_host_call("engine.execute", &json!({ "command": "file.saveAs", "params": { "path": ".." } })).unwrap_err().command_rejected);
        assert!(parse_host_call("engine.execute", &json!({ "command": "file.new", "params": { "template": 42 } })).unwrap_err().command_rejected);
        assert!(parse_host_call("engine.execute", &json!({ "command": "ui.hostTheme", "params": { "appearance": "neon" } })).unwrap_err().command_rejected);
    }

    #[test]
    fn resolve_host_appearance_restores_initial_state() {
        // Starts dark (true), host switches light -> dark -> reset ({}) restores true.
        assert!(!resolve_host_appearance(true, Some(HostAppearance::Light)));
        assert!(resolve_host_appearance(false, Some(HostAppearance::Dark)));
        assert!(resolve_host_appearance(true, None));
        // Now starts light (false), host switches dark -> reset restores false.
        assert!(resolve_host_appearance(false, Some(HostAppearance::Dark)));
        assert!(!resolve_host_appearance(true, None));
    }

    #[test]
    fn supported_commands_catalog_lists_five_commands() {
        let list = supported_commands_json();
        let arr = list.as_array().unwrap();
        let ids: Vec<&str> = arr.iter().filter_map(|c| c["id"].as_str()).collect();
        assert_eq!(ids, vec!["file.new", "file.save", "file.saveAs", "file.info", "ui.hostTheme"]);
    }
}
