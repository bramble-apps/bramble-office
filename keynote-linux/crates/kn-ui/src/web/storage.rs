//! Browser `localStorage` helpers for persisting app settings and look preferences.

pub fn get(key: &str) -> Option<String> {
    web_sys::window()?.local_storage().ok()??.get_item(key).ok()?
}

pub fn set(key: &str, value: &str) {
    if let Some(store) = web_sys::window().and_then(|w| w.local_storage().ok()).flatten() {
        let _ = store.set_item(key, value);
    }
}

pub fn remove(key: &str) {
    if let Some(store) = web_sys::window().and_then(|w| w.local_storage().ok()).flatten() {
        let _ = store.remove_item(key);
    }
}
