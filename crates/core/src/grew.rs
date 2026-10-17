//! "What grew": compares two scan snapshots of the same root.
//!
//! Snapshots only record folders of at least their threshold (50 MB by
//! default), so a folder missing from one side was smaller than that (or
//! gone). The comparison treats it as 0 there and says so.
//!
//! Growth is reported at the most specific folder that explains it: when one
//! subfolder accounts for most of a folder's change, the subfolder is listed
//! instead of every folder above it ("Discord cache +3.1 GB", not "C:\Users
//! +3.1 GB", "C:\Users\you +3.1 GB", …).

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::snapshot::Snapshot;

/// A child explaining at least this share of its parent's change replaces
/// the parent in the list.
const EXPLAINED_SHARE: f64 = 0.75;

/// One folder that changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// Full path.
    pub path: String,
    /// Size in the older snapshot (`None`: smaller than its threshold).
    pub old_size: Option<u64>,
    /// Size in the newer snapshot (`None`: smaller than its threshold, or gone).
    pub new_size: Option<u64>,
    pub delta: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub root: String,
    pub old_taken_at: i64,
    pub new_taken_at: i64,
    /// Change of the whole root.
    pub total_delta: i64,
    /// Largest growth first.
    pub grew: Vec<Change>,
    /// Largest shrink first.
    pub shrank: Vec<Change>,
    /// Folders below this size are not recorded.
    pub threshold: u64,
}

/// Compares `old` with `new` and returns at most `limit` entries each way,
/// ignoring changes smaller than `min_delta` bytes.
pub fn compare(old: &Snapshot, new: &Snapshot, limit: usize, min_delta: u64) -> Comparison {
    let old_map: HashMap<&str, u64> = old
        .folders
        .iter()
        .map(|f| (f.path.as_str(), f.size))
        .collect();
    let new_map: HashMap<&str, u64> = new
        .folders
        .iter()
        .map(|f| (f.path.as_str(), f.size))
        .collect();
    let mut deltas: BTreeMap<&str, (Option<u64>, Option<u64>, i64)> = BTreeMap::new();
    for path in old_map.keys().chain(new_map.keys()) {
        let o = old_map.get(path).copied();
        let n = new_map.get(path).copied();
        let delta = n.unwrap_or(0) as i64 - o.unwrap_or(0) as i64;
        deltas.insert(path, (o, n, delta));
    }

    // The largest same-direction change among each folder's children.
    let mut child_max: HashMap<&str, (i64, i64)> = HashMap::new();
    for (path, (_, _, d)) in &deltas {
        if path.is_empty() {
            continue;
        }
        let parent = parent_of(path);
        let e = child_max.entry(parent).or_insert((0, 0));
        e.0 = e.0.max(*d);
        e.1 = e.1.min(*d);
    }

    let sep = std::path::MAIN_SEPARATOR;
    let mut changes: Vec<Change> = deltas
        .iter()
        .filter(|(path, (_, _, d))| {
            if path.is_empty() || d.unsigned_abs() < min_delta {
                return false;
            }
            let (up, down) = child_max.get(*path).copied().unwrap_or((0, 0));
            let explained = if *d > 0 {
                up as f64 >= EXPLAINED_SHARE * *d as f64
            } else {
                (down as f64) <= EXPLAINED_SHARE * *d as f64
            };
            !explained
        })
        .map(|(path, (o, n, d))| Change {
            path: join(&new.root, path, sep),
            old_size: *o,
            new_size: *n,
            delta: *d,
        })
        .collect();
    changes.sort_by(|a, b| b.delta.cmp(&a.delta).then(a.path.cmp(&b.path)));
    let grew: Vec<Change> = changes
        .iter()
        .filter(|c| c.delta > 0)
        .take(limit)
        .cloned()
        .collect();
    let shrank: Vec<Change> = changes
        .iter()
        .rev()
        .filter(|c| c.delta < 0)
        .take(limit)
        .cloned()
        .collect();
    Comparison {
        root: new.root.clone(),
        old_taken_at: old.taken_at,
        new_taken_at: new.taken_at,
        total_delta: new.size as i64 - old.size as i64,
        grew,
        shrank,
        threshold: old.min_folder_size.max(new.min_folder_size),
    }
}

fn parent_of(path: &str) -> &str {
    match path.rfind(['\\', '/']) {
        Some(i) => &path[..i],
        None => "",
    }
}

fn join(root: &str, rel: &str, sep: char) -> String {
    if rel.is_empty() {
        root.to_string()
    } else if root.ends_with(['\\', '/']) {
        format!("{root}{rel}")
    } else {
        format!("{root}{sep}{rel}")
    }
}

/// Whether two snapshot roots are the same folder (case-insensitive, either
/// separator, trailing separator ignored).
pub fn same_root(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s = s
            .strip_prefix(r"\\?\")
            .or_else(|| s.strip_prefix("//?/"))
            .unwrap_or(s);
        s.trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_lowercase()
    };
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::ScanMethod;
    use crate::snapshot::SnapshotFolder;

    const MB: u64 = 1024 * 1024;

    fn snap(taken_at: i64, folders: &[(&str, u64)]) -> Snapshot {
        Snapshot {
            format: 1,
            app_version: "test".into(),
            root: r"C:\".into(),
            taken_at,
            method: ScanMethod::Walk,
            size: folders.iter().find(|f| f.0.is_empty()).map_or(0, |f| f.1),
            allocated: 0,
            files: 0,
            dirs: 0,
            min_folder_size: 50 * MB,
            folders: folders
                .iter()
                .map(|(p, s)| SnapshotFolder {
                    path: p.replace('\\', std::path::MAIN_SEPARATOR_STR),
                    size: *s,
                    allocated: *s,
                    files: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn growth_is_shown_where_it_happened() {
        let old = snap(
            1_000,
            &[
                ("", 10_000 * MB),
                ("Users", 6_000 * MB),
                (r"Users\me", 6_000 * MB),
                (r"Users\me\AppData", 2_000 * MB),
                (r"Users\me\AppData\Roaming", 1_000 * MB),
                (r"Users\me\AppData\Roaming\discord", 100 * MB),
                (r"Users\me\Videos", 3_000 * MB),
                ("Games", 1_000 * MB),
            ],
        );
        let new = snap(
            2_000,
            &[
                ("", 13_500 * MB),
                ("Users", 8_100 * MB),
                (r"Users\me", 8_100 * MB),
                (r"Users\me\AppData", 5_100 * MB),
                (r"Users\me\AppData\Roaming", 4_100 * MB),
                (r"Users\me\AppData\Roaming\discord", 3_200 * MB),
                (r"Users\me\Videos", 2_000 * MB),
                ("Games", 1_000 * MB),
                ("NewApp", 400 * MB),
            ],
        );
        let c = compare(&old, &new, 10, 10 * MB);
        assert_eq!(c.total_delta, 3_500 * MB as i64);
        let sep = std::path::MAIN_SEPARATOR;
        let grew: Vec<(String, i64)> = c
            .grew
            .iter()
            .map(|g| (g.path.clone(), g.delta / MB as i64))
            .collect();
        assert_eq!(
            grew,
            vec![
                (
                    format!(r"C:\Users{sep}me{sep}AppData{sep}Roaming{sep}discord"),
                    3_100
                ),
                (r"C:\NewApp".to_string(), 400),
            ],
            "parents explained by discord are hidden"
        );
        assert_eq!(c.grew[1].old_size, None, "was under the threshold");
        assert_eq!(c.shrank.len(), 1);
        assert_eq!(c.shrank[0].path, format!(r"C:\Users{sep}me{sep}Videos"));
        assert_eq!(c.shrank[0].delta, -1_000 * MB as i64);
    }

    #[test]
    fn roots_compare_loosely() {
        assert!(same_root(r"C:\", "c:"));
        assert!(same_root(r"D:\Projects\", "d:/projects"));
        assert!(same_root(r"\\?\C:\Users\a", r"c:\users\A\"));
        assert!(!same_root(r"C:\", r"D:\"));
    }
}
