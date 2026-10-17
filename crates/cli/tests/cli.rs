//! End-to-end tests of the `wfc` binary on synthetic folders.

use std::process::Command;

/// The binary, with its data folder pointed at `data` so tests never write
/// snapshots into the real user profile.
fn wfc(data: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wfc"));
    cmd.env("LOCALAPPDATA", data).env("XDG_DATA_HOME", data);
    cmd
}

#[test]
fn scan_prints_largest_entries_first() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("big")).unwrap();
    std::fs::write(tmp.path().join("big/data.bin"), vec![0u8; 300_000]).unwrap();
    std::fs::write(tmp.path().join("small.txt"), b"hello").unwrap();

    let out = wfc(tmp.path())
        .args(["scan", "--quiet", "--no-snapshot", "--no-mft"])
        .arg(tmp.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let big = text.find("big").expect("lists the big folder");
    let small = text.find("small.txt").expect("lists the small file");
    assert!(big < small, "largest first:\n{text}");
    assert!(text.contains("2 files"));
}

#[test]
fn scan_json_is_valid_and_complete() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("a/b")).unwrap();
    std::fs::write(tmp.path().join("a/b/f.bin"), vec![1u8; 4096]).unwrap();

    let data = tempfile::tempdir().unwrap();
    let out = wfc(data.path())
        .args(["scan", "--json", "--quiet", "--no-mft", "--depth", "3"])
        .arg(tmp.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["summary"]["files"], 1);
    assert_eq!(v["summary"]["size"], 4096);
    assert_eq!(v["root"]["children"][0]["name"], "a");
    assert_eq!(
        v["root"]["children"][0]["children"][0]["children"][0]["name"],
        "f.bin"
    );
}

#[test]
fn scan_rejects_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("x.txt");
    std::fs::write(&f, b"x").unwrap();
    let out = wfc(tmp.path())
        .args(["scan", "--quiet"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a folder"));
}

#[test]
fn drives_lists_something() {
    let tmp = tempfile::tempdir().unwrap();
    let out = wfc(tmp.path()).args(["drives", "--json"]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(!v.as_array().unwrap().is_empty());
}
