//! Shared app state: the running scan (if any) and the last finished one.

use std::sync::{Arc, Mutex, MutexGuard};

use cleaner_core::progress::CancelToken;
use cleaner_core::scan::Scan;

/// A finished scan the frontend can browse. Node IDs are only valid
/// together with this scan's `id`.
#[derive(Debug)]
pub struct LoadedScan {
    pub id: u64,
    pub scan: Scan,
}

#[derive(Debug, Default)]
pub struct Inner {
    pub next_id: u64,
    pub running: Option<(u64, CancelToken)>,
    pub current: Option<Arc<LoadedScan>>,
}

#[derive(Debug, Default)]
pub struct AppState(Mutex<Inner>);

impl AppState {
    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while holding the lock leaves plain data behind, which is
        // still safe to use.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}
