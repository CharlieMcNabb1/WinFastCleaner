//! Times the MFT reader on an NTFS image or volume:
//! `cargo run --release -p cleaner-core --example mft_bench -- <image-or-\\.\C:>`
use std::time::Instant;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: mft_bench <ntfs image or \\\\.\\C:>");
    let file = std::fs::File::open(&path).expect("open volume or image");
    let started = Instant::now();
    let (tree, stats) = cleaner_core::mft::read_tree(
        file,
        std::path::PathBuf::from(&path),
        &[],
        &cleaner_core::progress::CancelToken::new(),
    )
    .expect("read MFT")
    .expect("root exists");
    let root = tree.node(tree.root());
    println!(
        "{} records ({} in use) -> {} files, {} folders, {} in {:?}",
        stats.records,
        stats.in_use,
        root.files(),
        root.dirs(),
        cleaner_core::format::format_size(root.size()),
        started.elapsed()
    );
}
