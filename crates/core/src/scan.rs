//! Scan orchestration: picks the MFT reader or the parallel walker, reports
//! progress, and returns the tree with a summary.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::platform::{self, VolumeSpace};
use crate::progress::{CancelToken, Counters, ScanPhase, ScanProgress};
use crate::tree::Tree;
use crate::walk::{self, Skipped, WalkOptions};
use crate::{Error, Result};

/// How a scan read the disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanMethod {
    /// Read the NTFS Master File Table directly (fast, needs admin).
    Mft,
    /// Parallel directory walk.
    Walk,
}

/// Options for [`scan`].
#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Try the MFT reader first when possible (Windows, NTFS, admin).
    pub allow_mft: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self { allow_mft: true }
    }
}

/// Totals and diagnostics for a finished scan.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub root: String,
    pub method: ScanMethod,
    /// Why the MFT reader was not used, when it was not.
    pub mft_unavailable: Option<String>,
    pub started_at: i64,
    pub duration_ms: u64,
    pub size: u64,
    pub allocated: u64,
    pub files: u64,
    pub dirs: u64,
    /// Entries that could not be read (permission denied, in use).
    pub skipped: u64,
    pub skipped_details: Vec<Skipped>,
    /// Space of the volume containing the root, when known.
    pub volume: Option<VolumeSpace>,
}

/// A finished scan.
#[derive(Debug)]
pub struct Scan {
    pub tree: Tree,
    pub summary: ScanSummary,
}

/// Scans the folder or drive at `path`.
///
/// `progress` is called about ten times per second from a background
/// thread. Cancelling `cancel` makes the scan return [`Error::Cancelled`].
pub fn scan(
    path: &Path,
    opts: &ScanOptions,
    progress: &(dyn Fn(&ScanProgress) + Sync),
    cancel: &CancelToken,
) -> Result<Scan> {
    let md = std::fs::metadata(path).map_err(|e| Error::io(path, e))?;
    if !md.is_dir() {
        return Err(Error::InvalidInput(format!(
            "{} is not a folder or drive",
            path.display()
        )));
    }
    let root =
        platform::simplify_path(std::fs::canonicalize(path).map_err(|e| Error::io(path, e))?);
    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let started = Instant::now();
    let counters = Counters::new();
    let finished = AtomicBool::new(false);

    let result = std::thread::scope(|s| {
        let ticker = s.spawn(|| {
            while !finished.load(Ordering::Acquire) {
                progress(&counters.snapshot());
                std::thread::park_timeout(Duration::from_millis(100));
            }
            // Always end with the final numbers, so even a scan that finishes
            // before the first tick reports its progress.
            progress(&counters.snapshot());
        });
        let r = run(&root, opts, &counters, cancel);
        finished.store(true, Ordering::Release);
        ticker.thread().unpark();
        r
    });
    let (tree, method, mft_unavailable, skipped) = result?;

    let r = tree.node(tree.root());
    let summary = ScanSummary {
        root: root.display().to_string(),
        method,
        mft_unavailable,
        started_at,
        duration_ms: started.elapsed().as_millis() as u64,
        size: r.size(),
        allocated: r.allocated(),
        files: r.files(),
        dirs: r.dirs(),
        skipped: skipped.count,
        skipped_details: skipped.details,
        volume: platform::volume_space(&root),
    };
    Ok(Scan { tree, summary })
}

type RunResult = (Tree, ScanMethod, Option<String>, walk::SkipLog);

fn run(
    root: &Path,
    opts: &ScanOptions,
    counters: &Counters,
    cancel: &CancelToken,
) -> Result<RunResult> {
    let mft_unavailable = if opts.allow_mft {
        match try_mft(root, counters, cancel) {
            Ok(tree) => return Ok((tree, ScanMethod::Mft, None, walk::SkipLog::default())),
            Err(MftSkip::Cancelled) => return Err(Error::Cancelled),
            Err(MftSkip::Unavailable(reason)) => Some(reason),
        }
    } else {
        Some("Fast MFT scan turned off".to_string())
    };
    counters.set_phase(ScanPhase::Walking);
    let (tree, skipped) = walk::walk(root, &WalkOptions::for_root(root), counters, cancel)?;
    Ok((tree, ScanMethod::Walk, mft_unavailable, skipped))
}

enum MftSkip {
    #[cfg_attr(not(windows), allow(dead_code))]
    Cancelled,
    Unavailable(String),
}

#[cfg(windows)]
fn try_mft(root: &Path, counters: &Counters, cancel: &CancelToken) -> Result<Tree, MftSkip> {
    use crate::platform::DriveKind;
    use std::ffi::OsStr;
    use std::path::Component;

    let unavailable = |s: &str| MftSkip::Unavailable(s.to_string());
    let volume_root = platform::volume_root(root).ok_or_else(|| unavailable("Unknown volume"))?;
    let vr = volume_root.to_string_lossy();
    let b = vr.as_bytes();
    // Only plain drive letters (not folder mount points or network shares).
    if !(b.len() == 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\') {
        return Err(unavailable("Fast MFT scan only works on drive letters"));
    }
    if !matches!(
        platform::drive_kind(&volume_root),
        DriveKind::Fixed | DriveKind::Removable
    ) {
        return Err(unavailable("Fast MFT scan only works on local drives"));
    }
    if platform::file_system(&volume_root).as_deref() != Some("NTFS") {
        return Err(unavailable("Fast MFT scan only works on NTFS drives"));
    }
    let volume = match platform::open_volume(b[0] as char) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(unavailable("Fast MFT scan needs administrator rights"));
        }
        Err(e) => {
            return Err(MftSkip::Unavailable(format!(
                "Could not open the volume: {e}"
            )));
        }
    };
    let rel = root.strip_prefix(&volume_root).unwrap_or(Path::new(""));
    let components: Vec<&OsStr> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n),
            _ => None,
        })
        .collect();
    match crate::mft::read_tree_with(volume, volume_root.clone(), &components, counters, cancel) {
        Ok(Some((tree, _stats))) => Ok(tree),
        Ok(None) => Err(unavailable(
            "Folder not found in the MFT (it may be a link)",
        )),
        Err(Error::Cancelled) => Err(MftSkip::Cancelled),
        Err(e) => Err(MftSkip::Unavailable(format!("MFT could not be read: {e}"))),
    }
}

#[cfg(not(windows))]
fn try_mft(_root: &Path, _counters: &Counters, _cancel: &CancelToken) -> Result<Tree, MftSkip> {
    Err(MftSkip::Unavailable(
        "Fast MFT scan is only available on Windows".to_string(),
    ))
}

/// Returns the path a scan of `path` would use as its root.
pub fn resolve_root(path: &Path) -> Result<PathBuf> {
    Ok(platform::simplify_path(
        std::fs::canonicalize(path).map_err(|e| Error::io(path, e))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests write fresh files to temp folders; the walker sees them
    /// immediately, while the on-disk MFT may lag behind, so tests that check
    /// exact numbers always walk.
    const WALK_ONLY: ScanOptions = ScanOptions { allow_mft: false };
    use std::sync::Mutex;

    #[test]
    fn scans_a_folder_and_reports_progress() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..50 {
            let d = tmp.path().join(format!("d{i}"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("f.bin"), vec![0u8; 1000 + i]).unwrap();
        }
        let updates = Mutex::new(0usize);
        let scan = scan(
            tmp.path(),
            &WALK_ONLY,
            &|_p| *updates.lock().unwrap() += 1,
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(scan.summary.files, 50);
        assert_eq!(scan.summary.dirs, 50);
        assert_eq!(scan.summary.size, (0..50).map(|i| 1000 + i).sum::<u64>());
        assert!(*updates.lock().unwrap() >= 1);
        assert_eq!(scan.summary.method, ScanMethod::Walk);
        assert!(scan.summary.mft_unavailable.is_some());
    }

    /// A scan that finishes before the first progress tick must still report
    /// progress, and the last report must carry the final numbers.
    #[test]
    fn very_fast_scans_still_report_final_progress() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("only.bin"), b"x").unwrap();
        for _ in 0..200 {
            let reports = Mutex::new(Vec::new());
            let done = scan(
                tmp.path(),
                &WALK_ONLY,
                &|p| reports.lock().unwrap().push(p.files),
                &CancelToken::new(),
            )
            .unwrap();
            let reports = reports.into_inner().unwrap();
            assert!(!reports.is_empty(), "no progress reported");
            assert_eq!(reports.last(), Some(&done.summary.files));
        }
    }

    #[test]
    fn rejects_files_and_missing_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("file.txt");
        std::fs::write(&f, "x").unwrap();
        let none = |_: &ScanProgress| {};
        assert!(matches!(
            scan(&f, &WALK_ONLY, &none, &CancelToken::new()),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            scan(
                &tmp.path().join("missing"),
                &WALK_ONLY,
                &none,
                &CancelToken::new()
            ),
            Err(Error::Io { .. })
        ));
    }
}
