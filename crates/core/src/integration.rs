//! The two things WinFastCleaner adds to Windows outside its own folders,
//! both only when the user turns them on in Settings:
//!
//! - the scheduled cleaning task (`\WinFastCleaner\Scheduled cleaning`), and
//! - the tray monitor's sign-in entry, a value in the user's own `Run` key.
//!
//! [`remove_all`] takes both away again (the uninstaller calls it through
//! `wfc remove-integration`). The data folder (settings, logs, and the
//! recovery bin, which may hold the user's files) is never touched here.

use std::path::{Path, PathBuf};

/// The tray monitor's program, next to the app.
pub const TRAY_PROGRAM: &str = "wfc-tray.exe";
/// Window class of the tray's hidden window, used to ask it to quit.
pub const TRAY_WINDOW_CLASS: &str = "WinFastCleanerTrayWindow";
/// Name of the tray's value in `HKCU\...\Run`.
pub const RUN_VALUE: &str = "WinFastCleaner tray monitor";
#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// A program that ships next to the running one (`wfcw.exe`,
/// `wfc-tray.exe`, the app).
pub fn sibling(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join(name);
    p.is_file().then_some(p)
}

/// The command line the sign-in entry runs.
pub fn tray_command(program: &Path) -> String {
    format!("\"{}\"", program.display())
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::winreg::{Hive, KEY_QUERY_VALUE, KEY_SET_VALUE, Key};

    /// Adds (with `Some`) or removes the tray's sign-in entry.
    pub fn set_tray_autostart(program: Option<&Path>) -> Result<(), String> {
        let key = Key::create(Hive::CurrentUser, RUN_KEY, KEY_SET_VALUE)
            .map_err(|e| format!("The sign-in entry could not be changed (error {e})."))?;
        match program {
            Some(p) => key.set_string(RUN_VALUE, &tray_command(p)),
            None => key.delete_value(RUN_VALUE),
        }
        .map_err(|e| format!("The sign-in entry could not be changed (error {e})."))
    }

    /// The tray's sign-in entry, if there is one.
    pub fn tray_autostart() -> Option<String> {
        Key::open(Hive::CurrentUser, RUN_KEY, KEY_QUERY_VALUE)
            .ok()
            .flatten()?
            .get(RUN_VALUE)?
            .text()
            .map(|(t, _)| t)
    }

    /// The tray's hidden window (a top-level window that is never shown).
    fn tray_window() -> windows_sys::Win32::Foundation::HWND {
        use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;
        let class: Vec<u16> = TRAY_WINDOW_CLASS.encode_utf16().chain(Some(0)).collect();
        // SAFETY: `class` is NUL-terminated; a null title matches any.
        unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) }
    }

    /// Whether the tray monitor is running (for this user).
    pub fn tray_running() -> bool {
        !tray_window().is_null()
    }

    /// Asks a running tray monitor to quit. Returns whether one was running.
    pub fn close_tray() -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        let hwnd = tray_window();
        if hwnd.is_null() {
            return false;
        }
        // SAFETY: posting WM_CLOSE to a window handle is always allowed; a
        // stale handle just fails.
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
        true
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn set_tray_autostart(_program: Option<&Path>) -> Result<(), String> {
        Err("The tray monitor only works on Windows.".into())
    }

    pub fn tray_autostart() -> Option<String> {
        None
    }

    pub fn tray_running() -> bool {
        false
    }

    pub fn close_tray() -> bool {
        false
    }
}

pub use imp::*;

/// Removes the scheduled task and the tray's sign-in entry, and closes the
/// tray. Returns the problems (empty when everything is gone).
pub fn remove_all() -> Vec<String> {
    let mut problems = Vec::new();
    #[cfg(windows)]
    if let Err(e) =
        crate::wincom::delete_task(crate::schedule::TASK_FOLDER, crate::schedule::TASK_NAME)
    {
        problems.push(format!("Scheduled task: {e}"));
    }
    if cfg!(windows) {
        if let Err(e) = set_tray_autostart(None) {
            problems.push(format!("Tray sign-in entry: {e}"));
        }
        close_tray();
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_command_is_quoted() {
        assert_eq!(
            tray_command(Path::new(r"C:\Program Files\WinFastCleaner\wfc-tray.exe")),
            r#""C:\Program Files\WinFastCleaner\wfc-tray.exe""#
        );
    }
}
