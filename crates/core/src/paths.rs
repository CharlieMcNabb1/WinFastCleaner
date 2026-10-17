//! Where the app keeps its own data.

use std::io;
use std::path::{Path, PathBuf};

/// The per-user data folder: `%LOCALAPPDATA%\WinFastCleaner` on Windows.
/// Settings, scan snapshots, the recovery index, and logs live here.
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
