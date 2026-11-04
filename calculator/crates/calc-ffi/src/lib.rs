//! C ABI over calc-core for the Flutter app: `cc_init(dir)` once, then `cc_call(json)` with
//! the requests in API.md. Returned strings are freed with `cc_free`.

use calc_core::api;
use calc_core::session::Session;
use std::ffi::{c_char, CStr, CString};
use std::path::PathBuf;
use std::sync::Mutex;

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn take(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

fn give(s: String) -> *mut c_char {
    CString::new(s.replace('\0', "")).map(|c| c.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// Opens the session with its data folder (settings, history, notes, rates).
#[no_mangle]
pub extern "C" fn cc_init(dir: *const c_char) {
    let d = take(dir);
    let s = Session::new(if d.is_empty() { None } else { Some(PathBuf::from(d)) });
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
}

#[no_mangle]
pub extern "C" fn cc_call(req: *const c_char) -> *mut c_char {
    let req: serde_json::Value = serde_json::from_str(&take(req)).unwrap_or(serde_json::Value::Null);
    let mut g = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let sn = g.get_or_insert_with(|| Session::new(None));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| api::call(sn, &req)))
        .unwrap_or_else(|_| serde_json::json!({"error": "panic"}));
    give(out.to_string())
}

#[no_mangle]
pub extern "C" fn cc_free(p: *mut c_char) {
    if !p.is_null() {
        drop(unsafe { CString::from_raw(p) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        cc_init(std::ptr::null());
        let req = CString::new(r#"{"cmd":"frame","w":393,"h":852,"insets":{"top":47,"bottom":34}}"#).unwrap();
        let out = cc_call(req.as_ptr());
        let s = take(out);
        cc_free(out);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["layout"]["keys"].as_array().unwrap().len(), 20);
        assert_eq!(v["display"]["main"][0]["t"], "0");
    }
}
