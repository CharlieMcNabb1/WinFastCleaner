//! Small OS-specific helpers: drives, free space, elevation, and file
//! details the standard library does not expose.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Kind of drive, as reported by Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Optical,
    RamDisk,
    Unknown,
}

/// A mounted drive.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    /// Root path, for example `C:\`.
    pub root: String,
    pub label: String,
    /// File system name, for example `NTFS`.
    pub file_system: String,
    pub kind: DriveKind,
    pub total: u64,
    pub free: u64,
    /// Whether Windows is installed on this drive.
    pub is_system: bool,
}

/// Total and free bytes of the volume containing `path`.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeSpace {
    pub total: u64,
    pub free: u64,
}

/// Removes the `\\?\` prefix that `canonicalize` adds on Windows, when the
/// path can be written without it.
pub fn simplify_path(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && rest.len() < 260 {
            return PathBuf::from(rest);
        }
    }
    path
}

pub use imp::*;

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::{OsStr, OsString};
    use std::fs::File;
    use std::io;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, NO_ERROR, SetLastError};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        GetCompressedFileSizeW, GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetDriveTypeW,
        GetFileInformationByHandle, GetLogicalDrives, GetVolumeInformationW, GetVolumePathNameW,
    };
    use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_REMOTE: u32 = 4;
    const DRIVE_CDROM: u32 = 5;
    const DRIVE_RAMDISK: u32 = 6;
    const INVALID_FILE_SIZE: u32 = u32::MAX;

    fn wide(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn from_wide_buf(buf: &[u16]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        OsString::from_wide(&buf[..len])
            .to_string_lossy()
            .into_owned()
    }

    /// Whether the current process runs with an elevated (admin) token.
    pub fn is_elevated() -> bool {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: GetCurrentProcess returns a pseudo-handle; `token` is a
        // valid out-pointer.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        // SAFETY: `token` was opened above with TOKEN_QUERY; the buffer is a
        // TOKEN_ELEVATION of the size we pass.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            )
        };
        // SAFETY: `token` is a handle we own.
        unsafe { CloseHandle(token) };
        ok != 0 && elevation.TokenIsElevated != 0
    }

    /// The Windows folder, for example `C:\Windows`.
    pub fn windows_dir() -> Option<PathBuf> {
        let mut buf = [0u16; 512];
        // SAFETY: the buffer and its length match.
        let len = unsafe { GetWindowsDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
        (len > 0 && len < buf.len()).then(|| PathBuf::from(OsString::from_wide(&buf[..len])))
    }

    /// Root of the volume containing `path`, for example `C:\`.
    pub fn volume_root(path: &Path) -> Option<PathBuf> {
        let w = wide(path.as_os_str());
        let mut buf = [0u16; 1024];
        // SAFETY: `w` is NUL-terminated; the buffer and its length match.
        let ok = unsafe { GetVolumePathNameW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        (ok != 0).then(|| PathBuf::from(from_wide_buf(&buf)))
    }

    /// File system name (for example `NTFS`) of a volume root.
    pub fn file_system(root: &Path) -> Option<String> {
        volume_information(root).map(|(_, fs)| fs)
    }

    fn volume_information(root: &Path) -> Option<(String, String)> {
        let w = wide(root.as_os_str());
        let mut label = [0u16; 261];
        let mut fs = [0u16; 261];
        // SAFETY: `w` is NUL-terminated; buffers and lengths match; optional
        // out-pointers are null.
        let ok = unsafe {
            GetVolumeInformationW(
                w.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                fs.as_mut_ptr(),
                fs.len() as u32,
            )
        };
        (ok != 0).then(|| (from_wide_buf(&label), from_wide_buf(&fs)))
    }

    pub fn drive_kind(root: &Path) -> DriveKind {
        let w = wide(root.as_os_str());
        // SAFETY: `w` is NUL-terminated.
        match unsafe { GetDriveTypeW(w.as_ptr()) } {
            DRIVE_FIXED => DriveKind::Fixed,
            DRIVE_REMOVABLE => DriveKind::Removable,
            DRIVE_REMOTE => DriveKind::Network,
            DRIVE_CDROM => DriveKind::Optical,
            DRIVE_RAMDISK => DriveKind::RamDisk,
            _ => DriveKind::Unknown,
        }
    }

    pub fn volume_space(path: &Path) -> Option<VolumeSpace> {
        let w = wide(path.as_os_str());
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        // SAFETY: `w` is NUL-terminated; out-pointers are valid u64s.
        let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) };
        (ok != 0).then_some(VolumeSpace { total, free })
    }

    /// Cluster (allocation unit) size of the volume containing `path`.
    pub fn cluster_size(path: &Path) -> Option<u64> {
        let root = volume_root(path)?;
        let w = wide(root.as_os_str());
        let (mut spc, mut bps, mut free, mut total) = (0u32, 0u32, 0u32, 0u32);
        // SAFETY: `w` is NUL-terminated; out-pointers are valid u32s.
        let ok =
            unsafe { GetDiskFreeSpaceW(w.as_ptr(), &mut spc, &mut bps, &mut free, &mut total) };
        (ok != 0 && spc > 0 && bps > 0).then(|| u64::from(spc) * u64::from(bps))
    }

    /// All mounted drive letters.
    pub fn drives() -> Vec<DriveInfo> {
        // SAFETY: no arguments.
        let mask = unsafe { GetLogicalDrives() };
        let system = windows_dir()
            .and_then(|w| w.to_string_lossy().chars().next())
            .map(|c| c.to_ascii_uppercase());
        let mut out = Vec::new();
        for i in 0..26u8 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i) as char;
            let root = format!("{letter}:\\");
            let root_path = Path::new(&root);
            let kind = drive_kind(root_path);
            let (label, file_system) = volume_information(root_path).unwrap_or_default();
            let space = volume_space(root_path);
            // Skip empty card readers and optical drives without media.
            if space.is_none() && matches!(kind, DriveKind::Removable | DriveKind::Optical) {
                continue;
            }
            let space = space.unwrap_or(VolumeSpace { total: 0, free: 0 });
            out.push(DriveInfo {
                root,
                label,
                file_system,
                kind,
                total: space.total,
                free: space.free,
                is_system: system == Some(letter),
            });
        }
        out
    }

    /// Actual allocated size for compressed, sparse, and other special files.
    pub fn compressed_size(path: &Path) -> Option<u64> {
        let w = wide(path.as_os_str());
        let mut high = 0u32;
        // SAFETY: SetLastError has no preconditions. Clearing it lets us tell
        // a real size of 0xFFFFFFFF apart from a failure below.
        unsafe { SetLastError(NO_ERROR) };
        // SAFETY: `w` is NUL-terminated; `high` is a valid out-pointer.
        let low = unsafe { GetCompressedFileSizeW(w.as_ptr(), &mut high) };
        if low == INVALID_FILE_SIZE
            && io::Error::last_os_error().raw_os_error() != Some(NO_ERROR as i32)
        {
            return None;
        }
        Some((u64::from(high) << 32) | u64::from(low))
    }

    /// Link count and a volume-unique file identity, without opening the
    /// file's data (so cloud placeholders are never downloaded).
    pub fn file_link_info(path: &Path) -> Option<(u32, u64, u64)> {
        let file = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .ok()?;
        // SAFETY: an all-zero BY_HANDLE_FILE_INFORMATION is a valid value.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is valid for the lifetime of `file`; `info` is a
        // valid out-pointer.
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) };
        (ok != 0).then(|| {
            (
                info.nNumberOfLinks,
                u64::from(info.dwVolumeSerialNumber),
                (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            )
        })
    }

    /// Opens a volume such as `C:` for raw, read-only access. Requires admin.
    pub fn open_volume(letter: char) -> io::Result<File> {
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(format!(r"\\.\{}:", letter.to_ascii_uppercase()))
    }
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    /// Whether the current process runs as root.
    pub fn is_elevated() -> bool {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }

    pub fn windows_dir() -> Option<PathBuf> {
        None
    }

    pub fn volume_root(_path: &Path) -> Option<PathBuf> {
        Some(PathBuf::from("/"))
    }

    pub fn file_system(_root: &Path) -> Option<String> {
        None
    }

    fn statvfs(path: &Path) -> Option<libc::statvfs> {
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: an all-zero statvfs is a valid value to pass as output.
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c` is NUL-terminated and `st` is a valid out-pointer.
        (unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0).then_some(st)
    }

    // The statvfs field types differ between Unix flavors, so the casts are
    // needed for portability even where they are no-ops.
    #[allow(clippy::unnecessary_cast)]
    pub fn volume_space(path: &Path) -> Option<VolumeSpace> {
        let st = statvfs(path)?;
        let frsize = st.f_frsize as u64;
        Some(VolumeSpace {
            total: st.f_blocks as u64 * frsize,
            free: st.f_bavail as u64 * frsize,
        })
    }

    #[allow(clippy::unnecessary_cast)]
    pub fn cluster_size(path: &Path) -> Option<u64> {
        statvfs(path)
            .map(|st| st.f_frsize as u64)
            .filter(|&s| s > 0)
    }

    /// On non-Windows systems (development and tests) the root file system
    /// stands in for a drive.
    pub fn drives() -> Vec<DriveInfo> {
        let space = volume_space(Path::new("/")).unwrap_or(VolumeSpace { total: 0, free: 0 });
        vec![DriveInfo {
            root: "/".into(),
            label: String::new(),
            file_system: String::new(),
            kind: DriveKind::Fixed,
            total: space.total,
            free: space.free,
            is_system: true,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_strips_verbatim_prefix() {
        assert_eq!(
            simplify_path(PathBuf::from(r"\\?\C:\Users")),
            PathBuf::from(r"C:\Users")
        );
        assert_eq!(
            simplify_path(PathBuf::from(r"\\?\UNC\server\share")),
            PathBuf::from(r"\\server\share")
        );
        assert_eq!(
            simplify_path(PathBuf::from("/home/x")),
            PathBuf::from("/home/x")
        );
    }

    #[test]
    fn root_volume_has_space() {
        let here = std::env::temp_dir();
        let space = volume_space(&here).expect("free space of temp dir");
        assert!(space.total >= space.free);
        assert!(cluster_size(&here).unwrap_or(0) > 0);
        assert!(!drives().is_empty());
    }
}
