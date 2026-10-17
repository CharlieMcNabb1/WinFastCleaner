//! App updates through winget, the Windows Package Manager.
//!
//! winget is the only outside program the app uses (besides Windows' own
//! DISM for Deep clean). It runs as a separate process with fixed arguments
//! and no window. The program is taken from its fixed per-user location,
//! never found through `PATH`. Package ids come from winget's own list and
//! are checked before being passed back to it, so a name can never be read
//! as an option.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::progress::CancelToken;

/// An installed app with an update available.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Upgrade {
    pub name: String,
    pub id: String,
    pub version: String,
    pub available: String,
    pub source: Option<String>,
    /// winget's id came through intact, so the app can update it. (Very
    /// long ids can be cut short in winget's table.)
    pub can_update: bool,
}

/// How one update went.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeResult {
    pub id: String,
    pub ok: bool,
    pub message: String,
}

const LIST_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const UPGRADE_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const MAX_OUTPUT: usize = 4 * 1024 * 1024;

// winget's own result codes.
const NO_APPLICATIONS_FOUND: u32 = 0x8A15_0014;
const UPDATE_NOT_APPLICABLE: u32 = 0x8A15_002B;

/// The message shown when winget isn't installed.
pub const MISSING: &str = "winget (the Windows Package Manager) isn't installed. It comes with \
    App Installer: open the Microsoft Store, search for \"App Installer\", and install or update \
    it. Then come back here.";

/// Where winget is: the per-user App Installer alias.
pub fn find() -> Option<PathBuf> {
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    if !local.is_absolute() {
        return None;
    }
    let exe = local.join(r"Microsoft\WindowsApps\winget.exe");
    // The alias is a special link; look at it without opening its target.
    std::fs::symlink_metadata(&exe).ok().map(|_| exe)
}

/// Lists apps with updates. Cancelling stops winget (this only reads).
pub fn list(winget: &Path, cancel: &CancelToken) -> Result<Vec<Upgrade>, String> {
    let (code, text) = run(
        winget,
        &[
            "upgrade",
            "--accept-source-agreements",
            "--disable-interactivity",
        ],
        LIST_TIMEOUT,
        Some(cancel),
    )?;
    if let Some(rows) = parse_list(&text) {
        return Ok(rows);
    }
    match code {
        Some(0) => Ok(Vec::new()),
        Some(c) if c as u32 == NO_APPLICATIONS_FOUND || c as u32 == UPDATE_NOT_APPLICABLE => {
            Ok(Vec::new())
        }
        _ => Err(failure("winget could not list updates", code, &text)),
    }
}

/// Updates one app. This is not cancelled midway (stopping an installer
/// halfway could leave the app broken); a cancel takes effect between apps.
pub fn upgrade(winget: &Path, up: &Upgrade) -> UpgradeResult {
    let result = |ok: bool, message: String| UpgradeResult {
        id: up.id.clone(),
        ok,
        message,
    };
    if !up.can_update || !valid_id(&up.id) {
        return result(
            false,
            "This app can't be updated from here. Update it from the app itself.".into(),
        );
    }
    let mut args = vec![
        "upgrade",
        "--id",
        up.id.as_str(),
        "--exact",
        "--silent",
        "--accept-source-agreements",
        "--accept-package-agreements",
        "--disable-interactivity",
    ];
    if let Some(s) = up.source.as_deref().filter(|s| valid_source(s)) {
        args.extend(["--source", s]);
    }
    match run(winget, &args, UPGRADE_TIMEOUT, None) {
        Ok((Some(0), _)) => result(true, format!("Updated to {}.", up.available)),
        Ok((Some(c), _)) if c as u32 == UPDATE_NOT_APPLICABLE => {
            result(true, "Already up to date.".into())
        }
        Ok((code, text)) => result(false, failure("The update failed", code, &text)),
        Err(e) => result(false, e),
    }
}

/// A package id winget may be given: letters, digits, and `. _ - +`,
/// starting with a letter or digit (so it can't look like an option).
pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= 128
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
}

/// A source name such as `winget` or `msstore`.
pub fn valid_source(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn failure(what: &str, code: Option<i32>, text: &str) -> String {
    let code = code
        .map(|c| format!(" (code 0x{:08X})", c as u32))
        .unwrap_or_default();
    match last_message(text) {
        Some(m) => format!("{what}{code}: {m}"),
        None => format!("{what}{code}."),
    }
}

/// winget's last meaningful line, without progress bars and spinners.
fn last_message(text: &str) -> Option<String> {
    text.split(['\r', '\n'])
        .map(|l| l.trim())
        .rfind(|l| {
            !l.is_empty()
                && !l.contains(['█', '▒'])
                && !l.chars().all(|c| matches!(c, '-' | '\\' | '|' | '/' | ' '))
        })
        .map(|l| clean(l).chars().take(300).collect())
}

fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Runs winget and collects its output. `cancel` (when given) stops it.
fn run(
    winget: &Path,
    args: &[&str],
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> Result<(Option<i32>, String), String> {
    let mut cmd = Command::new(winget);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("winget could not be started: {e}"))?;
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|mut r| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let mut buf = [0u8; 8192];
            while let Ok(n) = r.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if out.len() < MAX_OUTPUT {
                    out.extend_from_slice(&buf[..n]);
                }
            }
            out
        })
    })
    .collect();
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => return Err(format!("winget stopped unexpectedly: {e}")),
        }
        if cancel.is_some_and(|c| c.is_cancelled()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Cancelled.".into());
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err("winget took too long and was stopped.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let mut text = String::new();
    for r in readers {
        if let Ok(bytes) = r.join() {
            text.push_str(&String::from_utf8_lossy(&bytes));
            text.push('\n');
        }
    }
    Ok((status.code(), text))
}

// ------------------------------------------------------------ parsing

/// Display width of a character in a console (East Asian wide characters
/// take two columns).
fn width(c: char) -> usize {
    let u = c as u32;
    let wide = matches!(u,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x3FFFD);
    if wide { 2 } else { 1 }
}

/// Splits a line at display columns `starts`.
fn split_at_columns(line: &str, starts: &[usize]) -> Vec<String> {
    let mut fields = vec![String::new(); starts.len()];
    let mut col = 0;
    for c in line.chars() {
        let field = starts.iter().rposition(|&s| s <= col).unwrap_or(0);
        fields[field].push(c);
        col += width(c);
    }
    fields
}

/// The character just before display column `at`, if any.
fn char_before(line: &str, at: usize) -> Option<char> {
    let mut col = 0;
    let mut prev = None;
    for c in line.chars() {
        if col >= at {
            return prev;
        }
        prev = Some(c);
        col += width(c);
    }
    if col >= at { prev } else { None }
}

fn line_width(line: &str) -> usize {
    line.chars().map(width).sum()
}

/// Reads winget's update table. The column headers are translated on
/// non-English Windows, so columns are found by position: the header line
/// sits right above the line of dashes, and the columns are, in order,
/// name, id, version, available, and (usually) source. `None` when there
/// is no table.
pub fn parse_list(text: &str) -> Option<Vec<Upgrade>> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| {
            let l = l.trim_end_matches('\r');
            l.rsplit('\r').next().unwrap_or(l).trim_end()
        })
        .collect();
    let dashes = lines
        .iter()
        .position(|l| l.trim().len() >= 10 && l.trim().chars().all(|c| c == '-'))?;
    let header = *lines.get(dashes.checked_sub(1)?)?;
    let hc: Vec<char> = header.chars().collect();
    let mut starts = Vec::new();
    let mut col = 0;
    for (i, &c) in hc.iter().enumerate() {
        if c != ' ' && (i == 0 || hc[i - 1] == ' ') {
            starts.push(col);
        }
        col += width(c);
    }
    if !(4..=5).contains(&starts.len()) {
        return None;
    }
    let mut out = Vec::new();
    for line in &lines[dashes + 1..] {
        // The table ends at the first line that doesn't fit its columns
        // (a blank line, the "N upgrades available" footer, or a second
        // table's title).
        if line.trim().is_empty() || line_width(line) <= starts[3] {
            break;
        }
        if starts[1..4]
            .iter()
            .any(|&s| char_before(line, s).is_some_and(|c| c != ' '))
        {
            break;
        }
        let f: Vec<String> = split_at_columns(line, &starts)
            .into_iter()
            .map(|s| clean(&s))
            .collect();
        let (name, id, version, available) = (&f[0], &f[1], &f[2], &f[3]);
        if id.is_empty()
            || id.contains(char::is_whitespace)
            || version.is_empty()
            || available.is_empty()
        {
            break;
        }
        let source = f.get(4).filter(|s| !s.is_empty()).cloned();
        out.push(Upgrade {
            name: name.trim_end_matches('…').trim().to_string(),
            can_update: valid_id(id),
            id: id.clone(),
            version: version.clone(),
            available: available.clone(),
            source,
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        let row = |r: [&str; 5]| {
            format!(
                "{:<29}{:<31}{:<14}{:<14}{}\r\n",
                r[0], r[1], r[2], r[3], r[4]
            )
        };
        let mut s = String::from("   - \r   \\ \r\r\n");
        s += &row(["Name", "Id", "Version", "Available", "Source"]);
        s += &format!("{}\r\n", "-".repeat(94));
        s += &row(["Git", "Git.Git", "2.44.0", "2.45.1", "winget"]);
        s += &row([
            "Microsoft Visual C++ 2015-2…",
            "Microsoft.VCRedist.2015+.x64",
            "14.38.33135.0",
            "14.40.33810.0",
            "winget",
        ]);
        s += &row([
            "Mozilla Firefox (x64 en-US)",
            "Mozilla.Firefox",
            "< 126.0",
            "126.0.1",
            "winget",
        ]);
        s += &row([
            "A very long app name here",
            "Some.Really.Long.Package.I…",
            "1.0",
            "2.0",
            "winget",
        ]);
        s += "4 upgrades available.\r\n\r\n";
        s += "The following packages have an upgrade available, but require explicit targeting for upgrade:\r\n";
        s += "Name   Id          Version Available Source\r\n";
        s += "-------------------------------------------\r\n";
        s += "Pinned Pinned.App  1.0     2.0       winget\r\n";
        s
    }

    #[test]
    fn upgrade_table_is_read_by_column() {
        let rows = parse_list(&sample()).unwrap();
        assert_eq!(rows.len(), 4, "{rows:#?}");
        assert_eq!(
            rows[0],
            Upgrade {
                name: "Git".into(),
                id: "Git.Git".into(),
                version: "2.44.0".into(),
                available: "2.45.1".into(),
                source: Some("winget".into()),
                can_update: true,
            }
        );
        // winget cuts long names short with an ellipsis.
        assert_eq!(rows[1].name, "Microsoft Visual C++ 2015-2");
        assert_eq!(rows[1].id, "Microsoft.VCRedist.2015+.x64");
        assert_eq!(rows[2].version, "< 126.0");
        assert_eq!(rows[2].available, "126.0.1");
        // A truncated id can't be passed back to winget.
        assert!(!rows[3].can_update);
        assert!(rows.iter().all(|r| r.id != "Pinned.App"));
    }

    #[test]
    fn translated_and_wide_headers_work() {
        let text = "名前         ID           バージョン 利用可能 ソース\n\
---------------------------------------------------------\n\
メモ帳       Vendor.Notes 1.0        1.1      winget\n";
        let rows = parse_list(text).unwrap();
        assert_eq!(rows.len(), 1, "{rows:#?}");
        assert_eq!(rows[0].name, "メモ帳");
        assert_eq!(rows[0].id, "Vendor.Notes");
        assert_eq!(rows[0].available, "1.1");

        let text = "Nom      ID       Version Disponible\n------------------------------------\nOutil    A.Outil  1.0     2.0\n";
        let rows = parse_list(text).unwrap();
        assert_eq!(rows[0].id, "A.Outil");
        assert_eq!(rows[0].source, None);
    }

    #[test]
    fn no_table_means_no_rows() {
        assert_eq!(
            parse_list("No installed package found matching input criteria.\n"),
            None
        );
        assert_eq!(parse_list(""), None);
        let empty = "Name Id Version Available Source\n--------------------------------\n0 upgrades available.\n";
        assert_eq!(parse_list(empty), Some(Vec::new()));
    }

    #[test]
    fn ids_and_sources_are_checked() {
        for ok in [
            "Git.Git",
            "Microsoft.VCRedist.2015+.x64",
            "9NBLGGH4NNS1",
            "a_b-c",
        ] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in [
            "",
            "-h",
            "--id",
            "a b",
            "Some.Package…",
            "a;b",
            "a\"b",
            "a\\b",
            &"x".repeat(129),
        ] {
            assert!(!valid_id(bad), "{bad}");
        }
        assert!(valid_source("winget") && valid_source("msstore"));
        assert!(!valid_source("--source") && !valid_source("a b") && !valid_source(""));
    }

    #[test]
    fn messages_skip_progress_output() {
        let text = "Found Git [Git.Git] Version 2.45.1\r\n  ██████▒▒▒▒  50%\r\n\\\r|\r\nInstaller failed with exit code: 1603\r\n";
        assert_eq!(
            failure("The update failed", Some(-1978335215), text),
            "The update failed (code 0x8A150011): Installer failed with exit code: 1603"
        );
        assert_eq!(failure("x", None, ""), "x.");
    }

    #[test]
    fn untrusted_rows_are_never_updated() {
        let up = Upgrade {
            name: "x".into(),
            id: "--source=evil".into(),
            version: "1".into(),
            available: "2".into(),
            source: None,
            can_update: true,
        };
        // Refused before winget would even be looked for.
        let r = upgrade(Path::new("does-not-exist"), &up);
        assert!(!r.ok);
        assert!(r.message.contains("can't be updated"));
    }
}
