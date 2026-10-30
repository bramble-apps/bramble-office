//! Progress of opening a presentation, reported to whoever asked on this thread (the app's
//! "Opening…" window), and a way to cancel it.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

/// Shared between the opening thread and the UI: progress in thousandths, and Cancel.
#[derive(Default)]
pub struct Progress {
    pub permille: AtomicU32,
    pub cancel: AtomicBool,
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Progress>>> = const { RefCell::new(None) };
    /// The part of the bar the current step fills (copying a cloud file, then opening it).
    static SPAN: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((0.0, 1.0)) };
}

/// Map the next step's 0..1 progress onto `lo..hi` of the bar.
pub fn span(lo: f32, hi: f32) {
    SPAN.with(|s| s.set((lo, hi)));
}

/// Report opening progress on this thread to `p` (None stops reporting).
pub fn watch(p: Option<Arc<Progress>>) {
    span(0.0, 1.0);
    CURRENT.with(|c| *c.borrow_mut() = p);
}

/// Set progress to `fraction` (0..1) of the way; false if the user cancelled.
pub fn report(fraction: f32) -> bool {
    CURRENT.with(|c| match &*c.borrow() {
        Some(p) => {
            let (lo, hi) = SPAN.with(|s| s.get());
            let f = lo + (hi - lo) * fraction.clamp(0.0, 1.0);
            p.permille.store((f * 1000.0) as u32, Ordering::Relaxed);
            !p.cancel.load(Ordering::Relaxed)
        }
        None => true,
    })
}
