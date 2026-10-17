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

// ------------------------------------------------------------ cleaning
//
// These tests only ever clean inside temp folders: every `clean` passes
// `--rules-dir` with test rules whose paths point into the temp folder, so
// the built-in rules are never used.

struct Sandbox {
    tmp: tempfile::TempDir,
    /// Plays the role of %LOCALAPPDATA% (and holds the data folder).
    local: std::path::PathBuf,
    rules: std::path::PathBuf,
}

const TEST_RULES: &str = r#"
[[rule]]
id = "test.cache"
name = "Test cache"
group = "Test"
tier = "green"
description = "A cache that rebuilds itself."
lose = "Nothing."
paths = ['%LOCALAPPDATA%\App\Cache']

[[rule]]
id = "test.logs"
name = "Test logs"
group = "Test"
tier = "yellow"
description = "Old logs."
lose = "Old log lines."
paths = ['%LOCALAPPDATA%\App\Logs']
include = ["*.log"]
"#;

fn sandbox() -> Sandbox {
    let tmp = tempfile::tempdir().unwrap();
    let local = std::fs::canonicalize(tmp.path()).unwrap();
    let rules = local.join("test-rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join("test.toml"), TEST_RULES).unwrap();
    for (rel, len) in [
        ("App/Cache/a.bin", 5000),
        ("App/Cache/sub/b.bin", 3000),
        ("App/Logs/x.log", 400),
        ("App/Logs/keep.txt", 100),
    ] {
        let p = local.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![b'x'; len]).unwrap();
    }
    Sandbox { tmp, local, rules }
}

impl Sandbox {
    fn wfc(&self) -> Command {
        let mut cmd = wfc(&self.local);
        cmd.stdin(std::process::Stdio::null());
        cmd
    }

    fn clean(&self, extra: &[&str]) -> std::process::Output {
        self.wfc()
            .arg("clean")
            .arg("--rules-dir")
            .arg(&self.rules)
            .args(extra)
            .output()
            .unwrap()
    }

    fn exists(&self, rel: &str) -> bool {
        self.local.join(rel).exists()
    }
}

fn ok(out: &std::process::Output) -> String {
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[test]
fn rules_list_includes_the_built_in_rules() {
    let s = sandbox();
    let out = s.wfc().args(["rules", "list", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_str(&ok(&out)).unwrap();
    let ids: Vec<&str> = v["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"windows.user-temp"), "{ids:?}");
    assert!(ids.contains(&"chrome.cache"));
    assert_eq!(v["errors"].as_array().unwrap().len(), 0, "{v}");
    // No built-in rule is ever red.
    assert!(
        v["rules"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["tier"] != "red")
    );
}

#[test]
fn rules_check_reports_problems() {
    let s = sandbox();
    let good = s.rules.join("test.toml");
    let out = s
        .wfc()
        .args(["rules", "check"])
        .arg(&good)
        .output()
        .unwrap();
    assert!(ok(&out).contains("OK (2 rules)"));

    let bad = s.local.join("bad.toml");
    std::fs::write(
        &bad,
        TEST_RULES.replace("tier = \"yellow\"", "tier = \"red\""),
    )
    .unwrap();
    let out = s.wfc().args(["rules", "check"]).arg(&bad).output().unwrap();
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("test.logs"), "{text}");

    // A rule aimed at the app's own data folder is refused.
    let own = s.local.join("own.toml");
    std::fs::write(
        &own,
        TEST_RULES.replace(r"%LOCALAPPDATA%\App\Logs", r"%LOCALAPPDATA%\WinFastCleaner"),
    )
    .unwrap();
    let out = s.wfc().args(["rules", "check"]).arg(&own).output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn dry_run_changes_nothing() {
    let s = sandbox();
    let out = s.clean(&["--tier", "yellow", "--dry-run", "--list"]);
    let text = ok(&out);
    assert!(text.contains("Dry run"), "{text}");
    assert!(text.contains("a.bin") && text.contains("x.log"), "{text}");
    assert!(
        !text.contains("keep.txt"),
        "include pattern applied: {text}"
    );
    assert!(text.contains("Would clean 8.2 KB (3 files)"), "{text}");
    for f in ["App/Cache/a.bin", "App/Cache/sub/b.bin", "App/Logs/x.log"] {
        assert!(s.exists(f), "{f} must still exist");
    }

    let out = s.clean(&["--dry-run", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&ok(&out)).unwrap();
    assert_eq!(v["dryRun"], true);
    assert_eq!(
        v["plan"]["items"].as_array().unwrap().len(),
        2,
        "green only by default"
    );
    assert!(s.exists("App/Cache/a.bin"));
}

#[test]
fn clean_asks_first_and_refuses_without_a_terminal() {
    let s = sandbox();
    let out = s.clean(&[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--yes"));
    assert!(s.exists("App/Cache/a.bin"));
}

#[test]
fn clean_then_restore_round_trip() {
    let s = sandbox();
    let out = s.clean(&["--tier", "yellow", "--yes", "--json"]);
    let report: serde_json::Value = serde_json::from_str(&ok(&out)).unwrap();
    assert_eq!(report["deletedCount"], 2, "{report}");
    assert_eq!(report["binnedCount"], 1, "{report}");
    assert_eq!(report["skippedCount"], 0, "{report}");
    assert!(!s.exists("App/Cache/a.bin"));
    assert!(!s.exists("App/Cache/sub"), "emptied folder removed");
    assert!(s.exists("App/Cache"), "the rule's own folder is kept");
    assert!(!s.exists("App/Logs/x.log"));
    assert!(s.exists("App/Logs/keep.txt"));

    let out = s
        .wfc()
        .args(["recover", "list", "--json"])
        .output()
        .unwrap();
    let list: serde_json::Value = serde_json::from_str(&ok(&out)).unwrap();
    let batch = list[0]["id"].as_str().unwrap().to_string();
    assert_eq!(batch, report["batchId"].as_str().unwrap());

    let out = s
        .wfc()
        .args(["recover", "restore", &batch])
        .output()
        .unwrap();
    assert!(ok(&out).contains("Restored 1 file"));
    assert_eq!(
        std::fs::read(s.local.join("App/Logs/x.log")).unwrap().len(),
        400
    );

    let out = s.wfc().args(["recover", "list"]).output().unwrap();
    assert!(ok(&out).contains("empty"));

    // Everything was logged locally.
    let log = std::fs::read_to_string(s.local.join("WinFastCleaner/logs/cleanup.log")).unwrap();
    assert_eq!(log.lines().count(), 4, "{log}");
    drop(s.tmp);
}

#[test]
fn exclusions_win_over_rules() {
    let s = sandbox();
    let keep = s.local.join("App/Cache/a.bin");
    let settings = serde_json::json!({
        "exclusions": [{ "kind": "file", "value": keep }]
    });
    std::fs::create_dir_all(s.local.join("WinFastCleaner")).unwrap();
    std::fs::write(
        s.local.join("WinFastCleaner/settings.json"),
        settings.to_string(),
    )
    .unwrap();
    let out = s.clean(&["--yes", "--json"]);
    let report: serde_json::Value = serde_json::from_str(&ok(&out)).unwrap();
    assert_eq!(report["deletedCount"], 1, "{report}");
    assert!(keep.exists(), "excluded file kept");
    assert!(!s.exists("App/Cache/sub/b.bin"));
}

#[test]
fn unknown_rules_are_an_error() {
    let s = sandbox();
    let out = s.clean(&["--rule", "nope", "--dry-run"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no rule named"));
}

// ------------------------------------------------------------ files

fn sized(path: &std::path::Path, len: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::File::create(path).unwrap().set_len(len).unwrap();
}

#[test]
fn dupes_are_matched_by_full_contents() {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let mut almost = body.clone();
    almost[150_000] ^= 1; // same size, start, and end; different middle
    for (rel, data) in [
        ("a/one.bin", &body),
        ("b/two.bin", &body),
        ("c/almost.bin", &almost),
    ] {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }
    let out = wfc(&root)
        .args([
            "dupes",
            "--no-mft",
            "--no-snapshot",
            "--json",
            "--min-size",
            "1KB",
        ])
        .arg(&root)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&ok(&out)).unwrap();
    let groups = v["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{v}");
    let names: Vec<&str> = groups[0]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 2);
    assert!(names[0].ends_with("one.bin") && names[1].ends_with("two.bin"));
    assert_eq!(v["wasted"], 300_000);
    // Nothing was changed.
    assert!(root.join("a/one.bin").exists() && root.join("b/two.bin").exists());
}

#[test]
fn find_large_and_old_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap().join("disk");
    sized(&root.join("videos/movie.mkv"), 300 << 20);
    sized(&root.join("setup.exe"), 120 << 20);
    sized(&root.join("small.txt"), 10);
    let out = wfc(tmp.path())
        .args(["find", "large", "--no-mft", "--no-snapshot", "--json"])
        .arg(&root)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&ok(&out)).unwrap();
    let files = v["files"].as_array().unwrap();
    assert_eq!(files.len(), 2, "{v}");
    assert!(files[0]["path"].as_str().unwrap().ends_with("movie.mkv"));
    assert_eq!(files[0]["kind"], "video");

    let out = wfc(tmp.path())
        .args([
            "find",
            "large",
            "--no-mft",
            "--no-snapshot",
            "--json",
            "--kind",
            "installer",
        ])
        .arg(&root)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&ok(&out)).unwrap();
    assert_eq!(v["files"].as_array().unwrap().len(), 1);

    // Nothing here is a year old.
    let out = wfc(tmp.path())
        .args(["find", "old", "--no-mft", "--no-snapshot", "--json"])
        .arg(&root)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&ok(&out)).unwrap();
    assert!(v["files"].as_array().unwrap().is_empty(), "{v}");
}

#[test]
fn grew_lists_the_folder_that_grew() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    sized(&root.join("stable/a.bin"), 80 << 20);
    sized(&root.join("apps/chat/cache/c.bin"), 60 << 20);
    let scan = |data: &std::path::Path| {
        let out = wfc(data)
            .args(["scan", "--quiet", "--no-mft"])
            .arg(&root)
            .output()
            .unwrap();
        ok(&out);
    };
    scan(data.path());
    // Snapshot names have one-second resolution.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    sized(&root.join("apps/chat/cache/d.bin"), 900 << 20);
    sized(&root.join("new-game/data.pak"), 200 << 20);
    scan(data.path());

    let out = wfc(data.path())
        .args(["snapshots", "--json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&ok(&out)).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2, "{v}");

    let out = wfc(data.path())
        .args(["grew", "--json"])
        .arg(root.display().to_string())
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&ok(&out)).unwrap();
    let grew: Vec<(&str, i64)> = v["grew"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["path"].as_str().unwrap(), c["delta"].as_i64().unwrap()))
        .collect();
    assert_eq!(grew.len(), 2, "{v}");
    assert!(
        grew[0].0.ends_with("cache"),
        "most specific folder: {grew:?}"
    );
    assert_eq!(grew[0].1, 900 << 20);
    assert!(grew[1].0.ends_with("new-game"), "{grew:?}");
    assert_eq!(v["totalDelta"], 1100 << 20);
}
