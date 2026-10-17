//! Large and old file finder.
//!
//! It works on a finished scan's tree, so after a scan it costs almost
//! nothing. Everything it finds is the user's own files: red tier, never
//! selected automatically, and always cleaned through the recovery bin
//! (`cleaner::plan_manual`). Protected and excluded files are never listed.

use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::cleaner::Context;
use crate::filetypes::{FileKind, kind_of};
use crate::progress::CancelToken;
use crate::tree::{NodeId, Tree, flags};

const MIB: u64 = 1024 * 1024;

/// Files the finders never list: links, second names of hard links, cloud
/// placeholders, system files, NTFS metadata, and unreadable entries.
pub(crate) const NEVER_LISTED: u16 = flags::LINK
    | flags::HARDLINK_DUP
    | flags::CLOUD
    | flags::SYSTEM
    | flags::METAFILE
    | flags::UNREADABLE;

/// What to look for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinderOptions {
    pub min_size: u64,
    /// Only files neither modified nor opened in this many days.
    pub older_than_days: Option<u32>,
    /// Only these types (all types when empty).
    pub kinds: Vec<FileKind>,
    /// At most this many files, largest first.
    pub limit: usize,
}

impl FinderOptions {
    /// The spec's defaults for large files: the 200 largest over 100 MB.
    pub fn large() -> FinderOptions {
        FinderOptions {
            min_size: 100 * MIB,
            older_than_days: None,
            kinds: Vec::new(),
            limit: 200,
        }
    }

    /// Defaults for old files: unused for a year, 1 MB or more.
    pub fn old() -> FinderOptions {
        FinderOptions {
            min_size: MIB,
            older_than_days: Some(365),
            kinds: Vec::new(),
            limit: 500,
        }
    }
}

/// A file the finder found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundFile {
    pub path: PathBuf,
    pub size: u64,
    pub modified: i64,
    /// The later of modified and last opened. Windows may not record when a
    /// file was last opened, in which case this is the modified time.
    pub last_used: i64,
    pub kind: FileKind,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinderResult {
    pub files: Vec<FoundFile>,
    /// Matching files not shown because of the limit (approximate).
    pub more: u64,
    /// Matching files left out because they are protected or excluded.
    pub hidden: u64,
    /// Old-file matches left out because they were opened recently.
    pub recently_opened: u64,
}

/// Finds large (and optionally old) files in a scan.
pub fn find(
    tree: &Tree,
    opts: &FinderOptions,
    ctx: &Context,
    cancel: &CancelToken,
) -> FinderResult {
    let cutoff = opts
        .older_than_days
        .map(|d| ctx.now - i64::from(d) * 86_400);
    let mut candidates: Vec<(u64, NodeId)> = Vec::new();
    for id in 0..tree.len() as NodeId {
        let n = tree.node(id);
        if n.is_dir() || n.flags() & NEVER_LISTED != 0 || n.size() < opts.min_size {
            continue;
        }
        if cutoff.is_some_and(|c| n.modified() >= c) {
            continue;
        }
        if !opts.kinds.is_empty() && !opts.kinds.contains(&kind_of(&tree.name(id))) {
            continue;
        }
        candidates.push((n.size(), id));
    }
    candidates.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut result = FinderResult::default();
    for (i, &(_, id)) in candidates.iter().enumerate() {
        if result.files.len() >= opts.limit || cancel.is_cancelled() {
            result.more = (candidates.len() - i) as u64;
            break;
        }
        let path = tree.path(id);
        if ctx.guard.check(&path).is_err() || ctx.exclusions.is_excluded(&path) {
            result.hidden += 1;
            continue;
        }
        // Fresh facts from the disk (the scan may be a few minutes old).
        let Ok(md) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !md.is_file() || md.len() < opts.min_size {
            continue;
        }
        let modified = secs(md.modified());
        let last_used = modified.max(secs(md.accessed()));
        if cutoff.is_some_and(|c| last_used >= c) {
            result.recently_opened += 1;
            continue;
        }
        result.files.push(FoundFile {
            kind: kind_of(&tree.name(id)),
            path,
            size: md.len(),
            modified,
            last_used,
        });
    }
    result
}

fn secs(t: std::io::Result<std::time::SystemTime>) -> i64 {
    t.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::CancelToken;
    use crate::rules::Vars;
    use crate::safety::Guard;
    use crate::scan::{ScanOptions, scan};
    use crate::settings::{Exclusion, ExclusionKind, Exclusions};

    fn ctx(exclusions: &[Exclusion]) -> Context {
        Context {
            guard: Guard::system(),
            vars: Vars::default(),
            exclusions: Exclusions::new(exclusions),
            elevated: false,
            running: Default::default(),
            now: crate::scan::now_unix(),
        }
    }

    fn file(dir: &std::path::Path, name: &str, len: u64, days_old: u64) -> PathBuf {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let f = std::fs::File::create(&p).unwrap();
        f.set_len(len).unwrap();
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(days_old * 86_400);
        f.set_times(std::fs::FileTimes::new().set_modified(t).set_accessed(t))
            .unwrap();
        std::fs::canonicalize(p).unwrap()
    }

    #[test]
    fn finds_the_largest_and_the_oldest() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let big = file(root, "videos/big.mkv", 300 * MIB, 0);
        let mid = file(root, "setup.exe", 150 * MIB, 800);
        file(root, "small.txt", 10, 800);
        let old = file(root, "docs/old.pdf", 2 * MIB, 400);
        let keep = file(root, "keep/huge.iso", 500 * MIB, 900);
        let tree = scan(
            root,
            &ScanOptions { allow_mft: false },
            &|_| {},
            &CancelToken::new(),
        )
        .unwrap()
        .tree;
        let c = ctx(&[Exclusion {
            kind: ExclusionKind::Folder,
            value: keep.parent().unwrap().display().to_string(),
        }]);

        let large = find(&tree, &FinderOptions::large(), &c, &CancelToken::new());
        let paths: Vec<_> = large.files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(
            paths,
            vec![big.clone(), mid.clone()],
            "largest first, excluded left out"
        );
        assert_eq!(large.hidden, 1);
        assert_eq!(large.files[0].kind, FileKind::Video);

        let olds = find(&tree, &FinderOptions::old(), &c, &CancelToken::new());
        let paths: Vec<_> = olds.files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(
            paths,
            vec![mid.clone(), old],
            "only old files, 1 MB or more"
        );

        let installers = FinderOptions {
            kinds: vec![FileKind::Installer],
            ..FinderOptions::large()
        };
        let only = find(&tree, &installers, &c, &CancelToken::new());
        assert_eq!(only.files.len(), 1);
        assert_eq!(only.files[0].path, mid);

        let one = FinderOptions {
            limit: 1,
            ..FinderOptions::large()
        };
        let r = find(&tree, &one, &c, &CancelToken::new());
        assert_eq!((r.files.len(), r.more), (1, 1));
    }
}
