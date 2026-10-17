//! Windows only: compares the fast MFT scan with the folder walker on a real
//! system folder. Runs when the tests are elevated (as on GitHub's Windows
//! runners); otherwise it is skipped with a message.
#![cfg(windows)]

use std::path::PathBuf;

use cleaner_core::platform;
use cleaner_core::progress::CancelToken;
use cleaner_core::scan::{ScanMethod, ScanOptions, scan};
use cleaner_core::tree::flags;

fn within_one_percent(a: u64, b: u64) -> bool {
    let (a, b) = (a as f64, b as f64);
    (a - b).abs() <= b.max(1.0) * 0.01
}

#[test]
fn mft_scan_matches_walker_on_system_folder() {
    if !platform::is_elevated() {
        eprintln!("skipping: the MFT scan needs administrator rights");
        return;
    }
    // A folder that does not change while the tests run.
    let folder: PathBuf = platform::windows_dir()
        .expect("Windows folder")
        .join("System32")
        .join("drivers");
    let none = |_: &_| {};
    let mft = scan(
        &folder,
        &ScanOptions { allow_mft: true },
        &none,
        &CancelToken::new(),
    )
    .unwrap();
    let walk = scan(
        &folder,
        &ScanOptions { allow_mft: false },
        &none,
        &CancelToken::new(),
    )
    .unwrap();

    assert_eq!(
        mft.summary.method,
        ScanMethod::Mft,
        "{:?}",
        mft.summary.mft_unavailable
    );
    assert_eq!(walk.summary.method, ScanMethod::Walk);
    eprintln!(
        "MFT: {} files, {} bytes in {} ms; walk: {} files, {} bytes in {} ms",
        mft.summary.files,
        mft.summary.size,
        mft.summary.duration_ms,
        walk.summary.files,
        walk.summary.size,
        walk.summary.duration_ms
    );
    // Some system folders deny listing even to administrators (for example
    // Defender's). The MFT still sees their files, so leave those folders
    // out of the comparison.
    let (mut mft_files, mut mft_size) = (mft.summary.files, mft.summary.size);
    for id in 0..walk.tree.len() as u32 {
        if walk.tree.node(id).has(flags::UNREADABLE)
            && let Some(m) = mft.tree.find(&walk.tree.path(id))
        {
            mft_files -= mft.tree.node(m).files();
            mft_size -= mft.tree.node(m).size();
        }
    }
    assert!(
        within_one_percent(mft_files, walk.summary.files),
        "files: MFT {mft_files} vs walk {}",
        walk.summary.files
    );
    assert!(
        within_one_percent(mft_size, walk.summary.size),
        "size: MFT {mft_size} vs walk {}",
        walk.summary.size
    );
}

#[test]
fn mft_scan_of_whole_system_drive() {
    if !platform::is_elevated() {
        eprintln!("skipping: the MFT scan needs administrator rights");
        return;
    }
    let drive = platform::windows_dir()
        .and_then(|w| platform::volume_root(&w))
        .expect("system drive");
    let done = scan(
        &drive,
        &ScanOptions::default(),
        &|_| {},
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(done.summary.method, ScanMethod::Mft);
    let space = done.summary.volume.expect("volume space");
    let used = space.total - space.free;
    eprintln!(
        "{}: {} files, {} bytes on disk (volume used: {used}) in {} ms",
        done.summary.root, done.summary.files, done.summary.allocated, done.summary.duration_ms
    );
    // Allocated bytes of all files cannot exceed the space in use, and should
    // account for most of it (the rest is folder indexes and NTFS internals).
    assert!(done.summary.allocated <= used + used / 50);
    assert!(done.summary.allocated >= used / 2);
}
