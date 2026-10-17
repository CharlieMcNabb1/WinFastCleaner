//! Privacy cleaning: Windows traces of what you did.
//!
//! This never reads, exports, or logs the contents of what it clears. It only
//! counts items (file names and registry value names are not even returned)
//! and removes them:
//!
//! - **Recent documents**: Windows' own "clear recent documents" call
//!   (`SHAddToRecentDocs` with no item), then any shortcuts left in the
//!   Recent folder.
//! - **Jump lists**: the jump list files in the Recent folder.
//! - **Run history**: the values of your own `RunMRU` registry key (the Run
//!   dialog's history). Nothing else in the registry is touched.
//! - **Clipboard**: what is copied right now.
//!
//! Browser history, cookies, and sessions are not part of this yet.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cleaner::SkippedItem;
use crate::fsops;
use crate::log::{self, Action};
use crate::safety::Guard;

/// A kind of trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Trace {
    RecentDocuments,
    JumpLists,
    RunHistory,
    Clipboard,
}

impl Trace {
    pub const ALL: [Trace; 4] = [
        Trace::RecentDocuments,
        Trace::JumpLists,
        Trace::RunHistory,
        Trace::Clipboard,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Trace::RecentDocuments => "Recent documents",
            Trace::JumpLists => "Jump lists",
            Trace::RunHistory => "Run history",
            Trace::Clipboard => "Clipboard",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Trace::RecentDocuments => {
                "The list of files you opened recently, shown in File Explorer's Quick access and Start."
            }
            Trace::JumpLists => {
                "The recent and frequent items shown when you right-click an app on the taskbar or Start."
            }
            Trace::RunHistory => "Commands you typed in the Run dialog (Windows key + R).",
            Trace::Clipboard => "What is copied right now.",
        }
    }

    pub fn lose(self) -> &'static str {
        match self {
            Trace::RecentDocuments => {
                "The recent files lists. The files themselves are not touched."
            }
            Trace::JumpLists => {
                "Recent items in jump lists, and items you pinned to them. The files themselves are not touched."
            }
            Trace::RunHistory => "The Run dialog's suggestions.",
            Trace::Clipboard => {
                "What you copied. Clipboard history (Windows key + V) is kept; clear it in Windows Settings."
            }
        }
    }
}

/// Where traces are. [`Locations::system`] for the real thing; tests point
/// these at temporary folders and a test registry key.
#[derive(Clone, Debug)]
pub struct Locations {
    /// `%APPDATA%\Microsoft\Windows\Recent`.
    pub recent: Option<PathBuf>,
    /// Use Windows' own "clear recent documents" call (off in tests, so they
    /// never touch the real list).
    pub use_shell: bool,
    /// Registry key under HKEY_CURRENT_USER holding the Run history.
    pub run_mru_key: String,
    /// Whether to touch the real clipboard (off in tests).
    pub clipboard: bool,
}

const RUN_MRU: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU";

impl Locations {
    pub fn system() -> Locations {
        let recent = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|p| crate::platform::long_path(&p.join(r"Microsoft\Windows\Recent")));
        Locations {
            recent,
            use_shell: cfg!(windows),
            run_mru_key: RUN_MRU.to_string(),
            clipboard: cfg!(windows),
        }
    }

    fn jump_list_dirs(&self) -> Vec<(PathBuf, &'static str)> {
        match &self.recent {
            Some(r) => vec![
                (r.join("AutomaticDestinations"), ".automaticdestinations-ms"),
                (r.join("CustomDestinations"), ".customdestinations-ms"),
            ],
            None => Vec::new(),
        }
    }
}

/// What the Privacy tab shows for one trace.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceInfo {
    pub trace: Trace,
    pub name: &'static str,
    pub description: &'static str,
    pub lose: &'static str,
    /// How many items there are (`None` when unknown).
    pub items: Option<u64>,
    /// False when this trace doesn't exist on this system.
    pub available: bool,
}

/// Counts each trace. Never reads contents.
pub fn survey(loc: &Locations) -> Vec<TraceInfo> {
    Trace::ALL
        .iter()
        .map(|&t| {
            let items = count(t, loc);
            TraceInfo {
                trace: t,
                name: t.name(),
                description: t.description(),
                lose: t.lose(),
                available: items.is_some(),
                items,
            }
        })
        .collect()
}

fn count(t: Trace, loc: &Locations) -> Option<u64> {
    match t {
        Trace::RecentDocuments => {
            let dir = loc.recent.as_ref()?;
            Some(files_with_suffix(dir, ".lnk").len() as u64)
        }
        Trace::JumpLists => {
            loc.recent.as_ref()?;
            Some(
                loc.jump_list_dirs()
                    .iter()
                    .map(|(d, sfx)| files_with_suffix(d, sfx).len() as u64)
                    .sum(),
            )
        }
        Trace::RunHistory => win::run_history_count(&loc.run_mru_key),
        Trace::Clipboard => {
            if loc.clipboard {
                win::clipboard_has_data().map(u64::from)
            } else {
                None
            }
        }
    }
}

/// Regular files (not links) directly in `dir` whose name ends with
/// `suffix` (ignoring case).
fn files_with_suffix(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file() && !t.is_symlink()))
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .to_lowercase()
                .ends_with(suffix)
        })
        .map(|e| e.path())
        .collect()
}

/// The result of clearing one trace.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceResult {
    pub trace: Trace,
    /// Items removed.
    pub cleared: u64,
    pub skipped: Vec<SkippedItem>,
    /// Why the whole trace couldn't be cleared, if it couldn't.
    pub error: Option<String>,
}

/// Clears the chosen traces and logs what was removed (names of files, never
/// their contents).
pub fn clear(
    traces: &[Trace],
    loc: &Locations,
    guard: &Guard,
    data_dir: &Path,
) -> Vec<TraceResult> {
    let now = crate::scan::now_unix();
    let mut results = Vec::new();
    let mut logged: Vec<(PathBuf, u64, Option<String>)> = Vec::new();
    for &t in traces {
        let mut r = TraceResult {
            trace: t,
            cleared: 0,
            skipped: Vec::new(),
            error: None,
        };
        match t {
            Trace::RecentDocuments => match &loc.recent {
                Some(dir) => {
                    let before = files_with_suffix(dir, ".lnk").len() as u64;
                    if loc.use_shell {
                        win::clear_recent_documents();
                    }
                    // Whatever the shell left behind.
                    let left = files_with_suffix(dir, ".lnk");
                    let deleted = delete_files(&left, guard, &mut r.skipped, &mut logged);
                    let after = before.saturating_sub(left.len() as u64) + deleted;
                    r.cleared = after.min(before);
                }
                None => r.error = Some("The Recent folder was not found.".into()),
            },
            Trace::JumpLists => {
                if loc.recent.is_none() {
                    r.error = Some("The Recent folder was not found.".into());
                }
                for (dir, sfx) in loc.jump_list_dirs() {
                    let files = files_with_suffix(&dir, sfx);
                    r.cleared += delete_files(&files, guard, &mut r.skipped, &mut logged);
                }
            }
            Trace::RunHistory => match win::clear_run_history(&loc.run_mru_key) {
                Ok(n) => {
                    r.cleared = n;
                    logged.push((
                        PathBuf::from(format!(r"HKEY_CURRENT_USER\{}", loc.run_mru_key)),
                        0,
                        Some(format!("cleared {n} entries")),
                    ));
                }
                Err(e) => r.error = Some(e),
            },
            Trace::Clipboard => {
                let res = if loc.clipboard {
                    win::clear_clipboard()
                } else {
                    Err("The clipboard is not available.".into())
                };
                match res {
                    Ok(()) => {
                        r.cleared = 1;
                        logged.push((PathBuf::from("clipboard"), 0, Some("emptied".into())));
                    }
                    Err(e) => r.error = Some(e),
                }
            }
        }
        results.push(r);
    }
    let entries: Vec<log::Entry<'_>> = logged
        .iter()
        .map(|(p, size, detail)| log::Entry {
            time: now,
            batch: None,
            action: Action::Deleted,
            tier: None,
            path: p,
            size: *size,
            detail: detail.as_deref(),
        })
        .collect();
    let _ = log::append(data_dir, &entries);
    results
}

/// Deletes files through link-safe handles, after the guard. Returns how
/// many were deleted.
fn delete_files(
    files: &[PathBuf],
    guard: &Guard,
    skipped: &mut Vec<SkippedItem>,
    logged: &mut Vec<(PathBuf, u64, Option<String>)>,
) -> u64 {
    let mut n = 0;
    for f in files {
        let result = (|| -> Result<u64, String> {
            let h = fsops::open(f).map_err(|e| fsops::describe_error(&e))?;
            let info = h.info().map_err(|e| fsops::describe_error(&e))?;
            if info.is_dir || info.is_special {
                return Err("Links and folders are not deleted".into());
            }
            let fin = h.final_path().map_err(|e| fsops::describe_error(&e))?;
            guard.check(&fin).map_err(|b| format!("Protected: {b}"))?;
            h.delete().map_err(|e| fsops::describe_error(&e))?;
            Ok(info.size)
        })();
        match result {
            Ok(size) => {
                n += 1;
                logged.push((f.clone(), size, None));
            }
            Err(reason) => skipped.push(SkippedItem {
                path: f.clone(),
                reason,
            }),
        }
    }
    n
}

#[cfg(windows)]
mod win {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, CountClipboardFormats, EmptyClipboard, OpenClipboard,
    };
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, RegCloseKey, RegDeleteValueW,
        RegEnumValueW, RegOpenKeyExW,
    };
    use windows_sys::Win32::UI::Shell::{SHARD_PIDL, SHAddToRecentDocs};

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the key was opened by `open_key` and is closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn open_key(path: &str, access: u32) -> Result<Option<Key>, String> {
        let w = wide(path);
        let mut key: HKEY = null_mut();
        // SAFETY: `w` is NUL-terminated and `key` is a valid out-pointer.
        let err = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, w.as_ptr(), 0, access, &mut key) };
        match err {
            ERROR_SUCCESS => Ok(Some(Key(key))),
            2 => Ok(None), // ERROR_FILE_NOT_FOUND: no history yet
            e => Err(format!("Could not open the Run history (error {e}).")),
        }
    }

    /// Value names only (never the data).
    fn value_names(key: &Key) -> Vec<Vec<u16>> {
        let mut names = Vec::new();
        for i in 0.. {
            let mut buf = vec![0u16; 16_384];
            let mut len = buf.len() as u32;
            // SAFETY: the buffer and its length match; data pointers are null
            // because only names are read.
            let err = unsafe {
                RegEnumValueW(
                    key.0,
                    i,
                    buf.as_mut_ptr(),
                    &mut len,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                )
            };
            if err != ERROR_SUCCESS {
                break;
            }
            buf.truncate(len as usize);
            names.push(buf);
        }
        names
    }

    fn is_mru_list(name: &[u16]) -> bool {
        String::from_utf16_lossy(name).eq_ignore_ascii_case("MRUList")
    }

    pub fn run_history_count(path: &str) -> Option<u64> {
        match open_key(path, KEY_QUERY_VALUE) {
            Ok(Some(key)) => {
                Some(value_names(&key).iter().filter(|n| !is_mru_list(n)).count() as u64)
            }
            Ok(None) => Some(0),
            Err(_) => None,
        }
    }

    pub fn clear_run_history(path: &str) -> Result<u64, String> {
        let Some(key) = open_key(path, KEY_QUERY_VALUE | KEY_SET_VALUE)? else {
            return Ok(0);
        };
        let names = value_names(&key);
        let mut cleared = 0;
        for name in &names {
            let mut z = name.clone();
            z.push(0);
            // SAFETY: `z` is a NUL-terminated value name of this key.
            if unsafe { RegDeleteValueW(key.0, z.as_ptr()) } == ERROR_SUCCESS && !is_mru_list(name)
            {
                cleared += 1;
            }
        }
        Ok(cleared)
    }

    pub fn clear_recent_documents() {
        // SAFETY: a null item with SHARD_PIDL is the documented way to clear
        // the recent documents list.
        unsafe { SHAddToRecentDocs(SHARD_PIDL as u32, std::ptr::null()) };
    }

    pub fn clipboard_has_data() -> Option<bool> {
        // SAFETY: no preconditions; only counts formats, never reads data.
        Some(unsafe { CountClipboardFormats() } > 0)
    }

    pub fn clear_clipboard() -> Result<(), String> {
        // Another program may hold the clipboard for a moment.
        for _ in 0..10 {
            // SAFETY: a null owner window is allowed.
            if unsafe { OpenClipboard(null_mut()) } != 0 {
                // SAFETY: the clipboard is open by this thread.
                let ok = unsafe { EmptyClipboard() } != 0;
                // SAFETY: closes what was opened above.
                unsafe { CloseClipboard() };
                return if ok {
                    Ok(())
                } else {
                    Err("Windows did not let the clipboard be emptied.".into())
                };
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Err("Another program is using the clipboard. Try again.".into())
    }
}

#[cfg(not(windows))]
mod win {
    pub fn run_history_count(_path: &str) -> Option<u64> {
        None
    }

    pub fn clear_run_history(_path: &str) -> Result<u64, String> {
        Err("Run history only exists on Windows.".into())
    }

    pub fn clear_recent_documents() {}

    pub fn clipboard_has_data() -> Option<bool> {
        None
    }

    pub fn clear_clipboard() -> Result<(), String> {
        Err("The clipboard is only cleared on Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locations(recent: &Path, key: &str) -> Locations {
        Locations {
            recent: Some(recent.to_path_buf()),
            use_shell: false,
            run_mru_key: key.to_string(),
            clipboard: false,
        }
    }

    #[test]
    fn recent_items_and_jump_lists_are_counted_and_cleared() {
        let tmp = tempfile::tempdir().unwrap();
        let base = crate::platform::simplify_path(std::fs::canonicalize(tmp.path()).unwrap());
        let recent = base.join("Recent");
        let auto = recent.join("AutomaticDestinations");
        let custom = recent.join("CustomDestinations");
        for d in [&auto, &custom] {
            std::fs::create_dir_all(d).unwrap();
        }
        for n in ["a.docx.lnk", "b.pdf.LNK"] {
            std::fs::write(recent.join(n), b"shortcut").unwrap();
        }
        std::fs::write(recent.join("desktop.ini"), b"keep").unwrap();
        std::fs::write(auto.join("1b4dd67f29cb1962.automaticDestinations-ms"), b"j").unwrap();
        std::fs::write(custom.join("590aee7bdd69b59b.customDestinations-ms"), b"j").unwrap();
        std::fs::write(custom.join("other.txt"), b"keep").unwrap();
        let loc = locations(&recent, r"Software\WinFastCleaner-test-none");

        let survey = survey(&loc);
        let get = |t: Trace| survey.iter().find(|i| i.trace == t).unwrap().items;
        assert_eq!(get(Trace::RecentDocuments), Some(2));
        assert_eq!(get(Trace::JumpLists), Some(2));
        assert_eq!(get(Trace::Clipboard), None, "not touched in tests");

        let data = base.join("data");
        let guard = Guard::system();
        let r = clear(
            &[Trace::RecentDocuments, Trace::JumpLists],
            &loc,
            &guard,
            &data,
        );
        assert_eq!(r[0].cleared, 2, "{:?}", r[0]);
        assert_eq!(r[1].cleared, 2, "{:?}", r[1]);
        assert!(
            recent.join("desktop.ini").exists(),
            "only shortcuts are removed"
        );
        assert!(custom.join("other.txt").exists());
        let log = std::fs::read_to_string(data.join("logs/cleanup.log")).unwrap();
        assert_eq!(log.lines().count(), 4);
        assert!(!log.contains("shortcut"), "contents are never logged");
    }

    #[cfg(unix)]
    #[test]
    fn links_in_the_recent_folder_are_not_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let recent = tmp.path().join("Recent");
        std::fs::create_dir_all(&recent).unwrap();
        let outside = tmp.path().join("important.lnk");
        std::fs::write(&outside, b"keep").unwrap();
        std::os::unix::fs::symlink(&outside, recent.join("trap.lnk")).unwrap();
        let loc = locations(&recent, "none");
        assert_eq!(survey(&loc)[0].items, Some(0), "links are not counted");
        clear(
            &[Trace::RecentDocuments],
            &loc,
            &Guard::system(),
            &tmp.path().join("d"),
        );
        assert!(outside.exists());
    }

    /// Uses a throwaway key under HKCU, never the real Run history.
    #[cfg(windows)]
    #[test]
    fn run_history_values_are_cleared() {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
        };
        let parent = format!(r"Software\WinFastCleaner-test-{}", std::process::id());
        let path = format!(r"{parent}\RunMRU");
        let w = |s: &str| -> Vec<u16> {
            std::ffi::OsStr::new(s)
                .encode_wide()
                .chain(Some(0))
                .collect()
        };
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: strings are NUL-terminated and `key` is a valid out-pointer.
        let err = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                w(&path).as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(err, 0);
        for (name, data) in [("a", "notepad\\1"), ("b", "cmd\\1"), ("MRUList", "ba")] {
            let d = w(data);
            // SAFETY: `d` is a NUL-terminated UTF-16 string of the size passed.
            unsafe {
                RegSetValueExW(
                    key,
                    w(name).as_ptr(),
                    0,
                    REG_SZ,
                    d.as_ptr().cast(),
                    (d.len() * 2) as u32,
                )
            };
        }
        // SAFETY: closes the key created above.
        unsafe { RegCloseKey(key) };

        let tmp = tempfile::tempdir().unwrap();
        let loc = locations(tmp.path(), &path);
        let s = survey(&loc);
        assert_eq!(
            s.iter()
                .find(|i| i.trace == Trace::RunHistory)
                .unwrap()
                .items,
            Some(2)
        );
        let r = clear(
            &[Trace::RunHistory],
            &loc,
            &Guard::system(),
            &tmp.path().join("d"),
        );
        assert_eq!(r[0].cleared, 2, "{:?}", r[0]);
        assert_eq!(win::run_history_count(&path), Some(0));

        // SAFETY: deletes only the throwaway test key.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, w(&parent).as_ptr()) };
    }
}
