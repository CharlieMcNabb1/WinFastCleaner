//! Cancellation and progress reporting for long operations.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::Serialize;

/// Cheap, clonable flag that asks a running operation to stop.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// What a scan is doing right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    Starting,
    /// Walking directories.
    Walking,
    /// Reading the NTFS Master File Table.
    ReadingMft,
    /// Building the folder tree and totals.
    Building,
}

/// A progress update, sent roughly ten times per second.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub phase: ScanPhase,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    /// For MFT scans: records processed so far and in total.
    pub done: u64,
    pub total: u64,
    /// The folder currently being read (walker only).
    pub current: Option<String>,
}

/// Shared counters updated by scanner threads.
#[derive(Debug)]
pub(crate) struct Counters {
    pub phase: Mutex<ScanPhase>,
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub current: Mutex<Option<String>>,
}

impl Counters {
    pub fn new() -> Self {
        Self {
            phase: Mutex::new(ScanPhase::Starting),
            files: AtomicU64::new(0),
            dirs: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
            current: Mutex::new(None),
        }
    }

    pub fn set_phase(&self, phase: ScanPhase) {
        if let Ok(mut p) = self.phase.lock() {
            *p = phase;
        }
    }

    /// Records the folder being read. Skips the update if another thread
    /// holds the lock, since this is only informational.
    pub fn set_current(&self, path: impl FnOnce() -> String) {
        if let Ok(mut cur) = self.current.try_lock() {
            *cur = Some(path());
        }
    }

    pub fn snapshot(&self) -> ScanProgress {
        ScanProgress {
            phase: self.phase.lock().map(|p| *p).unwrap_or(ScanPhase::Starting),
            files: self.files.load(Ordering::Relaxed),
            dirs: self.dirs.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            current: self.current.lock().ok().and_then(|c| c.clone()),
        }
    }
}
