//! Cleaning rules: plain data files that say what is safe to clean.
//!
//! A rule file is TOML with one or more `[[rule]]` tables (the format is
//! documented in `docs/rule-format.md`). Rules are **data only**: loading
//! one can never run code or a command, unknown fields are rejected, and a
//! rule that points at a protected location is refused.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::glob::Glob;
use crate::safety::Guard;

/// Safety tier of an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// Regenerates automatically; selected by default.
    Green,
    /// Safe but has a cost; never selected by default.
    Yellow,
    /// The user's own files (manual selections only); never automatic.
    Red,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::Green => "Safe",
            Tier::Yellow => "Optional",
            Tier::Red => "Review",
        }
    }
}

/// Environment variables a rule path may start with.
pub const ALLOWED_VARS: &[&str] = &[
    "TEMP",
    "LOCALAPPDATA",
    "APPDATA",
    "USERPROFILE",
    "PROGRAMDATA",
    "SYSTEMROOT",
];

/// Where a rule came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "file")]
pub enum RuleSource {
    BuiltIn(String),
    User(PathBuf),
}

/// A validated cleaning rule.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub group: String,
    pub tier: Tier,
    pub description: String,
    pub lose: String,
    pub paths: Vec<String>,
    #[serde(skip)]
    pub include: Vec<Glob>,
    #[serde(skip)]
    pub exclude: Vec<Glob>,
    pub min_age_days: Option<u32>,
    pub requires_admin: bool,
    pub close_apps: Vec<String>,
    pub remove_empty_folders: bool,
    pub source: RuleSource,
}

/// A problem found while loading rules. Invalid rules are reported, never
/// silently skipped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleError {
    pub source: String,
    pub rule: Option<String>,
    pub message: String,
}

/// Rules plus any errors found while loading them.
#[derive(Clone, Debug, Default, Serialize)]
pub struct RuleSet {
    pub rules: Vec<Rule>,
    pub errors: Vec<RuleError>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    rule: Vec<RawRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    id: String,
    name: String,
    group: String,
    tier: Tier,
    description: String,
    lose: String,
    paths: Vec<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    min_age_days: Option<u32>,
    #[serde(default)]
    requires_admin: bool,
    #[serde(default)]
    close_apps: Vec<String>,
    #[serde(default = "default_true")]
    remove_empty_folders: bool,
}

fn default_true() -> bool {
    true
}

/// The built-in rule files, compiled into the program.
const BUILTIN_FILES: &[(&str, &str)] = &[
    ("windows.toml", include_str!("../../../rules/windows.toml")),
    (
        "browsers.toml",
        include_str!("../../../rules/browsers.toml"),
    ),
    ("apps.toml", include_str!("../../../rules/apps.toml")),
    (
        "dev-tools.toml",
        include_str!("../../../rules/dev-tools.toml"),
    ),
];

/// Largest user rule file accepted.
const MAX_RULE_FILE_BYTES: u64 = 256 * 1024;
const MAX_USER_RULE_FILES: usize = 200;

/// Values for the variables rule paths may use.
#[derive(Clone, Debug, Default)]
pub struct Vars(BTreeMap<String, PathBuf>);

impl Vars {
    /// Variables of the current user and system.
    pub fn system() -> Vars {
        let mut v = Vars::default();
        for name in ALLOWED_VARS {
            let value = if *name == "SYSTEMROOT" {
                crate::platform::windows_dir()
            } else {
                std::env::var_os(name).map(PathBuf::from)
            };
            if let Some(p) = value.filter(|p| p.is_absolute()) {
                v.0.insert((*name).to_string(), p);
            }
        }
        if !v.0.contains_key("TEMP") {
            v.0.insert("TEMP".into(), std::env::temp_dir());
        }
        // The user-temp rule empties %TEMP% as a whole, so a misconfigured
        // TEMP (say, pointing at Documents) must never be trusted.
        if v.0.get("TEMP").is_some_and(|t| !looks_like_temp_dir(t)) {
            v.0.remove("TEMP");
        }
        v
    }

    pub fn with(mut self, name: &str, value: impl Into<PathBuf>) -> Vars {
        self.0.insert(name.to_ascii_uppercase(), value.into());
        self
    }

    pub fn get(&self, name: &str) -> Option<&Path> {
        self.0.get(&name.to_ascii_uppercase()).map(PathBuf::as_path)
    }
}

/// Whether a folder's own name says it is for temporary files
/// (`Temp`, `TMP`, `tmp-files`, …).
pub fn looks_like_temp_dir(path: &Path) -> bool {
    path.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .is_some_and(|n| n.contains("temp") || n.contains("tmp"))
}

/// Built-in rules plus the user's rules from `user_dir` (if any).
pub fn load(user_dir: Option<&Path>, vars: &Vars, guard: &Guard) -> RuleSet {
    let mut set = RuleSet::default();
    for (name, text) in BUILTIN_FILES {
        parse_into(
            text,
            RuleSource::BuiltIn((*name).to_string()),
            vars,
            guard,
            &mut set,
        );
    }
    if let Some(dir) = user_dir {
        load_dir_into(dir, vars, guard, &mut set);
    }
    dedupe_ids(&mut set);
    set
}

/// Only the rules in `dir` (used by `wfc clean --rules-dir`).
pub fn load_dir(dir: &Path, vars: &Vars, guard: &Guard) -> RuleSet {
    let mut set = RuleSet::default();
    load_dir_into(dir, vars, guard, &mut set);
    dedupe_ids(&mut set);
    set
}

fn load_dir_into(dir: &Path, vars: &Vars, guard: &Guard, set: &mut RuleSet) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            set.errors.push(RuleError {
                source: dir.display().to_string(),
                rule: None,
                message: format!("could not read the rules folder: {e}"),
            });
            return;
        }
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("toml"))
        })
        .collect();
    files.sort();
    if files.len() > MAX_USER_RULE_FILES {
        set.errors.push(RuleError {
            source: dir.display().to_string(),
            rule: None,
            message: format!(
                "too many rule files; only the first {MAX_USER_RULE_FILES} were loaded"
            ),
        });
        files.truncate(MAX_USER_RULE_FILES);
    }
    for file in files {
        load_file_into(&file, vars, guard, set);
    }
}

/// Only the rules in one file (used by `wfc rules check`).
pub fn load_file(file: &Path, vars: &Vars, guard: &Guard) -> RuleSet {
    let mut set = RuleSet::default();
    load_file_into(file, vars, guard, &mut set);
    dedupe_ids(&mut set);
    set
}

fn load_file_into(file: &Path, vars: &Vars, guard: &Guard, set: &mut RuleSet) {
    let text = std::fs::metadata(file)
        .map_err(|e| e.to_string())
        .and_then(|m| {
            if !m.is_file() {
                Err("not a file".to_string())
            } else if m.len() > MAX_RULE_FILE_BYTES {
                Err("file is larger than 256 KB".to_string())
            } else {
                std::fs::read_to_string(file).map_err(|e| e.to_string())
            }
        });
    match text {
        Ok(text) => parse_into(
            &text,
            RuleSource::User(file.to_path_buf()),
            vars,
            guard,
            set,
        ),
        Err(message) => set.errors.push(RuleError {
            source: file.display().to_string(),
            rule: None,
            message,
        }),
    }
}

fn dedupe_ids(set: &mut RuleSet) {
    let mut seen = HashSet::new();
    let mut kept = Vec::with_capacity(set.rules.len());
    for rule in set.rules.drain(..) {
        if seen.insert(rule.id.clone()) {
            kept.push(rule);
        } else {
            set.errors.push(RuleError {
                source: source_name(&rule.source),
                rule: Some(rule.id.clone()),
                message: "another rule already uses this id".into(),
            });
        }
    }
    set.rules = kept;
}

fn source_name(s: &RuleSource) -> String {
    match s {
        RuleSource::BuiltIn(n) => format!("built-in {n}"),
        RuleSource::User(p) => p.display().to_string(),
    }
}

/// Parses one rule file's text.
pub fn parse_into(text: &str, source: RuleSource, vars: &Vars, guard: &Guard, set: &mut RuleSet) {
    let src = source_name(&source);
    let raw: RawFile = match toml::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            set.errors.push(RuleError {
                source: src,
                rule: None,
                message: format!("not a valid rule file: {}", e.message()),
            });
            return;
        }
    };
    if raw.rule.is_empty() {
        set.errors.push(RuleError {
            source: src,
            rule: None,
            message: "the file has no [[rule]] entries".into(),
        });
        return;
    }
    for r in raw.rule {
        let id = r.id.clone();
        match validate(r, source.clone(), vars, guard) {
            Ok(rule) => set.rules.push(rule),
            Err(message) => set.errors.push(RuleError {
                source: src.clone(),
                rule: Some(id),
                message,
            }),
        }
    }
}

fn check_text(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("`{field}` must not be empty"));
    }
    if value.chars().count() > max {
        return Err(format!("`{field}` is longer than {max} characters"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("`{field}` contains control characters"));
    }
    Ok(())
}

fn validate(r: RawRule, source: RuleSource, vars: &Vars, guard: &Guard) -> Result<Rule, String> {
    let id_ok = (3..=64).contains(&r.id.len())
        && r.id
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && r.id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
    if !id_ok {
        return Err("`id` must be 3-64 characters of a-z, 0-9, `.` and `-`".into());
    }
    check_text("name", &r.name, 80)?;
    check_text("group", &r.group, 40)?;
    check_text("description", &r.description, 400)?;
    check_text("lose", &r.lose, 300)?;
    if r.tier == Tier::Red {
        return Err("rules can only be green or yellow; red is for your own files".into());
    }
    if r.paths.is_empty() || r.paths.len() > 16 {
        return Err("`paths` must list 1 to 16 locations".into());
    }
    for p in &r.paths {
        validate_rule_path(p, vars, guard)?;
    }
    let include = if r.include.is_empty() {
        vec![Glob::new("**").expect("valid pattern")]
    } else {
        compile_globs("include", &r.include)?
    };
    let exclude = compile_globs("exclude", &r.exclude)?;
    if r.min_age_days.is_some_and(|d| d > 3650) {
        return Err("`min_age_days` must be at most 3650".into());
    }
    if r.close_apps.len() > 16 {
        return Err("`close_apps` lists too many programs".into());
    }
    for app in &r.close_apps {
        let ok = app.len() <= 64
            && app.to_ascii_lowercase().ends_with(".exe")
            && app
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || " ._-".contains(c));
        if !ok {
            return Err(format!(
                "`close_apps`: `{app}` must be a program file name such as `chrome.exe`"
            ));
        }
    }
    Ok(Rule {
        id: r.id,
        name: r.name,
        group: r.group,
        tier: r.tier,
        description: r.description,
        lose: r.lose,
        paths: r.paths,
        include,
        exclude,
        min_age_days: r.min_age_days,
        requires_admin: r.requires_admin,
        close_apps: r
            .close_apps
            .iter()
            .map(|a| a.to_ascii_lowercase())
            .collect(),
        remove_empty_folders: r.remove_empty_folders,
        source,
    })
}

fn compile_globs(field: &str, patterns: &[String]) -> Result<Vec<Glob>, String> {
    if patterns.len() > 64 {
        return Err(format!("`{field}` has too many patterns"));
    }
    patterns
        .iter()
        .map(|p| Glob::new(p).map_err(|e| format!("`{field}`: {e}")))
        .collect()
}

/// A rule path split into its variable and the segments after it.
#[derive(Debug)]
struct RulePath<'a> {
    var: &'a str,
    segments: Vec<&'a str>,
}

fn parse_rule_path(p: &str) -> Result<RulePath<'_>, String> {
    let rest = p
        .strip_prefix('%')
        .ok_or_else(|| format!("`{p}` must start with a variable such as %LOCALAPPDATA%"))?;
    let (var, tail) = rest
        .split_once('%')
        .ok_or_else(|| format!("`{p}` has an unfinished variable"))?;
    if !ALLOWED_VARS.iter().any(|v| v.eq_ignore_ascii_case(var)) {
        return Err(format!(
            "`%{var}%` is not an allowed variable (use one of {})",
            ALLOWED_VARS.join(", ")
        ));
    }
    let mut segments = Vec::new();
    for seg in tail.split(['/', '\\']) {
        if seg.is_empty() {
            continue;
        }
        if seg == "." || seg == ".." || seg.contains(':') || seg.contains('%') || seg.contains("**")
        {
            return Err(format!("`{p}`: segment `{seg}` is not allowed"));
        }
        if seg.ends_with('.') || seg.ends_with(' ') {
            return Err(format!("`{p}`: names may not end with a dot or space"));
        }
        segments.push(seg);
    }
    if segments.is_empty() && !var.eq_ignore_ascii_case("TEMP") {
        return Err(format!("`{p}` is too broad: name a folder inside %{var}%"));
    }
    if segments.len() > 24 {
        return Err(format!("`{p}` is too deep"));
    }
    Ok(RulePath { var, segments })
}

fn is_pattern(seg: &str) -> bool {
    seg.contains('*') || seg.contains('?')
}

/// Checks a rule path's syntax and, when the variable is known here, that
/// its fixed part does not point into a protected location.
fn validate_rule_path(p: &str, vars: &Vars, guard: &Guard) -> Result<(), String> {
    let rp = parse_rule_path(p)?;
    let Some(base) = vars.get(rp.var) else {
        return Ok(()); // Not available on this system; nothing to check.
    };
    let mut fixed = base.to_path_buf();
    for seg in rp.segments.iter().take_while(|s| !is_pattern(s)) {
        fixed.push(seg);
    }
    guard
        .check(&fixed)
        .map_err(|b| format!("`{p}` points into a protected location ({b})"))
}

/// Expands a rule path into existing locations on this system. Pattern
/// segments are matched against folder names; links are never followed.
pub fn expand_path(p: &str, vars: &Vars) -> Vec<PathBuf> {
    let Ok(rp) = parse_rule_path(p) else {
        return Vec::new();
    };
    let Some(base) = vars.get(rp.var) else {
        return Vec::new();
    };
    let mut current = vec![base.to_path_buf()];
    for (i, seg) in rp.segments.iter().enumerate() {
        let last = i + 1 == rp.segments.len();
        let mut next = Vec::new();
        for dir in &current {
            if is_pattern(seg) {
                let Ok(entries) = std::fs::read_dir(dir) else {
                    continue;
                };
                for e in entries.flatten() {
                    let is_real_dir = e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink());
                    if is_real_dir
                        && crate::glob::match_name_str(seg, &e.file_name().to_string_lossy())
                    {
                        next.push(e.path());
                    }
                }
            } else {
                next.push(dir.join(seg));
            }
        }
        // Intermediate segments must be real folders (not links).
        if !last {
            next.retain(|d| std::fs::symlink_metadata(d).is_ok_and(|m| m.is_dir()));
        }
        current = next;
    }
    current.retain(|p| {
        std::fs::symlink_metadata(p)
            .is_ok_and(|m| !m.file_type().is_symlink() && (m.is_dir() || m.is_file()))
    });
    current.sort();
    current.dedup();
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win_guard() -> Guard {
        Guard::windows(&crate::safety::tests::test_layout())
    }

    fn win_vars() -> Vars {
        Vars::default()
            .with("TEMP", r"C:\Users\test-user\AppData\Local\Temp")
            .with("LOCALAPPDATA", r"C:\Users\test-user\AppData\Local")
            .with("APPDATA", r"C:\Users\test-user\AppData\Roaming")
            .with("USERPROFILE", r"C:\Users\test-user")
            .with("PROGRAMDATA", r"C:\ProgramData")
            .with("SYSTEMROOT", r"C:\Windows")
    }

    #[test]
    fn only_real_temp_folders_are_trusted_as_temp() {
        for ok in [
            r"C:\Users\a\AppData\Local\Temp",
            r"D:\TMP",
            "/tmp",
            r"C:\my-temp-files",
        ] {
            assert!(looks_like_temp_dir(Path::new(ok)), "{ok}");
        }
        for bad in [r"C:\Users\a\Documents", r"C:\", r"D:\", r"C:\Users\a", "/"] {
            assert!(!looks_like_temp_dir(Path::new(bad)), "{bad}");
        }
        assert!(Vars::system().get("TEMP").is_none_or(looks_like_temp_dir));
    }

    /// The README lists every built-in rule; keep it in step.
    #[test]
    fn readme_lists_every_built_in_rule() {
        let readme = include_str!("../../../README.md");
        let mut set = RuleSet::default();
        for (name, text) in BUILTIN_FILES {
            parse_into(
                text,
                RuleSource::BuiltIn((*name).to_string()),
                &win_vars(),
                &win_guard(),
                &mut set,
            );
        }
        for r in &set.rules {
            assert!(
                readme.contains(&format!("| {} |", r.name)),
                "README.md's built-in rules table is missing `{}`",
                r.name
            );
        }
    }

    #[test]
    fn built_in_rules_all_load_cleanly() {
        let set = load(None, &win_vars(), &win_guard());
        assert!(set.errors.is_empty(), "{:#?}", set.errors);
        assert!(set.rules.len() >= 20, "only {} rules", set.rules.len());
        assert!(set.rules.iter().all(|r| r.tier != Tier::Red));
        // Every rule explains itself.
        for r in &set.rules {
            assert!(!r.description.is_empty() && !r.lose.is_empty(), "{}", r.id);
        }
    }

    fn one(text: &str) -> RuleSet {
        let mut set = RuleSet::default();
        parse_into(
            text,
            RuleSource::BuiltIn("test.toml".into()),
            &win_vars(),
            &win_guard(),
            &mut set,
        );
        set
    }

    const OK: &str = r#"
        [[rule]]
        id = "test.ok"
        name = "Test"
        group = "Test"
        tier = "green"
        description = "d"
        lose = "nothing"
        paths = ['%LOCALAPPDATA%\App\Cache']
    "#;

    #[test]
    fn valid_rule_parses() {
        let set = one(OK);
        assert!(set.errors.is_empty(), "{:?}", set.errors);
        assert_eq!(set.rules[0].include.len(), 1);
        assert!(set.rules[0].remove_empty_folders);
    }

    #[test]
    fn invalid_rules_are_reported_with_reasons() {
        let cases = [
            (OK.replace("tier = \"green\"", "tier = \"red\""), "red"),
            (
                OK.replace("tier = \"green\"", "tier = \"purple\""),
                "not a valid rule file",
            ),
            (
                OK.replace(
                    "lose = \"nothing\"",
                    "lose = \"nothing\"\nrun = \"cmd /c del\"",
                ),
                "not a valid rule file",
            ),
            (
                OK.replace(r"%LOCALAPPDATA%\App\Cache", r"C:\Temp"),
                "must start with a variable",
            ),
            (
                OK.replace(r"%LOCALAPPDATA%\App\Cache", r"%PATH%\x"),
                "not an allowed variable",
            ),
            (
                OK.replace(r"%LOCALAPPDATA%\App\Cache", r"%LOCALAPPDATA%\..\..\x"),
                "not allowed",
            ),
            (
                OK.replace(r"%LOCALAPPDATA%\App\Cache", r"%SYSTEMROOT%\System32"),
                "protected",
            ),
            (
                OK.replace(r"%LOCALAPPDATA%\App\Cache", r"%SYSTEMROOT%"),
                "too broad",
            ),
            (
                OK.replace(r"%LOCALAPPDATA%\App\Cache", r"%USERPROFILE%"),
                "too broad",
            ),
            (
                OK.replace(r"%LOCALAPPDATA%\App\Cache", r"%LOCALAPPDATA%\Packages\x"),
                "protected",
            ),
            (
                OK.replace(r"%LOCALAPPDATA%\App\Cache", r"%APPDATA%\Microsoft\Protect"),
                "protected",
            ),
            (OK.replace("id = \"test.ok\"", "id = \"Bad ID\""), "`id`"),
            (
                OK.replace("description = \"d\"", "description = \"\""),
                "description",
            ),
            (format!("{OK}\ninclude = ['../x']"), "include"),
            (format!("{OK}\nclose_apps = ['chrome']"), "close_apps"),
            (format!("{OK}\nmin_age_days = 99999"), "min_age_days"),
            ("not toml at all [".to_string(), "not a valid rule file"),
            ("".to_string(), "no [[rule]]"),
        ];
        for (text, needle) in cases {
            let set = one(&text);
            assert!(set.rules.is_empty(), "accepted: {text}");
            assert!(
                set.errors.iter().any(|e| e.message.contains(needle)),
                "expected `{needle}` in {:?}",
                set.errors
            );
        }
    }

    #[test]
    fn duplicate_ids_are_reported() {
        let mut set = RuleSet::default();
        for _ in 0..2 {
            parse_into(
                OK,
                RuleSource::BuiltIn("t".into()),
                &win_vars(),
                &win_guard(),
                &mut set,
            );
        }
        dedupe_ids(&mut set);
        assert_eq!(set.rules.len(), 1);
        assert_eq!(set.errors.len(), 1);
    }

    #[test]
    fn expansion_follows_patterns_but_not_links() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for d in [
            "User Data/Default/Cache",
            "User Data/Profile 1/Cache",
            "User Data/Profile 2/Other",
        ] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let vars = Vars::default().with("LOCALAPPDATA", root);
        let found = expand_path(r"%LOCALAPPDATA%\User Data\*\Cache", &vars);
        assert_eq!(found.len(), 2);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                root.join("User Data/Default"),
                root.join("User Data/Linked"),
            )
            .unwrap();
            assert_eq!(
                expand_path(r"%LOCALAPPDATA%\User Data\*\Cache", &vars).len(),
                2
            );
        }
        assert!(expand_path(r"%LOCALAPPDATA%\Missing\*", &vars).is_empty());
        assert!(expand_path(r"%APPDATA%\x", &vars).is_empty());
    }

    #[test]
    fn user_rules_load_from_a_folder() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("mine.toml"), OK).unwrap();
        std::fs::write(tmp.path().join("broken.toml"), "[[rule]]\nid = 1").unwrap();
        std::fs::write(tmp.path().join("notes.txt"), "ignored").unwrap();
        let set = load_dir(tmp.path(), &win_vars(), &win_guard());
        assert_eq!(set.rules.len(), 1);
        assert_eq!(set.errors.len(), 1);
    }
}
