//! Startup manager: what launches when you sign in.
//!
//! Lists the registry Run keys (yours and the machine's), the Startup
//! folders, and scheduled tasks that run at sign-in. Turning an item off uses
//! the same switch as Task Manager's Startup tab (the `StartupApproved`
//! registry values under `Explorer`), or for a task, the task's own on/off
//! flag in Task Scheduler. Either way it can be turned back on, and nothing
//! is ever deleted: the Run value, the shortcut, or the task stays where it
//! is.
//!
//! Services set to start automatically are listed separately and are
//! read-only here; services that are part of Windows are left out.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Where a startup item comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// Your Run key (`HKEY_CURRENT_USER`).
    UserRun,
    /// The machine's Run key, for every user.
    MachineRun,
    /// The machine's Run key for 32-bit programs.
    MachineRun32,
    /// Your Startup folder.
    UserFolder,
    /// The Startup folder for every user.
    CommonFolder,
    /// A scheduled task that runs when you sign in.
    Task,
}

impl Source {
    fn key(self) -> &'static str {
        match self {
            Source::UserRun => "user-run",
            Source::MachineRun => "machine-run",
            Source::MachineRun32 => "machine-run32",
            Source::UserFolder => "user-folder",
            Source::CommonFolder => "common-folder",
            Source::Task => "task",
        }
    }
}

/// Registry root of a Run key or approval key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

/// A Run key and where its on/off switches live.
#[derive(Clone, Debug)]
pub struct RunKey {
    pub source: Source,
    pub hive: Hive,
    pub key: String,
    /// Read the 32-bit view of the registry (`WOW6432Node`).
    pub wow32: bool,
    /// The `StartupApproved` key holding this key's on/off switches.
    pub approved: String,
}

/// A Startup folder and where its on/off switches live.
#[derive(Clone, Debug)]
pub struct StartupFolder {
    pub source: Source,
    pub dir: PathBuf,
    pub hive: Hive,
    pub approved: String,
}

/// Where to look. [`Locations::system`] for the real thing; tests use a
/// throwaway registry key and a temporary folder.
#[derive(Clone, Debug)]
pub struct Locations {
    pub run_keys: Vec<RunKey>,
    pub folders: Vec<StartupFolder>,
    /// Include scheduled tasks that run at sign-in.
    pub tasks: bool,
}

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

impl Locations {
    pub fn system() -> Locations {
        let env_dir = |var: &str, rest: &str| {
            std::env::var_os(var)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|p| crate::platform::long_path(&p.join(rest)))
        };
        let mut folders = Vec::new();
        if let Some(dir) = env_dir("APPDATA", r"Microsoft\Windows\Start Menu\Programs\Startup") {
            folders.push(StartupFolder {
                source: Source::UserFolder,
                dir,
                hive: Hive::CurrentUser,
                approved: format!(r"{APPROVED}\StartupFolder"),
            });
        }
        if let Some(dir) = env_dir(
            "ProgramData",
            r"Microsoft\Windows\Start Menu\Programs\StartUp",
        ) {
            folders.push(StartupFolder {
                source: Source::CommonFolder,
                dir,
                hive: Hive::LocalMachine,
                approved: format!(r"{APPROVED}\StartupFolder"),
            });
        }
        Locations {
            run_keys: vec![
                RunKey {
                    source: Source::UserRun,
                    hive: Hive::CurrentUser,
                    key: RUN.into(),
                    wow32: false,
                    approved: format!(r"{APPROVED}\Run"),
                },
                RunKey {
                    source: Source::MachineRun,
                    hive: Hive::LocalMachine,
                    key: RUN.into(),
                    wow32: false,
                    approved: format!(r"{APPROVED}\Run"),
                },
                RunKey {
                    source: Source::MachineRun32,
                    hive: Hive::LocalMachine,
                    key: RUN.into(),
                    wow32: true,
                    approved: format!(r"{APPROVED}\Run32"),
                },
            ],
            folders,
            tasks: cfg!(windows),
        }
    }
}

/// One thing that starts when you sign in.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    /// Stable within a listing; the app looks items up by it.
    pub id: String,
    pub name: String,
    pub source: Source,
    /// The registry key, folder, or task folder it is in.
    pub location: String,
    /// The command it runs.
    pub command: String,
    /// The program behind the command, when it can be told.
    pub program: Option<String>,
    pub publisher: Option<String>,
    pub enabled: bool,
    /// Starts for every user, so changing it needs administrator rights.
    pub machine: bool,
    /// When it was turned off (Unix seconds), if Windows recorded it.
    pub disabled_at: Option<i64>,
    /// For registry and folder items: the name of the on/off switch.
    #[serde(skip)]
    entry: String,
}

/// An auto-start service (read-only).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceItem {
    pub name: String,
    pub display_name: String,
    pub program: Option<String>,
    pub publisher: Option<String>,
    pub running: bool,
    /// "Automatic (delayed start)".
    pub delayed: bool,
}

/// Everything found, plus anything that couldn't be read.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupList {
    pub items: Vec<StartupItem>,
    pub errors: Vec<String>,
}

impl StartupList {
    pub fn enabled_count(&self) -> usize {
        self.items.iter().filter(|i| i.enabled).count()
    }
}

/// Lists everything that starts at sign-in.
pub fn list(loc: &Locations) -> StartupList {
    let mut out = StartupList::default();
    for rk in &loc.run_keys {
        match imp::run_values(rk) {
            Ok(values) => {
                for (name, command) in values {
                    let state = imp::approval(rk.hive, &rk.approved, &name);
                    out.items.push(item(
                        rk.source,
                        rk.hive,
                        &name,
                        name.clone(),
                        format!("{}\\{}", hive_name(rk.hive), rk.key),
                        command,
                        state,
                    ));
                }
            }
            Err(e) => out.errors.push(e),
        }
    }
    for f in &loc.folders {
        for (file, command) in folder_entries(&f.dir) {
            let state = imp::approval(f.hive, &f.approved, &file);
            let name = Path::new(&file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.clone());
            out.items.push(item(
                f.source,
                f.hive,
                &file,
                name,
                f.dir.display().to_string(),
                command,
                state,
            ));
        }
    }
    if loc.tasks {
        match imp::logon_tasks() {
            Ok(tasks) => out.items.extend(tasks),
            Err(e) => out
                .errors
                .push(format!("Scheduled tasks could not be read: {e}")),
        }
    }
    out.items.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    out
}

fn hive_name(h: Hive) -> &'static str {
    match h {
        Hive::CurrentUser => "HKEY_CURRENT_USER",
        Hive::LocalMachine => "HKEY_LOCAL_MACHINE",
    }
}

fn item(
    source: Source,
    hive: Hive,
    entry: &str,
    name: String,
    location: String,
    command: String,
    state: Approval,
) -> StartupItem {
    let program = target_program(&command);
    StartupItem {
        id: format!("{}:{}", source.key(), entry),
        name,
        source,
        location,
        publisher: program
            .as_deref()
            .and_then(|p| imp::publisher(Path::new(p))),
        program,
        command,
        enabled: state.enabled,
        machine: hive == Hive::LocalMachine,
        disabled_at: state.disabled_at,
        entry: entry.to_string(),
    }
}

/// Files in a Startup folder, with the command each one runs.
fn folder_entries(dir: &Path) -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let Ok(md) = e.path().symlink_metadata() else {
            continue;
        };
        let name = e.file_name().to_string_lossy().into_owned();
        if md.is_dir() || name.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }
        let path = e.path();
        let is_lnk = path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("lnk"));
        let command = match is_lnk.then(|| imp::shortcut_target(&path)).flatten() {
            Some((target, args)) if args.is_empty() => quote(&target),
            Some((target, args)) => format!("{} {args}", quote(&target)),
            None => quote(&path.display().to_string()),
        };
        out.push((name, command));
    }
    out
}

fn quote(path: &str) -> String {
    if path.contains(' ') {
        format!("\"{path}\"")
    } else {
        path.to_string()
    }
}

/// Turns an item on or off. Nothing is deleted: registry and folder items
/// get Task Manager's on/off switch, tasks get Task Scheduler's.
pub fn set_enabled(loc: &Locations, item: &StartupItem, enabled: bool) -> Result<(), String> {
    if item.source == Source::Task {
        let path = item
            .id
            .strip_prefix("task:")
            .ok_or("Unknown startup item.")?;
        return imp::set_task_enabled(path, enabled);
    }
    let (hive, approved) = loc
        .run_keys
        .iter()
        .find(|k| k.source == item.source)
        .map(|k| (k.hive, k.approved.as_str()))
        .or_else(|| {
            loc.folders
                .iter()
                .find(|f| f.source == item.source)
                .map(|f| (f.hive, f.approved.as_str()))
        })
        .ok_or("Unknown startup item.")?;
    if item.entry.is_empty() || item.entry.len() > 16_000 || item.entry.contains('\0') {
        return Err("Unknown startup item.".into());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    imp::set_approval(hive, approved, &item.entry, &approval_bytes(enabled, now))
}

// ------------------------------------------------------------ on/off switch

/// An item's state from its `StartupApproved` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Approval {
    pub enabled: bool,
    pub disabled_at: Option<i64>,
}

/// Reads a `StartupApproved` value: 12 bytes, the first one even when the
/// item is on and odd when it is off, followed (when off) by the time it was
/// turned off. A missing value means on.
pub fn parse_approval(data: Option<&[u8]>) -> Approval {
    let Some(d) = data.filter(|d| !d.is_empty()) else {
        return Approval {
            enabled: true,
            disabled_at: None,
        };
    };
    let enabled = d[0] & 1 == 0;
    let disabled_at = (!enabled && d.len() >= 12)
        .then(|| u64::from_le_bytes(d[4..12].try_into().unwrap_or_default()))
        .filter(|&ft| ft > 0)
        .map(crate::format::filetime_to_unix);
    Approval {
        enabled,
        disabled_at,
    }
}

/// The value Task Manager writes: `02` and zeros to turn on, `03` and the
/// current time to turn off.
pub fn approval_bytes(enabled: bool, now_unix: i64) -> [u8; 12] {
    let mut b = [0u8; 12];
    if enabled {
        b[0] = 2;
    } else {
        b[0] = 3;
        let ft = (now_unix.max(0) as u64 + 11_644_473_600) * 10_000_000;
        b[4..].copy_from_slice(&ft.to_le_bytes());
    }
    b
}

// ------------------------------------------------------------ commands

/// Expands `%NAME%` variables; unknown ones are left as they are.
pub fn expand_vars(s: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match lookup(name) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The program a command line runs: the quoted part, or for an unquoted
/// command the shortest prefix ending in a program extension (paths with
/// spaces are often left unquoted), or else the first word.
pub fn program_from_command(cmd: &str) -> Option<String> {
    let cmd = cmd.trim();
    if let Some(rest) = cmd.strip_prefix('"') {
        let p = rest.split('"').next().unwrap_or_default().trim();
        return (!p.is_empty()).then(|| p.to_string());
    }
    let lower = cmd.to_ascii_lowercase();
    let mut best: Option<usize> = None;
    for ext in [".exe", ".com", ".bat", ".cmd", ".vbs", ".ps1", ".lnk"] {
        let mut from = 0;
        while let Some(i) = lower[from..].find(ext) {
            let end = from + i + ext.len();
            if end == cmd.len() || cmd.as_bytes()[end] == b' ' {
                best = Some(best.map_or(end, |b: usize| b.min(end)));
                break;
            }
            from = end;
        }
    }
    let end = best.unwrap_or_else(|| cmd.find(' ').unwrap_or(cmd.len()));
    let p = cmd[..end].trim();
    (!p.is_empty()).then(|| p.to_string())
}

/// Like [`program_from_command`], but for `rundll32 some.dll,Entry` it is
/// the DLL that matters (its publisher, not Windows').
pub fn target_program(cmd: &str) -> Option<String> {
    let program = program_from_command(cmd)?;
    let file = program
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if file == "rundll32.exe" || file == "rundll32" {
        let trimmed = cmd.trim();
        let after = if let Some(rest) = trimmed.strip_prefix('"') {
            rest.split_once('"').map(|(_, r)| r).unwrap_or_default()
        } else {
            &trimmed[program.len().min(trimmed.len())..]
        };
        let arg = after.trim().trim_start_matches('"');
        let dll = arg.split([',', '"']).next().unwrap_or_default().trim();
        if !dll.is_empty() {
            return Some(dll.to_string());
        }
    }
    Some(program)
}

/// Where a program named without a folder (such as `ctfmon.exe`) lives.
#[cfg_attr(not(windows), allow(dead_code))]
fn resolve_program(program: &str) -> Option<PathBuf> {
    let p = Path::new(program);
    if p.is_absolute() {
        return Some(p.to_path_buf());
    }
    if program.contains(['\\', '/']) {
        return None;
    }
    let sys = crate::platform::windows_dir()?
        .join("System32")
        .join(program);
    Some(sys)
}

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;

    use super::*;
    use crate::winreg::{
        self, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_32KEY, KEY_WOW64_64KEY, Key,
    };

    fn hive(h: Hive) -> winreg::Hive {
        match h {
            Hive::CurrentUser => winreg::Hive::CurrentUser,
            Hive::LocalMachine => winreg::Hive::LocalMachine,
        }
    }

    fn view(h: Hive, wow32: bool) -> u32 {
        match (h, wow32) {
            (_, true) => KEY_WOW64_32KEY,
            (Hive::LocalMachine, false) => KEY_WOW64_64KEY,
            (Hive::CurrentUser, false) => 0,
        }
    }

    fn env(name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    pub fn run_values(rk: &RunKey) -> Result<Vec<(String, String)>, String> {
        let key = Key::open(
            hive(rk.hive),
            &rk.key,
            KEY_QUERY_VALUE | view(rk.hive, rk.wow32),
        )
        .map_err(|e| {
            format!(
                "{}\\{} could not be read (error {e}).",
                hive_name(rk.hive),
                rk.key
            )
        })?;
        let Some(key) = key else {
            return Ok(Vec::new());
        };
        Ok(key
            .values()
            .into_iter()
            .filter(|v| !v.name.is_empty())
            .filter_map(|v| {
                let (text, expand) = v.text()?;
                let text = if expand {
                    expand_vars(&text, env)
                } else {
                    text
                };
                (!text.trim().is_empty()).then_some((v.name, text))
            })
            .collect())
    }

    pub fn approval(h: Hive, approved: &str, name: &str) -> Approval {
        let value = Key::open(hive(h), approved, KEY_QUERY_VALUE | view(h, false))
            .ok()
            .flatten()
            .and_then(|k| k.get(name));
        parse_approval(value.as_ref().map(|v| v.data.as_slice()))
    }

    pub fn set_approval(h: Hive, approved: &str, name: &str, data: &[u8]) -> Result<(), String> {
        let denied = || {
            "Windows needs administrator rights to change an item that starts for every user. \
             Restart as administrator and try again."
                .to_string()
        };
        let key = Key::create(hive(h), approved, KEY_SET_VALUE | view(h, false)).map_err(|e| {
            if e == ERROR_ACCESS_DENIED {
                denied()
            } else {
                format!("The setting could not be saved (error {e}).")
            }
        })?;
        key.set_binary(name, data).map_err(|e| {
            if e == ERROR_ACCESS_DENIED {
                denied()
            } else {
                format!("The setting could not be saved (error {e}).")
            }
        })
    }

    pub fn shortcut_target(lnk: &Path) -> Option<(String, String)> {
        let (target, args) = crate::wincom::shortcut_target(lnk)?;
        let target = expand_vars(&target, env);
        (!target.is_empty()).then_some((target, expand_vars(&args, env)))
    }

    pub fn logon_tasks() -> Result<Vec<StartupItem>, String> {
        let tasks = crate::wincom::logon_tasks()?;
        Ok(tasks
            .into_iter()
            .map(|t| {
                let command = match (&t.command, &t.arguments) {
                    (Some(c), Some(a)) => {
                        format!("{} {}", quote(&expand_vars(c, env)), expand_vars(a, env))
                    }
                    (Some(c), None) => quote(&expand_vars(c, env)),
                    _ => String::new(),
                };
                let program = target_program(&command);
                let folder = match t.path.rfind('\\') {
                    Some(0) | None => "\\".to_string(),
                    Some(i) => t.path[..i].to_string(),
                };
                StartupItem {
                    id: format!("task:{}", t.path),
                    name: t.name,
                    source: Source::Task,
                    location: format!("Task Scheduler {folder}"),
                    publisher: program
                        .as_deref()
                        .and_then(|p| publisher(Path::new(p)))
                        .or(t.author.filter(|a| !a.starts_with('$'))),
                    program,
                    command,
                    enabled: t.enabled,
                    machine: false,
                    disabled_at: None,
                    entry: String::new(),
                }
            })
            .collect())
    }

    pub fn set_task_enabled(path: &str, enabled: bool) -> Result<(), String> {
        crate::wincom::set_task_enabled(path, enabled)
    }

    /// The company name in a program's version information.
    pub fn publisher(program: &Path) -> Option<String> {
        let path = resolve_program(&program.to_string_lossy())?;
        version_string(&path, "CompanyName")
    }

    fn wide(p: &std::ffi::OsStr) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        p.encode_wide().chain(Some(0)).collect()
    }

    /// Reads one string from a file's version resource (as data only; no
    /// code from the file is run). Cloud-only files are skipped so they are
    /// never downloaded.
    pub fn version_string(path: &Path, field: &str) -> Option<String> {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
        };
        const CLOUD: u32 = 0x1000 | 0x4_0000 | 0x40_0000;
        let md = std::fs::metadata(path).ok()?;
        if !md.is_file() || md.file_attributes() & CLOUD != 0 {
            return None;
        }
        let w = wide(path.as_os_str());
        let mut handle = 0u32;
        // SAFETY: `w` is NUL-terminated.
        let size = unsafe { GetFileVersionInfoSizeW(w.as_ptr(), &mut handle) };
        if size == 0 || size > 16 * 1024 * 1024 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        // SAFETY: the buffer is `size` bytes long.
        if unsafe { GetFileVersionInfoW(w.as_ptr(), 0, size, buf.as_mut_ptr().cast()) } == 0 {
            return None;
        }
        let query = |sub: &str| -> Option<&[u8]> {
            let q = wide(std::ffi::OsStr::new(sub));
            let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut len = 0u32;
            // SAFETY: `buf` holds the version resource; the returned pointer
            // points into it and is valid while `buf` lives.
            let ok = unsafe { VerQueryValueW(buf.as_ptr().cast(), q.as_ptr(), &mut ptr, &mut len) };
            if ok == 0 || ptr.is_null() || len == 0 {
                return None;
            }
            let start = ptr as usize - buf.as_ptr() as usize;
            buf.get(start..)
                .map(|rest| &rest[..rest.len().min(len as usize * 2)])
        };
        let mut langs: Vec<(u16, u16)> = query(r"\VarFileInfo\Translation")
            .map(|t| {
                t.chunks_exact(4)
                    .map(|c| {
                        (
                            u16::from_le_bytes([c[0], c[1]]),
                            u16::from_le_bytes([c[2], c[3]]),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        langs.extend([(0x0409, 0x04B0), (0x0409, 0x04E4), (0x0000, 0x04B0)]);
        for (lang, cp) in langs {
            let Some(bytes) = query(&format!(r"\StringFileInfo\{lang:04x}{cp:04x}\{field}")) else {
                continue;
            };
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&u| u != 0)
                .collect();
            let s = String::from_utf16_lossy(&units).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
        None
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;

    use super::*;

    pub fn run_values(_rk: &RunKey) -> Result<Vec<(String, String)>, String> {
        Ok(Vec::new())
    }

    pub fn approval(_h: Hive, _approved: &str, _name: &str) -> Approval {
        parse_approval(None)
    }

    pub fn set_approval(
        _h: Hive,
        _approved: &str,
        _name: &str,
        _data: &[u8],
    ) -> Result<(), String> {
        Err("The startup manager only works on Windows.".into())
    }

    pub fn shortcut_target(_lnk: &Path) -> Option<(String, String)> {
        None
    }

    pub fn logon_tasks() -> Result<Vec<StartupItem>, String> {
        Ok(Vec::new())
    }

    pub fn set_task_enabled(_path: &str, _enabled: bool) -> Result<(), String> {
        Err("The startup manager only works on Windows.".into())
    }

    pub fn publisher(_program: &Path) -> Option<String> {
        None
    }
}

/// The company name in a program's version information (Windows only).
pub fn publisher(program: &Path) -> Option<String> {
    imp::publisher(program)
}

/// Services set to start automatically, other than Windows' own.
pub fn services() -> Result<Vec<ServiceItem>, String> {
    crate::services::auto_start()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_give_their_program() {
        let p = |c| program_from_command(c);
        assert_eq!(
            p(r#""C:\Program Files\App\app.exe" --min"#).as_deref(),
            Some(r"C:\Program Files\App\app.exe")
        );
        assert_eq!(
            p(r"C:\Program Files\App\app.exe --min").as_deref(),
            Some(r"C:\Program Files\App\app.exe")
        );
        assert_eq!(p(r"C:\Tools\run.cmd").as_deref(), Some(r"C:\Tools\run.cmd"));
        assert_eq!(p("ctfmon.exe").as_deref(), Some("ctfmon.exe"));
        assert_eq!(
            p(r"C:\a.exe.config\b.exe /x").as_deref(),
            Some(r"C:\a.exe.config\b.exe")
        );
        assert_eq!(p("tool -x").as_deref(), Some("tool"));
        assert_eq!(p(r#""unterminated"#).as_deref(), Some("unterminated"));
        assert_eq!(p("   "), None);
        assert_eq!(p(r#""""#), None);

        assert_eq!(
            target_program(r"C:\Windows\System32\rundll32.exe C:\Vendor\helper.dll,Start")
                .as_deref(),
            Some(r"C:\Vendor\helper.dll")
        );
        assert_eq!(
            target_program(r#""C:\Windows\System32\rundll32.exe" "C:\Vendor X\h.dll",Run"#)
                .as_deref(),
            Some(r"C:\Vendor X\h.dll")
        );
        assert_eq!(
            target_program("rundll32.exe").as_deref(),
            Some("rundll32.exe")
        );
    }

    #[test]
    fn variables_expand() {
        let lookup = |n: &str| match n.to_ascii_lowercase().as_str() {
            "programfiles" => Some(r"C:\Program Files".to_string()),
            _ => None,
        };
        assert_eq!(
            expand_vars(r"%ProgramFiles%\App\a.exe", lookup),
            r"C:\Program Files\App\a.exe"
        );
        assert_eq!(expand_vars(r"%Nope%\x 100%", lookup), r"%Nope%\x 100%");
        assert_eq!(expand_vars("%%", lookup), "%%");
    }

    #[test]
    fn approval_values_round_trip() {
        assert!(parse_approval(None).enabled);
        assert!(parse_approval(Some(&[])).enabled);
        let on = approval_bytes(true, 1_700_000_000);
        assert_eq!(on, [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            parse_approval(Some(&on)),
            Approval {
                enabled: true,
                disabled_at: None
            }
        );
        let off = approval_bytes(false, 1_700_000_000);
        assert_eq!(off[0], 3);
        assert_eq!(
            parse_approval(Some(&off)),
            Approval {
                enabled: false,
                disabled_at: Some(1_700_000_000)
            }
        );
        // Other values Windows uses: even is on, odd is off.
        assert!(parse_approval(Some(&[6, 0, 0, 0])).enabled);
        assert!(!parse_approval(Some(&[7, 0, 0, 0])).enabled);
        assert_eq!(parse_approval(Some(&[3, 0, 0, 0])).disabled_at, None);
    }

    #[test]
    fn startup_folder_files_are_listed_without_desktop_ini() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("desktop.ini"), "x").unwrap();
        std::fs::write(tmp.path().join("tool.cmd"), "@echo off").unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        let entries = folder_entries(tmp.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "tool.cmd");
        assert!(entries[0].1.ends_with("tool.cmd"));
    }

    /// Uses a throwaway key under HKCU and a temporary folder, never the
    /// real startup locations.
    #[cfg(windows)]
    #[test]
    fn items_can_be_turned_off_and_on_without_deleting_them() {
        use crate::winreg::{self, KEY_QUERY_VALUE, KEY_SET_VALUE, Key};

        let base = format!(
            r"Software\WinFastCleaner-test-startup-{}",
            std::process::id()
        );
        let tmp = tempfile::tempdir().unwrap();
        let cmd = crate::platform::windows_dir()
            .unwrap()
            .join(r"System32\cmd.exe");
        {
            let run = Key::create(
                winreg::Hive::CurrentUser,
                &format!(r"{base}\Run"),
                KEY_SET_VALUE | KEY_QUERY_VALUE,
            )
            .unwrap();
            winreg::test_support::set_string(
                &run,
                "Test App",
                r#""C:\Program Files\Test App\app.exe" --min"#,
                false,
            );
            winreg::test_support::set_string(
                &run,
                "Notes",
                r"%SystemRoot%\System32\cmd.exe /d",
                true,
            );
        }
        std::fs::write(tmp.path().join("tool.cmd"), "@echo off").unwrap();
        crate::wincom::test_support::create_shortcut(&tmp.path().join("Editor.lnk"), &cmd, "/a")
            .unwrap();

        let loc = Locations {
            run_keys: vec![RunKey {
                source: Source::UserRun,
                hive: Hive::CurrentUser,
                key: format!(r"{base}\Run"),
                wow32: false,
                approved: format!(r"{base}\Approved\Run"),
            }],
            folders: vec![StartupFolder {
                source: Source::UserFolder,
                dir: tmp.path().to_path_buf(),
                hive: Hive::CurrentUser,
                approved: format!(r"{base}\Approved\StartupFolder"),
            }],
            tasks: false,
        };

        let list1 = list(&loc);
        assert!(list1.errors.is_empty(), "{:?}", list1.errors);
        let names: Vec<&str> = list1.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Editor", "Notes", "Test App", "tool"]);
        assert!(list1.items.iter().all(|i| i.enabled && !i.machine));
        assert_eq!(list1.enabled_count(), 4);
        let notes = list1.items.iter().find(|i| i.name == "Notes").unwrap();
        assert!(
            Path::new(notes.program.as_deref().unwrap()).is_absolute(),
            "{notes:?}"
        );
        assert_eq!(notes.publisher.as_deref(), Some("Microsoft Corporation"));
        let editor = list1.items.iter().find(|i| i.name == "Editor").unwrap();
        assert!(
            editor.command.to_ascii_lowercase().contains("cmd.exe"),
            "{editor:?}"
        );
        assert!(editor.command.ends_with("/a"), "{editor:?}");

        for name in ["Test App", "tool", "Editor"] {
            let it = list(&loc)
                .items
                .into_iter()
                .find(|i| i.name == name)
                .unwrap();
            set_enabled(&loc, &it, false).unwrap();
            let it = list(&loc)
                .items
                .into_iter()
                .find(|i| i.name == name)
                .unwrap();
            assert!(!it.enabled, "{name}");
            assert!(it.disabled_at.is_some(), "{name}");
            set_enabled(&loc, &it, true).unwrap();
            let it = list(&loc)
                .items
                .into_iter()
                .find(|i| i.name == name)
                .unwrap();
            assert!(it.enabled, "{name}");
        }
        let it = list(&loc)
            .items
            .into_iter()
            .find(|i| i.name == "Test App")
            .unwrap();
        set_enabled(&loc, &it, false).unwrap();
        assert_eq!(list(&loc).enabled_count(), 3);

        // Nothing was deleted: the Run value and the files are still there,
        // and the switch is Task Manager's format.
        let run = Key::open(
            winreg::Hive::CurrentUser,
            &format!(r"{base}\Run"),
            KEY_QUERY_VALUE,
        )
        .unwrap()
        .unwrap();
        assert!(run.get("Test App").is_some());
        assert!(tmp.path().join("tool.cmd").exists());
        assert!(tmp.path().join("Editor.lnk").exists());
        let approved = Key::open(
            winreg::Hive::CurrentUser,
            &format!(r"{base}\Approved\Run"),
            KEY_QUERY_VALUE,
        )
        .unwrap()
        .unwrap();
        let v = approved.get("Test App").unwrap();
        assert_eq!(v.data.len(), 12);
        assert_eq!(v.data[0], 3);

        winreg::test_support::delete_tree(&base);
    }

    #[cfg(windows)]
    #[test]
    fn real_startup_items_and_services_can_be_read() {
        let l = list(&Locations::system());
        assert!(l.errors.is_empty(), "{:?}", l.errors);
        for i in &l.items {
            assert!(!i.id.is_empty() && !i.name.is_empty(), "{i:?}");
        }
        let services = services().unwrap();
        let windir = crate::platform::windows_dir().unwrap();
        for s in &services {
            if let Some(p) = &s.program {
                assert!(
                    !crate::safety::Guard::is_within(Path::new(p), &windir),
                    "{s:?}"
                );
            }
        }
    }
}
