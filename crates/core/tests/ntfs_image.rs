//! Checks the MFT reader against a real NTFS volume: a synthetic image built
//! by `scripts/ntfs-test-image.sh` and mounted read-only with ntfs-3g.
//!
//! The same folder is scanned twice, once by reading the raw image's MFT and
//! once with the directory walker through the mount. The results must agree.
//!
//! Skipped (with a message) unless `WFC_NTFS_TEST_IMAGE` and
//! `WFC_NTFS_TEST_MOUNT` are set. CI always sets them.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cleaner_core::mft;
use cleaner_core::progress::CancelToken;
use cleaner_core::scan::{ScanOptions, scan};
use cleaner_core::tree::{Tree, flags};

fn image() -> Option<(PathBuf, PathBuf)> {
    let img = std::env::var_os("WFC_NTFS_TEST_IMAGE").map(PathBuf::from);
    let mnt = std::env::var_os("WFC_NTFS_TEST_MOUNT").map(PathBuf::from);
    match (img, mnt) {
        (Some(i), Some(m)) => Some((i, m)),
        _ => {
            eprintln!(
                "skipping NTFS image test: set WFC_NTFS_TEST_IMAGE and WFC_NTFS_TEST_MOUNT \
                 (see scripts/ntfs-test-image.sh)"
            );
            None
        }
    }
}

struct Info {
    is_dir: bool,
    size: u64,
    allocated: u64,
    flags: u16,
}

/// Every node by path relative to the root, skipping NTFS metadata files
/// (which a mounted volume does not list).
fn index(tree: &Tree) -> HashMap<PathBuf, Info> {
    let root = tree.root_path().to_path_buf();
    (1..tree.len() as u32)
        .filter(|&id| {
            !tree
                .lineage(id)
                .iter()
                .any(|&a| tree.node(a).has(flags::METAFILE))
        })
        .map(|id| {
            let n = tree.node(id);
            (
                tree.path(id).strip_prefix(&root).unwrap().to_path_buf(),
                Info {
                    is_dir: n.is_dir(),
                    size: n.size(),
                    allocated: n.allocated(),
                    flags: n.flags(),
                },
            )
        })
        .collect()
}

fn read_mft(img: &Path, mnt: &Path, sub: &[&OsStr]) -> (Tree, mft::MftStats) {
    let file = File::open(img).expect("open NTFS image");
    let started = Instant::now();
    let result = mft::read_tree(file, mnt.to_path_buf(), sub, &CancelToken::new())
        .expect("MFT read succeeds")
        .expect("folder exists in the MFT");
    eprintln!(
        "MFT read: {} records ({} in use) in {:?}",
        result.1.records,
        result.1.in_use,
        started.elapsed()
    );
    result
}

fn walk(path: &Path) -> Tree {
    scan(
        path,
        &ScanOptions { allow_mft: false },
        &|_| {},
        &CancelToken::new(),
    )
    .expect("walk succeeds")
    .tree
}

#[test]
fn mft_matches_walker_on_ntfs_image() {
    let Some((img, mnt)) = image() else { return };
    let (mft_tree, stats) = read_mft(&img, &mnt, &[]);
    let walk_tree = walk(&mnt);
    assert_eq!(stats.corrupt, 0);
    assert_eq!(stats.orphans, 0);

    let from_mft = index(&mft_tree);
    let from_walk = index(&walk_tree);
    for (path, w) in &from_walk {
        let m = from_mft
            .get(path)
            .unwrap_or_else(|| panic!("{} missing from the MFT tree", path.display()));
        // Through a Linux mount a junction looks like a plain symlink, so
        // folder-ness is only comparable for non-links.
        if m.flags & flags::LINK == 0 {
            assert_eq!(m.is_dir, w.is_dir, "{}: folder vs file", path.display());
        }
        assert_eq!(m.size, w.size, "{}: logical size", path.display());
        assert_eq!(
            m.flags & (flags::LINK | flags::HARDLINK | flags::HARDLINK_DUP | flags::HAS_HARDLINKS),
            w.flags & (flags::LINK | flags::HARDLINK | flags::HARDLINK_DUP | flags::HAS_HARDLINKS),
            "{}: link flags",
            path.display()
        );
        // Resident files (stored inside the MFT record) take no clusters;
        // ntfs-3g reports a nominal block count for them, so only compare
        // files that live in clusters.
        if !m.is_dir && m.allocated > 0 && path.file_name() != Some(OsStr::new("notes.md")) {
            assert_eq!(m.allocated, w.allocated, "{}: size on disk", path.display());
        }
    }
    let extra: Vec<_> = from_mft
        .keys()
        .filter(|p| !from_walk.contains_key(*p))
        .collect();
    assert!(extra.is_empty(), "only in the MFT tree: {extra:?}");

    // Totals agree once NTFS metadata files are left out.
    let metadata_size: u64 = mft_tree
        .children(0)
        .iter()
        .filter(|&&c| mft_tree.node(c).has(flags::METAFILE))
        .map(|&c| mft_tree.node(c).size())
        .sum();
    assert_eq!(
        mft_tree.node(0).size() - metadata_size,
        walk_tree.node(0).size()
    );
    assert!(metadata_size > 0, "$MFT, $LogFile and friends are counted");
}

#[test]
fn mft_handles_special_files() {
    let Some((img, mnt)) = image() else { return };
    let (tree, _) = read_mft(&img, &mnt, &[]);
    let node = |p: &str| {
        tree.node(
            tree.find(&mnt.join(p))
                .unwrap_or_else(|| panic!("{p} not found")),
        )
    };

    // Hard links: the clip is counted once across docs/ and media/.
    let clip = 5 * 1024 * 1024;
    assert_eq!(
        node("docs/clip-link.mp4").size() + node("media/video/clip.mp4").size(),
        clip
    );
    assert!(node("docs").has(flags::HAS_HARDLINKS));

    // Twelve long names plus the original overflow one record, so some of
    // them live in extension records. All 13 names must be found.
    let links = tree.find(&mnt.join("links")).unwrap();
    let names = tree
        .children(links)
        .iter()
        .filter(|&&c| tree.node(c).has(flags::HARDLINK))
        .count();
    assert_eq!(names, 13);
    assert_eq!(tree.node(links).size(), 50_000);

    // Junctions and symlinks are listed, never followed.
    for link in ["links/docs-junction", "links/song-symlink.flac"] {
        let n = node(link);
        assert!(n.has(flags::LINK), "{link} is a link");
        assert_eq!(n.size(), 0);
    }
    let junction = tree.find(&mnt.join("links/docs-junction")).unwrap();
    assert!(tree.children(junction).is_empty());

    // Sparse file: 40 MB logical, almost nothing allocated.
    let sparse = node("media/sparse.vhd");
    assert_eq!(sparse.size(), 40 * 1024 * 1024);
    assert!(
        sparse.allocated() <= 64 * 1024,
        "sparse allocated = {}",
        sparse.allocated()
    );

    // Alternate data streams count toward size on disk, not logical size.
    let notes = node("docs/notes.md");
    assert_eq!(notes.size(), 600);
    assert!(notes.allocated() >= 60_000);

    // Unicode names survive.
    assert_eq!(node("unicode/Café ☕/名前.txt").size(), 4242);
    assert_eq!(node("many").files(), 3000);
}

#[test]
fn mft_subfolder_matches_walker() {
    let Some((img, mnt)) = image() else { return };
    let (sub, _) = read_mft(&img, &mnt, &[OsStr::new("DOCS")]);
    let walked = walk(&mnt.join("docs"));
    assert_eq!(sub.node(0).size(), walked.node(0).size());
    assert_eq!(sub.node(0).files(), walked.node(0).files());
    // Inside the subfolder, the hard link is now the only name, so it carries
    // the size.
    let link = sub.find(&mnt.join("DOCS/clip-link.mp4")).unwrap();
    assert_eq!(sub.node(link).size(), 5 * 1024 * 1024);

    let missing = mft::read_tree(
        File::open(&img).unwrap(),
        mnt.clone(),
        &[OsStr::new("no-such-folder")],
        &CancelToken::new(),
    )
    .unwrap();
    assert!(missing.is_none());
    // Paths through a junction are not resolved by the MFT reader.
    let through_link = mft::read_tree(
        File::open(&img).unwrap(),
        mnt.clone(),
        &[OsStr::new("links"), OsStr::new("docs-junction")],
        &CancelToken::new(),
    )
    .unwrap();
    assert!(through_link.is_none());
}

#[test]
fn mft_read_can_be_cancelled() {
    let Some((img, mnt)) = image() else { return };
    let cancel = CancelToken::new();
    cancel.cancel();
    let res = mft::read_tree(File::open(&img).unwrap(), mnt, &[], &cancel);
    assert!(matches!(res, Err(cleaner_core::Error::Cancelled)));
}

#[test]
fn rejects_non_ntfs_data() {
    let Some(_) = image() else { return };
    let junk = std::io::Cursor::new(vec![0u8; 1 << 20]);
    let res = mft::read_tree(junk, PathBuf::from("/x"), &[], &CancelToken::new());
    assert!(matches!(res, Err(cleaner_core::Error::NotNtfs)));
}
