//! Lightweight scan snapshots, saved after each scan so a later scan can show
//! which folders grew ("What grew", Phase 3).
//!
//! A snapshot keeps only folder paths and sizes for folders above a size
//! threshold, never file names or contents. Snapshots stay on this machine.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

use crate::format::unix_to_utc;
use crate::scan::{Scan, ScanMethod};
use crate::tree::NodeId;
use crate::{Error, Result};

/// Folders smaller than this are left out of snapshots.
pub const DEFAULT_MIN_FOLDER_SIZE: u64 = 50 * 1024 * 1024;
/// How deep below the scan root folders are recorded.
pub const DEFAULT_MAX_DEPTH: usize = 12;
/// Snapshots older than this are deleted.
pub const DEFAULT_RETENTION_DAYS: u32 = 90;

const FORMAT_VERSION: u32 = 1;
const SUFFIX: &str = ".snapshot.json";
/// Refuse to load files larger than this (a snapshot is normally a few
/// hundred KB at most).
const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;

/// One recorded folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFolder {
    /// Path relative to the snapshot root ("" for the root itself).
    pub path: String,
    pub size: u64,
    pub allocated: u64,
    pub files: u64,
}

/// Folder sizes from one scan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub format: u32,
    pub app_version: String,
    pub root: String,
    pub taken_at: i64,
    pub method: ScanMethod,
    pub size: u64,
    pub allocated: u64,
    pub files: u64,
    pub dirs: u64,
    pub min_folder_size: u64,
    pub folders: Vec<SnapshotFolder>,
}

/// A snapshot found on disk.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    pub file: PathBuf,
    pub root: String,
    pub taken_at: i64,
    pub size: u64,
}

impl Snapshot {
    /// Records folders of at least `min_folder_size`, down to `max_depth`.
    pub fn from_scan(scan: &Scan, min_folder_size: u64, max_depth: usize) -> Self {
        let tree = &scan.tree;
        let sep = std::path::MAIN_SEPARATOR;
        let mut folders = Vec::new();
        let mut stack: Vec<(NodeId, String, usize)> = vec![(tree.root(), String::new(), 0)];
        while let Some((id, path, depth)) = stack.pop() {
            let node = tree.node(id);
            folders.push(SnapshotFolder {
                path: path.clone(),
                size: node.size(),
                allocated: node.allocated(),
                files: node.files(),
            });
            if depth >= max_depth {
                continue;
            }
            for &c in tree.children(id) {
                let child = tree.node(c);
                // Children are sorted by size, so stop at the first small one.
                if child.size() < min_folder_size {
                    break;
                }
                if child.is_dir() {
                    let name = tree.name(c);
                    let p = if path.is_empty() {
                        name.into_owned()
                    } else {
                        format!("{path}{sep}{name}")
                    };
                    stack.push((c, p, depth + 1));
                }
            }
        }
        folders.sort_by(|a, b| a.path.cmp(&b.path));
        Snapshot {
            format: FORMAT_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            root: scan.summary.root.clone(),
            taken_at: scan.summary.started_at,
            method: scan.summary.method,
            size: scan.summary.size,
            allocated: scan.summary.allocated,
            files: scan.summary.files,
            dirs: scan.summary.dirs,
            min_folder_size,
            folders,
        }
    }
}

/// Saves a snapshot into `dir` (created if needed) and returns its path.
/// The file is written to a temporary name first and then renamed, so a
/// crash never leaves a half-written snapshot.
pub fn save(dir: &Path, snap: &Snapshot) -> Result<PathBuf> {
    crate::paths::ensure_private_dir(dir).map_err(|e| Error::io(dir, e))?;
    let json = serde_json::to_vec(snap).map_err(|e| Error::Snapshot(e.to_string()))?;
    let (y, mo, d, h, mi, s) = unix_to_utc(snap.taken_at);
    let base = format!(
        "{}-{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z",
        root_key(&snap.root)
    );

    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let tmp = dir.join(format!(
        ".{base}.{}.{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| Error::io(&tmp, e))?;
        f.write_all(&json)
            .and_then(|()| f.sync_all())
            .map_err(|e| Error::io(&tmp, e))?;
    }
    for n in 0..100 {
        let name = if n == 0 {
            format!("{base}{SUFFIX}")
        } else {
            format!("{base}-{n}{SUFFIX}")
        };
        let target = dir.join(name);
        if target.symlink_metadata().is_ok() {
            continue;
        }
        return match fs::rename(&tmp, &target) {
            Ok(()) => Ok(target),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(Error::io(&target, e))
            }
        };
    }
    let _ = fs::remove_file(&tmp);
    Err(Error::Snapshot(
        "too many snapshots with the same timestamp".into(),
    ))
}

/// Loads one snapshot file.
pub fn load(path: &Path) -> Result<Snapshot> {
    let md = fs::symlink_metadata(path).map_err(|e| Error::io(path, e))?;
    if !md.is_file() {
        return Err(Error::Snapshot(format!("{} is not a file", path.display())));
    }
    if md.len() > MAX_SNAPSHOT_BYTES {
        return Err(Error::Snapshot(format!("{} is too large", path.display())));
    }
    let bytes = fs::read(path).map_err(|e| Error::io(path, e))?;
    let snap: Snapshot = serde_json::from_slice(&bytes)
        .map_err(|e| Error::Snapshot(format!("{}: {e}", path.display())))?;
    if snap.format != FORMAT_VERSION {
        return Err(Error::Snapshot(format!(
            "{}: unsupported format {}",
            path.display(),
            snap.format
        )));
    }
    Ok(snap)
}

/// Lists valid snapshots in `dir`, newest first. Unreadable files are
/// skipped.
pub fn list(dir: &Path) -> Result<Vec<SnapshotInfo>> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::io(dir, e)),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_snapshot = entry.file_name().to_string_lossy().ends_with(SUFFIX)
            && entry.file_type().is_ok_and(|t| t.is_file());
        if !is_snapshot {
            continue;
        }
        if let Ok(s) = load(&path) {
            out.push(SnapshotInfo {
                file: path,
                root: s.root,
                taken_at: s.taken_at,
                size: s.size,
            });
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.taken_at));
    Ok(out)
}

/// Deletes this app's snapshot files in `dir` that are older than
/// `retention_days`. Only regular files with the snapshot suffix, directly
/// inside `dir`, are ever removed. Returns how many were deleted.
pub fn prune(dir: &Path, retention_days: u32, now: i64) -> Result<usize> {
    let cutoff = now - i64::from(retention_days) * 86_400;
    let mut removed = 0;
    for info in list(dir)? {
        if info.taken_at >= cutoff {
            continue;
        }
        // Re-check right before deleting: must still be a regular file
        // directly inside `dir`, never a link.
        let ok = info.file.parent() == Some(dir)
            && fs::symlink_metadata(&info.file).is_ok_and(|m| m.is_file());
        if ok && fs::remove_file(&info.file).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// A filesystem-safe, collision-resistant key for a scan root.
fn root_key(root: &str) -> String {
    let mut readable: String = root
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    readable.truncate(40);
    // FNV-1a of the lowercased root keeps distinct roots apart.
    let mut hash: u32 = 0x811c_9dc5;
    for b in root.to_lowercase().bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{readable}-{hash:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests write fresh files to temp folders; the walker sees them
    /// immediately, while the on-disk MFT may lag behind, so tests that check
    /// exact numbers always walk.
    const WALK_ONLY: ScanOptions = ScanOptions { allow_mft: false };
    use crate::progress::CancelToken;
    use crate::scan::{ScanOptions, scan};

    fn sample_scan(dir: &Path) -> Scan {
        fs::create_dir_all(dir.join("big/inner")).unwrap();
        fs::create_dir_all(dir.join("small")).unwrap();
        fs::write(dir.join("big/inner/a.bin"), vec![0u8; 300_000]).unwrap();
        fs::write(dir.join("big/b.bin"), vec![0u8; 200_000]).unwrap();
        fs::write(dir.join("small/c.bin"), vec![0u8; 10]).unwrap();
        scan(dir, &WALK_ONLY, &|_| {}, &CancelToken::new()).unwrap()
    }

    #[test]
    fn records_only_large_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let s = sample_scan(tmp.path());
        let snap = Snapshot::from_scan(&s, 100_000, 12);
        let paths: Vec<_> = snap.folders.iter().map(|f| f.path.as_str()).collect();
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            paths,
            vec!["".to_string(), "big".into(), format!("big{sep}inner")]
        );
        assert_eq!(snap.folders[1].size, 500_000);
        assert_eq!(snap.files, 3);

        let shallow = Snapshot::from_scan(&s, 100_000, 1);
        assert_eq!(shallow.folders.len(), 2);
    }

    #[test]
    fn save_list_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let s = sample_scan(tmp.path());
        let snap = Snapshot::from_scan(&s, 1, 12);
        let a = save(data.path(), &snap).unwrap();
        let b = save(data.path(), &snap).unwrap();
        assert_ne!(a, b, "same-second snapshots must not overwrite each other");
        assert_eq!(load(&a).unwrap(), snap);
        let listed = list(data.path()).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].root, snap.root);
        // No temporary files are left behind.
        let leftovers = fs::read_dir(data.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn prune_removes_only_old_snapshot_files() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let s = sample_scan(tmp.path());
        let now = 2_000_000_000;
        let mut old = Snapshot::from_scan(&s, 1, 12);
        old.taken_at = now - 91 * 86_400;
        let mut recent = old.clone();
        recent.taken_at = now - 10 * 86_400;
        let old_path = save(data.path(), &old).unwrap();
        let recent_path = save(data.path(), &recent).unwrap();
        let unrelated = data.path().join("notes.txt");
        fs::write(&unrelated, "keep me").unwrap();

        assert_eq!(prune(data.path(), 90, now).unwrap(), 1);
        assert!(!old_path.exists());
        assert!(recent_path.exists());
        assert!(unrelated.exists());
    }

    #[cfg(unix)]
    #[test]
    fn prune_never_follows_links() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let s = sample_scan(tmp.path());
        let mut old = Snapshot::from_scan(&s, 1, 12);
        old.taken_at = 1;
        let real = save(outside.path(), &old).unwrap();
        let link = data.path().join("evil.snapshot.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(prune(data.path(), 90, 2_000_000_000).unwrap(), 0);
        assert!(real.exists());
        assert!(link.symlink_metadata().is_ok());
    }

    #[test]
    fn rejects_oversized_or_invalid_files() {
        let data = tempfile::tempdir().unwrap();
        let bad = data.path().join("x.snapshot.json");
        fs::write(&bad, b"{not json").unwrap();
        assert!(load(&bad).is_err());
        assert!(list(data.path()).unwrap().is_empty());
    }

    #[test]
    fn root_keys_are_safe_and_distinct() {
        let a = root_key(r"C:\Users\Test User");
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        );
        assert_ne!(root_key(r"C:\a_b"), root_key(r"C:\a b"));
        assert_eq!(root_key(r"C:\Data"), root_key(r"C:\Data"));
    }
}
