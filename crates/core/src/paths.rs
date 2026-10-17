//! Where the app keeps its own data, and safe ways to write it.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// The per-user data folder: `%LOCALAPPDATA%\WinFastCleaner` on Windows.
/// Settings, scan snapshots, the recovery index, rules, and logs live here.
pub fn default_data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    // Ignore relative values: data must never land in the current folder.
    base.filter(|b| b.is_absolute())
        .map(|b| b.join(crate::APP_DIR_NAME))
}

/// Folder for the user's own cleaning rules.
pub fn user_rules_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("rules")
}

/// Creates a private data folder if needed (owner-only on Unix; on Windows
/// `%LOCALAPPDATA%` is already private to the user).
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Writes `bytes` to `path` through a temporary file and a rename, so
/// readers never see a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent folder"))?;
    ensure_private_dir(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
