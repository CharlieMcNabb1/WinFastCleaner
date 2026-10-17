//! Shared app state: the running scan (if any), the last finished one, and
//! the cleaner's latest results and previewed plan.

use std::sync::{Arc, Mutex, MutexGuard};

use cleaner_core::cleaner::{CleanPlan, JunkScan};
use cleaner_core::dupes::DupeResult;
use cleaner_core::finder::FinderResult;
use cleaner_core::progress::CancelToken;
use cleaner_core::scan::Scan;

/// A finished scan the frontend can browse. Node IDs are only valid
/// together with this scan's `id`.
#[derive(Debug)]
pub struct LoadedScan {
    pub id: u64,
    pub scan: Scan,
}

/// The latest junk scan. Rule items are referred to by index within it.
#[derive(Debug)]
pub struct LoadedJunk {
    pub id: u64,
    pub scan: JunkScan,
}

/// A plan the user has seen. Only a previewed plan can be executed, and
/// only once.
#[derive(Debug)]
pub struct PreviewedPlan {
    pub id: u64,
    pub plan: CleanPlan,
    pub label: &'static str,
    /// The "send green items to the bin" setting the preview was shown with.
    pub green_to_bin: bool,
}

/// The latest large/old file results. Files are referred to by index.
#[derive(Debug)]
pub struct LoadedFind {
    pub id: u64,
    pub result: FinderResult,
}

/// The latest duplicate results. Files are referred to by (group, file).
#[derive(Debug)]
pub struct LoadedDupes {
    pub id: u64,
    pub result: DupeResult,
}

#[derive(Debug, Default)]
pub struct Inner {
    pub next_id: u64,
    pub running: Option<(u64, CancelToken)>,
    pub current: Option<Arc<LoadedScan>>,
    pub junk_running: Option<CancelToken>,
    pub junk: Option<Arc<LoadedJunk>>,
    pub plan: Option<PreviewedPlan>,
    pub cleaning: Option<CancelToken>,
    pub deep_cleaning: bool,
    /// A Files tab search (large/old or duplicates) in progress.
    pub finding: Option<CancelToken>,
    pub found: Option<Arc<LoadedFind>>,
    pub dupes: Option<Arc<LoadedDupes>>,
}

impl Inner {
    pub fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
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
