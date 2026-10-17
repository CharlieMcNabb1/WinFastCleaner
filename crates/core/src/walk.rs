//! Parallel directory walker: the scan method used without admin rights or
//! on non-NTFS drives.
//!
//! Safety: the walker never follows symlinks, junctions, or mount points.
//! (These are "name-surrogate" reparse points, which the standard library
//! reports as symlinks.) It does descend into other reparse-point folders,
//! such as OneDrive folders, because those are the real folder. It never
//! opens file contents, so cloud placeholders are never downloaded.

use std::collections::HashMap;
use std::fs::{self, DirEntry, Metadata};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::progress::{CancelToken, Counters};
use crate::tree::{NO_NODE, NodeData, NodeId, Tree, TreeBuilder, flags};
use crate::{Error, Result};

/// Options for [`walk`].
#[derive(Clone, Debug)]
pub struct WalkOptions {
    /// Allocation unit used to estimate size on disk on Windows, where a
    /// directory listing reports only the logical size.
    pub cluster_size: u64,
    /// Folders whose files are checked for hard links on Windows. Checking
    /// needs one extra system call per file, so by default it is done only
    /// under the Windows folder, where almost all hard links live. (On other
    /// systems every file is checked, because it costs nothing extra.)
    pub hardlink_probe_roots: Vec<PathBuf>,
}

impl WalkOptions {
    /// Sensible defaults for scanning `root`.
    pub fn for_root(root: &Path) -> Self {
        Self {
            cluster_size: crate::platform::cluster_size(root).unwrap_or(4096),
            hardlink_probe_roots: crate::platform::windows_dir().into_iter().collect(),
        }
    }
}

/// An entry that could not be read during a scan.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

/// Keep at most this many skipped entries with details; the rest are only
/// counted.
pub(crate) const MAX_SKIPPED_DETAILS: usize = 200;

#[derive(Debug, Default)]
pub(crate) struct SkipLog {
    pub count: u64,
    pub details: Vec<Skipped>,
}

impl SkipLog {
    pub fn add(&mut self, path: &Path, reason: impl ToString) {
        self.count += 1;
        if self.details.len() < MAX_SKIPPED_DETAILS {
            self.details.push(Skipped {
                path: path.display().to_string(),
                reason: reason.to_string(),
            });
        }
    }
}

struct Shared<'a> {
    builder: Mutex<TreeBuilder>,
    link_groups: Mutex<HashMap<(u64, u64), u32>>,
    skipped: Mutex<SkipLog>,
    counters: &'a Counters,
    cancel: &'a CancelToken,
    opts: &'a WalkOptions,
}

struct Entry {
    name: std::ffi::OsString,
    flags: u16,
    size: u64,
    allocated: u64,
    modified: i64,
    link_key: Option<(u64, u64)>,
    is_dir: bool,
}

/// Walks `root` (which must be a directory) and builds its tree.
pub(crate) fn walk(
    root: &Path,
    opts: &WalkOptions,
    counters: &Counters,
    cancel: &CancelToken,
) -> Result<(Tree, SkipLog)> {
    let mut builder = TreeBuilder::with_capacity(1 << 16, 1 << 20);
    let root_id = builder.push(
        root.as_os_str(),
        NodeData {
            parent: NO_NODE,
            flags: flags::DIR,
            size: 0,
            allocated: 0,
            modified: fs::metadata(root).map(|m| modified_secs(&m)).unwrap_or(0),
            link_group: None,
        },
    );
    let shared = Shared {
        builder: Mutex::new(builder),
        link_groups: Mutex::new(HashMap::new()),
        skipped: Mutex::new(SkipLog::default()),
        counters,
        cancel,
        opts,
    };
    let probe_root = in_probe_roots(root, opts);
    rayon::scope(|s| {
        let shared = &shared;
        s.spawn(move |s| process_dir(s, shared, root.to_path_buf(), root_id, probe_root));
    });
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let builder = shared
        .builder
        .into_inner()
        .unwrap_or_else(|e| e.into_inner());
    let skipped = shared
        .skipped
        .into_inner()
        .unwrap_or_else(|e| e.into_inner());
    Ok((builder.finish(root_id, root.to_path_buf()), skipped))
}

fn in_probe_roots(path: &Path, opts: &WalkOptions) -> bool {
    if !cfg!(windows) {
        return true;
    }
    let p = path.to_string_lossy().to_lowercase();
    opts.hardlink_probe_roots.iter().any(|r| {
        let r = r.to_string_lossy().to_lowercase();
        p == r
            || p.strip_prefix(&r)
                .is_some_and(|rest| rest.starts_with(['\\', '/']))
    })
}

fn process_dir<'s>(
    scope: &rayon::Scope<'s>,
    sh: &'s Shared<'s>,
    path: PathBuf,
    id: NodeId,
    probe_links: bool,
) {
    if sh.cancel.is_cancelled() {
        return;
    }
    sh.counters.set_current(|| path.display().to_string());

    let read = match fs::read_dir(&path) {
        Ok(r) => r,
        Err(e) => {
            if let Ok(mut b) = sh.builder.lock() {
                b.add_flags(id, flags::UNREADABLE);
            }
            if let Ok(mut s) = sh.skipped.lock() {
                s.add(&path, e);
            }
            return;
        }
    };

    let mut entries = Vec::new();
    let (mut files, mut bytes) = (0u64, 0u64);
    for item in read {
        let item = match item {
            Ok(i) => i,
            Err(e) => {
                if let Ok(mut s) = sh.skipped.lock() {
                    s.add(&path, e);
                }
                continue;
            }
        };
        match classify(&item, sh.opts, probe_links) {
            Ok(entry) => {
                if !entry.is_dir {
                    files += 1;
                    bytes += entry.size;
                }
                entries.push(entry);
            }
            Err(e) => {
                if let Ok(mut s) = sh.skipped.lock() {
                    s.add(&item.path(), e);
                }
            }
        }
    }
    sh.counters.files.fetch_add(files, Ordering::Relaxed);
    sh.counters.bytes.fetch_add(bytes, Ordering::Relaxed);

    // Assign link groups before taking the builder lock.
    let groups: Vec<Option<u32>> = {
        let mut map = match sh.link_groups.lock() {
            Ok(m) => m,
            Err(e) => e.into_inner(),
        };
        entries
            .iter()
            .map(|e| {
                e.link_key.map(|k| {
                    let next = map.len() as u32;
                    *map.entry(k).or_insert(next)
                })
            })
            .collect()
    };

    let mut subdirs = Vec::new();
    {
        let mut b = match sh.builder.lock() {
            Ok(b) => b,
            Err(e) => e.into_inner(),
        };
        for (entry, group) in entries.iter().zip(groups) {
            let child = b.push(
                &entry.name,
                NodeData {
                    parent: id,
                    flags: entry.flags,
                    size: entry.size,
                    allocated: entry.allocated,
                    modified: entry.modified,
                    link_group: group,
                },
            );
            if entry.is_dir {
                subdirs.push((child, path.join(&entry.name)));
            }
        }
    }
    sh.counters
        .dirs
        .fetch_add(subdirs.len() as u64, Ordering::Relaxed);

    for (child, child_path) in subdirs {
        let probe = probe_links || in_probe_roots(&child_path, sh.opts);
        scope.spawn(move |s| process_dir(s, sh, child_path, child, probe));
    }
}

fn modified_secs(md: &Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn classify(item: &DirEntry, opts: &WalkOptions, probe_links: bool) -> std::io::Result<Entry> {
    let file_type = item.file_type()?;
    // Never follows links: `DirEntry::metadata` describes the entry itself.
    let md = item.metadata()?;
    let name = item.file_name();
    let mut entry = Entry {
        name,
        flags: attribute_flags(&md),
        size: 0,
        allocated: 0,
        modified: modified_secs(&md),
        link_key: None,
        is_dir: false,
    };
    if file_type.is_symlink() {
        // Symlink, junction, or mount point: shown, never followed.
        entry.flags |= flags::LINK;
        if md.is_dir() || is_dir_link(&md) {
            entry.flags |= flags::DIR;
        }
        return Ok(entry);
    }
    if file_type.is_dir() {
        entry.flags |= flags::DIR;
        entry.is_dir = true;
        return Ok(entry);
    }
    entry.size = md.len();
    let (allocated, cloud) = allocated_size(&item.path(), &md, opts);
    entry.allocated = allocated;
    if cloud {
        entry.flags |= flags::CLOUD;
    }
    if probe_links {
        entry.link_key = hard_link_key(item, &md);
    }
    Ok(entry)
}

#[cfg(windows)]
mod sys {
    use super::*;
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x800;
    const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

    pub fn attribute_flags(md: &Metadata) -> u16 {
        let a = md.file_attributes();
        let mut f = 0;
        if a & FILE_ATTRIBUTE_HIDDEN != 0 {
            f |= flags::HIDDEN;
        }
        if a & FILE_ATTRIBUTE_SYSTEM != 0 {
            f |= flags::SYSTEM;
        }
        f
    }

    pub fn is_dir_link(md: &Metadata) -> bool {
        md.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0
    }

    /// Returns (size on disk, is a cloud placeholder).
    pub fn allocated_size(path: &Path, md: &Metadata, opts: &WalkOptions) -> (u64, bool) {
        let a = md.file_attributes();
        if a & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_OFFLINE) != 0 {
            // Contents live in the cloud; nothing (or almost nothing) is
            // stored locally. Do not touch the file.
            return (0, true);
        }
        let special =
            FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_SPARSE_FILE | FILE_ATTRIBUTE_REPARSE_POINT;
        if a & special != 0
            && let Some(size) = crate::platform::compressed_size(path)
        {
            return (size.div_ceil(opts.cluster_size) * opts.cluster_size, false);
        }
        (
            md.len().div_ceil(opts.cluster_size) * opts.cluster_size,
            false,
        )
    }

    pub fn hard_link_key(item: &DirEntry, _md: &Metadata) -> Option<(u64, u64)> {
        let (links, volume, index) = crate::platform::file_link_info(&item.path())?;
        (links > 1).then_some((volume, index))
    }
}

#[cfg(unix)]
mod sys {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    pub fn attribute_flags(_md: &Metadata) -> u16 {
        0
    }

    pub fn is_dir_link(_md: &Metadata) -> bool {
        false
    }

    pub fn allocated_size(_path: &Path, md: &Metadata, _opts: &WalkOptions) -> (u64, bool) {
        (md.blocks() * 512, false)
    }

    pub fn hard_link_key(_item: &DirEntry, md: &Metadata) -> Option<(u64, u64)> {
        (md.nlink() > 1).then(|| (md.dev(), md.ino()))
    }
}

use sys::{allocated_size, attribute_flags, hard_link_key, is_dir_link};


#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(path: &Path, len: usize) {
        let mut f = fs::File::create(path).unwrap();
        f.write_all(&vec![b'x'; len]).unwrap();
    }

    fn walk_dir(root: &Path) -> (Tree, SkipLog) {
        let counters = Counters::new();
        walk(
            root,
            &WalkOptions::for_root(root),
            &counters,
            &CancelToken::new(),
        )
        .unwrap()
    }

    #[test]
    fn sizes_and_counts_match_the_file_system() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("a/b/c")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        write_file(&root.join("top.bin"), 10_000);
        write_file(&root.join("a/one.bin"), 3_000);
        write_file(&root.join("a/b/two.bin"), 70_000);
        write_file(&root.join("a/b/c/three.bin"), 1);

        let (tree, skipped) = walk_dir(root);
        assert_eq!(skipped.count, 0);
        let r = tree.node(tree.root());
        assert_eq!(r.size(), 83_001);
        assert_eq!(r.files(), 4);
        assert_eq!(r.dirs(), 4);
        assert!(r.allocated() >= r.size());

        let a = tree.find(&root.join("a")).unwrap();
        assert_eq!(tree.node(a).size(), 73_001);
        // Largest child first.
        assert_eq!(tree.name(tree.children(a)[0]), "b");
        let three = tree.find(&root.join("a/b/c/three.bin")).unwrap();
        assert_eq!(tree.path(three), root.join("a/b/c/three.bin"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_listed_but_never_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("scan");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        write_file(&outside.join("big.bin"), 500_000);
        write_file(&root.join("small.bin"), 100);
        std::os::unix::fs::symlink(&outside, root.join("link-to-outside")).unwrap();
        std::os::unix::fs::symlink(root.join("small.bin"), root.join("file-link")).unwrap();
        // A loop back to the scan root must not hang or recurse.
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();

        let (tree, _) = walk_dir(&root);
        assert_eq!(tree.node(tree.root()).size(), 100);
        let link = tree.find(&root.join("link-to-outside")).unwrap();
        assert!(tree.node(link).has(flags::LINK));
        assert_eq!(tree.node(link).size(), 0);
        assert!(tree.children(link).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_are_counted_once() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("a")).unwrap();
        fs::create_dir_all(root.join("b")).unwrap();
        write_file(&root.join("a/data.bin"), 200_000);
        fs::hard_link(root.join("a/data.bin"), root.join("b/data-link.bin")).unwrap();

        let (tree, _) = walk_dir(root);
        let r = tree.node(tree.root());
        assert_eq!(r.size(), 200_000);
        assert_eq!(r.files(), 2);
        assert!(r.has(flags::HAS_HARDLINKS));
        let b = tree.find(&root.join("b")).unwrap();
        assert_eq!(tree.node(b).size(), 0);
        assert!(tree.node(b).has(flags::HAS_HARDLINKS));
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_folders_are_reported_not_fatal() {
        use std::os::unix::fs::PermissionsExt;
        if crate::platform::is_elevated() {
            // Root can read everything, so this case cannot be produced.
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("locked")).unwrap();
        write_file(&root.join("ok.bin"), 10);
        fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
        let (tree, skipped) = walk_dir(root);
        fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(skipped.count, 1);
        let locked = tree.find(&root.join("locked")).unwrap();
        assert!(tree.node(locked).has(flags::UNREADABLE));
        assert!(tree.node(tree.root()).has(flags::HAS_UNREADABLE));
    }

    #[test]
    fn cancellation_stops_the_walk() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("x")).unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        let res = walk(
            tmp.path(),
            &WalkOptions::for_root(tmp.path()),
            &Counters::new(),
            &cancel,
        );
        assert!(matches!(res, Err(Error::Cancelled)));
    }
}
