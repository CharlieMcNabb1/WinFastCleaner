//! Which programs are running (to skip rules whose app must be closed).
//! Only program names are read, nothing else about the processes.

use std::collections::HashSet;

/// Lowercase program file names of running processes, such as
/// `chrome.exe`.
pub fn running_programs() -> HashSet<String> {
    imp::running_programs()
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    pub fn running_programs() -> HashSet<String> {
        let mut names = HashSet::new();
        // SAFETY: no pointers are passed.
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            return names;
        }
        // SAFETY: all-zero is valid for this plain C struct; dwSize is set
        // as the API requires.
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        // SAFETY: `snap` is a valid snapshot handle; `entry` is a valid,
        // initialized out-pointer.
        let mut ok = unsafe { Process32FirstW(snap, &mut entry) };
        while ok != 0 {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = OsString::from_wide(&entry.szExeFile[..len]);
            names.insert(name.to_string_lossy().to_lowercase());
            // SAFETY: as above.
            ok = unsafe { Process32NextW(snap, &mut entry) };
        }
        // SAFETY: `snap` is a handle we own.
        unsafe { CloseHandle(snap) };
        names
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn running_programs() -> HashSet<String> {
        let mut names = HashSet::new();
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for e in entries.flatten() {
                if let Ok(comm) = std::fs::read_to_string(e.path().join("comm")) {
                    names.insert(comm.trim().to_lowercase());
                }
                // `comm` is cut to 15 characters and some apps rename it, so
                // also use the program file's name (only readable for our own
                // processes, which are the ones that matter).
                if let Some(exe) = std::fs::read_link(e.path().join("exe"))
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
                {
                    names.insert(exe);
                }
            }
        }
        names
    }
}
