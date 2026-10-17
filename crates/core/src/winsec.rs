//! Windows permissions for folders the app creates outside the user's
//! profile (the recovery bin on other drives).

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, READ_CONTROL,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const ADMINISTRATORS: &str = "S-1-5-32-544";
const LOCAL_SYSTEM: &str = "S-1-5-18";

/// Who may use a new folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirAccess {
    /// Only the current user, SYSTEM, and administrators; nothing is
    /// inherited from the parent.
    Private,
    /// A shared parent: any signed-in user may list it and create their own
    /// subfolder, but not rename, delete, or write files in it.
    SharedRoot,
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

/// The SID of the user running this process, such as `S-1-5-21-…-1001`.
pub fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle; `token` is a valid
    // out-pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut len = 0u32;
    // SAFETY: a size query with no buffer; `len` receives the needed size.
    unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len) };
    // u64 elements keep the buffer aligned for TOKEN_USER.
    let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
    // SAFETY: the buffer holds at least `len` bytes.
    let ok =
        unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) };
    let err = io::Error::last_os_error();
    // SAFETY: `token` was opened above and is closed once.
    unsafe { CloseHandle(token) };
    if ok == 0 {
        return Err(err);
    }
    // SAFETY: on success the buffer starts with an aligned TOKEN_USER whose
    // SID points into the same buffer, which is still alive.
    let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    sid_to_string(sid).ok_or_else(io::Error::last_os_error)
}

fn sid_to_string(sid: PSID) -> Option<String> {
    let mut out: *mut u16 = null_mut();
    // SAFETY: `sid` is a valid SID; `out` receives a LocalAlloc'd string.
    if unsafe { ConvertSidToStringSidW(sid, &mut out) } == 0 || out.is_null() {
        return None;
    }
    // SAFETY: `out` is a NUL-terminated string from the call above; it is
    // read before being freed once.
    let s = unsafe {
        let len = (0..).take_while(|&i| *out.add(i) != 0).count();
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(out, len));
        LocalFree(out.cast());
        s
    };
    Some(s)
}

/// Creates one folder with explicit permissions that do not inherit from
/// its parent (so a drive root's broad permissions never apply).
pub fn create_dir(path: &Path, access: DirAccess) -> io::Result<()> {
    let sddl = match access {
        DirAccess::Private => {
            let user = current_user_sid()?;
            format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{user})")
        }
        // Authenticated users: list, traverse, read attributes, add a
        // subfolder, read permissions (this folder only). The owner may also
        // set attributes (to hide it) and permissions.
        DirAccess::SharedRoot => {
            "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;;0x1200ad;;;AU)(A;;0x1601ad;;;OW)".to_string()
        }
    };
    let sddl_w = wide(OsStr::new(&sddl));
    let mut sd: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: `sddl_w` is NUL-terminated; `sd` receives a LocalAlloc'd
    // descriptor that is freed below.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl_w.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let sa = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };
    let path_w = wide(path.as_os_str());
    // SAFETY: `path_w` is NUL-terminated and `sa` points to a valid
    // descriptor for the duration of the call.
    let created = unsafe { CreateDirectoryW(path_w.as_ptr(), &sa) };
    let err = io::Error::last_os_error();
    // SAFETY: `sd` came from the conversion above and is freed once.
    unsafe { LocalFree(sd) };
    if created == 0 {
        return Err(err);
    }
    Ok(())
}

/// The owner of a file or folder (opened without following links).
pub fn owner_of(path: &Path) -> io::Result<Option<String>> {
    let f = std::fs::OpenOptions::new()
        .access_mode(READ_CONTROL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let mut owner: PSID = null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: the handle is open for READ_CONTROL; `owner` points into `sd`,
    // which is freed after the owner is read.
    let err = unsafe {
        GetSecurityInfo(
            f.as_raw_handle() as HANDLE,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut sd,
        )
    };
    if err != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(err as i32));
    }
    let s = if owner.is_null() {
        None
    } else {
        sid_to_string(owner)
    };
    // SAFETY: `sd` came from GetSecurityInfo and is freed once.
    unsafe { LocalFree(sd) };
    Ok(s)
}

/// Whether `path` is owned by the current user, administrators, or SYSTEM.
pub fn owned_by_user_or_admins(path: &Path) -> io::Result<bool> {
    let Some(owner) = owner_of(path)? else {
        return Ok(false);
    };
    Ok(owner == ADMINISTRATORS || owner == LOCAL_SYSTEM || owner == current_user_sid()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_folders_belong_to_the_user() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("private");
        create_dir(&dir, DirAccess::Private).unwrap();
        assert!(dir.is_dir());
        assert!(owned_by_user_or_admins(&dir).unwrap());
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
        // Files can still be created inside.
        std::fs::write(dir.join("f"), b"x").unwrap();

        let shared = tmp.path().join("shared");
        create_dir(&shared, DirAccess::SharedRoot).unwrap();
        create_dir(&shared.join("me"), DirAccess::Private).unwrap();
        assert!(
            create_dir(&dir, DirAccess::Private).is_err(),
            "already exists"
        );
    }
}
