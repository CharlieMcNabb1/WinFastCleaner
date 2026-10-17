//! The local cleanup log: one JSON line per file acted on. It stays on this
//! machine, holds only paths, sizes, tiers, and results, and is rotated so
//! it never grows without limit.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::rules::Tier;

const FILE_NAME: &str = "cleanup.log";
const MAX_BYTES: u64 = 5 * 1024 * 1024;
const KEEP_OLD: u32 = 3;

/// What happened to a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Deleted,
    /// Overwritten, then deleted (secure delete).
    Wiped,
    MovedToBin,
    Skipped,
    Restored,
    Purged,
}

/// One log line.
#[derive(Clone, Debug, Serialize)]
pub struct Entry<'a> {
    pub time: i64,
    pub batch: Option<&'a str>,
    pub action: Action,
    pub tier: Option<Tier>,
    pub path: &'a Path,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<&'a str>,
}

pub fn log_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

/// Appends entries to the log, rotating first if it is too big. Logging
/// problems never stop a cleanup, so errors are ignored by callers.
pub fn append(data_dir: &Path, entries: &[Entry<'_>]) -> std::io::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let dir = log_dir(data_dir);
    crate::paths::ensure_private_dir(&dir)?;
    let path = dir.join(FILE_NAME);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        rotate(&dir)?;
    }
    let mut buf = Vec::with_capacity(entries.len() * 160);
    for e in entries {
        serde_json::to_writer(&mut buf, e).map_err(std::io::Error::other)?;
        buf.push(b'\n');
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    f.write_all(&buf)
}

fn rotate(dir: &Path) -> std::io::Result<()> {
    let name = |i: u32| dir.join(format!("cleanup.{i}.log"));
    let _ = std::fs::remove_file(name(KEEP_OLD));
    for i in (1..KEEP_OLD).rev() {
        let _ = std::fs::rename(name(i), name(i + 1));
    }
    std::fs::rename(dir.join(FILE_NAME), name(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_and_rotates() {
        let tmp = tempfile::tempdir().unwrap();
        let p = PathBuf::from("/x/file.tmp");
        let entry = Entry {
            time: 1,
            batch: Some("b1"),
            action: Action::Deleted,
            tier: Some(Tier::Green),
            path: &p,
            size: 10,
            detail: None,
        };
        append(tmp.path(), &[entry.clone(), entry.clone()]).unwrap();
        let text = std::fs::read_to_string(log_dir(tmp.path()).join(FILE_NAME)).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains(r#""action":"deleted""#));

        // Force a rotation.
        std::fs::write(
            log_dir(tmp.path()).join(FILE_NAME),
            vec![b'x'; (MAX_BYTES + 1) as usize],
        )
        .unwrap();
        append(tmp.path(), &[entry]).unwrap();
        assert!(log_dir(tmp.path()).join("cleanup.1.log").exists());
        let fresh = std::fs::read_to_string(log_dir(tmp.path()).join(FILE_NAME)).unwrap();
        assert_eq!(fresh.lines().count(), 1);
    }
}
