//! Link-safe file operations.
//!
//! Every delete, move into the recovery bin, and restore works like this:
//!
//! 1. Open the item **without following links** (a symlink or junction is
//!    opened as itself).
//! 2. Ask the open handle for its final path and attributes, and check them
//!    (the caller runs the safety guard on that final path).
//! 3. Delete or rename **through the same handle**.
//!
//! Because the check and the action use the same handle, a folder that is
//! swapped for a junction between "check" and "delete" cannot redirect the
//! operation somewhere else.

use std::io;
use std::path::{Path, PathBuf};

/// Facts about an opened item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemInfo {
    pub is_dir: bool,
    /// Symlink, junction, mount point, or any other reparse point (cloud
    /// files, deduplicated files). These are never deleted or moved.
    pub is_special: bool,
    /// Windows "system" attribute.
    pub is_system: bool,
    /// Cloud placeholder whose contents are not on this disk.
    pub is_cloud: bool,
    pub size: u64,
    /// Unix seconds.
    pub modified: i64,
    pub links: u32,
}

/// A file or folder opened for deletion or renaming, without following
/// links.
#[derive(Debug)]
pub struct Opened {
    inner: imp::Handle,
}

/// Opens an item for deletion or renaming without following links.
pub fn open(path: &Path) -> io::Result<Opened> {
    Ok(Opened {
        inner: imp::Handle::open(path)?,
    })
}

/// Like [`open`], but also allows overwriting the file's contents
/// ([`Opened::wipe`]) for secure delete.
pub fn open_for_wipe(path: &Path) -> io::Result<Opened> {
    Ok(Opened {
        inner: imp::Handle::open_writable(path)?,
    })
}

impl Opened {
    pub fn info(&self) -> io::Result<ItemInfo> {
        self.inner.info()
    }

    /// The item's real location, resolved by the file system.
    pub fn final_path(&self) -> io::Result<PathBuf> {
        self.inner.final_path()
    }

    /// Overwrites the whole file with zeros and flushes it to disk, through
    /// the same handle (only for items opened with [`open_for_wipe`]).
    ///
    /// On SSDs, wear leveling may keep old copies of the data elsewhere, so
    /// this is best effort there.
    pub fn wipe(&self) -> io::Result<()> {
        self.inner.wipe()
    }

    /// Deletes the item (a file, or an empty folder).
    pub fn delete(self) -> io::Result<()> {
        self.inner.delete()
    }

    /// Renames the item to `target` on the same drive. Fails if `target`
    /// already exists.
    pub fn rename_no_replace(&self, target: &Path) -> io::Result<()> {
        self.inner.rename_no_replace(target)
    }
}

/// Which file an open handle refers to (volume and file number), so two
/// names of one hard-linked file can be told apart from two copies.
pub type FileId = (u64, u128);

/// Opens a regular file for reading without following links. Links,
/// folders, and cloud placeholders (whose contents would be downloaded by
/// reading them) are refused.
pub fn open_read(path: &Path) -> io::Result<(std::fs::File, FileId)> {
    read_imp::open(path)
}

#[cfg(windows)]
mod read_imp {
    use super::FileId;
    use std::fs::File;
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_OFFLINE,
        FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_SEQUENTIAL_SCAN,
        GetFileInformationByHandle,
    };

    pub fn open(path: &Path) -> io::Result<(File, FileId)> {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_SEQUENTIAL_SCAN)
            .open(path)?;
        // SAFETY: a plain C struct for which all zeros is a valid value.
        let mut bh: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is open and `bh` is a valid out-pointer.
        if unsafe { GetFileInformationByHandle(f.as_raw_handle() as HANDLE, &mut bh) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let a = bh.dwFileAttributes;
        if a & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
            return Err(io::Error::other("not a regular file"));
        }
        if a & (FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
            | FILE_ATTRIBUTE_RECALL_ON_OPEN)
            != 0
        {
            return Err(io::Error::other("cloud file (contents not on this PC)"));
        }
        let index = (u128::from(bh.nFileIndexHigh) << 32) | u128::from(bh.nFileIndexLow);
        Ok((f, (u64::from(bh.dwVolumeSerialNumber), index)))
    }
}

#[cfg(unix)]
mod read_imp {
    use super::FileId;
    use std::fs::File;
    use std::io;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Path;

    pub fn open(path: &Path) -> io::Result<(File, FileId)> {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let md = f.metadata()?;
        if !md.is_file() {
            return Err(io::Error::other("not a regular file"));
        }
        Ok((f, (md.dev(), u128::from(md.ino()))))
    }
}

/// Overwriting a file with other names (hard links) would also erase the
/// data under those names, which may be outside anything being cleaned.
const HARD_LINKED: &str = "the file has other names (hard links), so it is not overwritten";

/// Writes `len` zero bytes from the start of `file` and flushes them.
fn overwrite(file: &std::fs::File, len: u64) -> io::Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    let mut f = file;
    f.seek(SeekFrom::Start(0))?;
    let zeros = vec![0u8; 1024 * 1024];
    let mut left = len;
    while left > 0 {
        let n = left.min(zeros.len() as u64) as usize;
        f.write_all(&zeros[..n])?;
        left -= n as u64;
    }
    f.flush()?;
    file.sync_all()
}

/// Short reason for an I/O error, for "skipped" lists.
pub fn describe_error(e: &io::Error) -> String {
    #[cfg(windows)]
    {
        match e.raw_os_error() {
            Some(32) | Some(33) => return "In use by another program".into(),
            Some(5) => return "Access denied".into(),
            Some(2) | Some(3) => return "Already gone".into(),
            Some(145) => return "Folder is not empty".into(),
            _ => {}
        }
    }
    match e.kind() {
        io::ErrorKind::NotFound => "Already gone".into(),
        io::ErrorKind::PermissionDenied => "Access denied".into(),
        io::ErrorKind::AlreadyExists => "Something with that name already exists".into(),
        io::ErrorKind::DirectoryNotEmpty => "Folder is not empty".into(),
        _ => e.to_string(),
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::fs::File;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_OFFLINE,
        FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM, FILE_ATTRIBUTE_TAG_INFO,
        FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
        FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED,
        FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FileAttributeTagInfo, FileDispositionInfo, FileDispositionInfoEx,
        FileRenameInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
        GetFinalPathNameByHandleW, SYNCHRONIZE, SetFileInformationByHandle, VOLUME_NAME_DOS,
    };

    #[derive(Debug)]
    pub struct Handle {
        file: File,
    }

    impl Handle {
        fn raw(&self) -> HANDLE {
            self.file.as_raw_handle() as HANDLE
        }

        pub fn open(path: &Path) -> io::Result<Handle> {
            let file = std::fs::OpenOptions::new()
                .access_mode(DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                .open(path)?;
            Ok(Handle { file })
        }

        pub fn open_writable(path: &Path) -> io::Result<Handle> {
            const GENERIC_WRITE: u32 = 0x4000_0000;
            let file = std::fs::OpenOptions::new()
                .access_mode(GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
                // Nobody else may write while the file is overwritten.
                .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            Ok(Handle { file })
        }

        pub fn wipe(&self) -> io::Result<()> {
            let info = self.info()?;
            if info.is_dir || info.is_special || info.is_cloud {
                return Err(io::Error::other("only regular files can be overwritten"));
            }
            if info.links > 1 {
                return Err(io::Error::other(super::HARD_LINKED));
            }
            super::overwrite(&self.file, info.size)
        }

        pub fn info(&self) -> io::Result<ItemInfo> {
            // SAFETY: all-zero is a valid value for this plain C struct.
            let mut bh: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            // SAFETY: the handle is valid while `self` lives; `bh` is a valid
            // out-pointer.
            if unsafe { GetFileInformationByHandle(self.raw(), &mut bh) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut tag = FILE_ATTRIBUTE_TAG_INFO {
                FileAttributes: 0,
                ReparseTag: 0,
            };
            // SAFETY: the buffer is a FILE_ATTRIBUTE_TAG_INFO of the size we
            // pass, matching the requested class.
            let ok = unsafe {
                GetFileInformationByHandleEx(
                    self.raw(),
                    FileAttributeTagInfo,
                    (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                    size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            let a = bh.dwFileAttributes;
            let ft = (u64::from(bh.ftLastWriteTime.dwHighDateTime) << 32)
                | u64::from(bh.ftLastWriteTime.dwLowDateTime);
            Ok(ItemInfo {
                is_dir: a & FILE_ATTRIBUTE_DIRECTORY != 0,
                is_special: a & FILE_ATTRIBUTE_REPARSE_POINT != 0 || tag.ReparseTag != 0,
                is_system: a & FILE_ATTRIBUTE_SYSTEM != 0,
                is_cloud: a
                    & (FILE_ATTRIBUTE_OFFLINE
                        | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
                        | FILE_ATTRIBUTE_RECALL_ON_OPEN)
                    != 0,
                size: (u64::from(bh.nFileSizeHigh) << 32) | u64::from(bh.nFileSizeLow),
                modified: if ft == 0 {
                    0
                } else {
                    crate::format::filetime_to_unix(ft)
                },
                links: bh.nNumberOfLinks,
            })
        }

        pub fn final_path(&self) -> io::Result<PathBuf> {
            let mut buf = vec![0u16; 512];
            loop {
                // SAFETY: the buffer and its length match.
                let n = unsafe {
                    GetFinalPathNameByHandleW(
                        self.raw(),
                        buf.as_mut_ptr(),
                        buf.len() as u32,
                        FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
                    )
                } as usize;
                if n == 0 {
                    return Err(io::Error::last_os_error());
                }
                if n < buf.len() {
                    buf.truncate(n);
                    return Ok(PathBuf::from(std::ffi::OsString::from_wide(&buf)));
                }
                buf.resize(n + 1, 0);
            }
        }

        pub fn delete(self) -> io::Result<()> {
            let set_ex = |flags: u32| {
                let info = FILE_DISPOSITION_INFO_EX { Flags: flags };
                // SAFETY: `info` is a FILE_DISPOSITION_INFO_EX of the size we
                // pass, matching the class.
                unsafe {
                    SetFileInformationByHandle(
                        self.raw(),
                        FileDispositionInfoEx,
                        (&info as *const FILE_DISPOSITION_INFO_EX).cast(),
                        size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
                    )
                }
            };
            let flags =
                FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE;
            if set_ex(flags | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS) != 0 || set_ex(flags) != 0 {
                return Ok(());
            }
            // Older file systems (FAT) only support the basic form.
            let info = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: `info` is a FILE_DISPOSITION_INFO of the size we pass.
            let ok = unsafe {
                SetFileInformationByHandle(
                    self.raw(),
                    FileDispositionInfo,
                    (&info as *const FILE_DISPOSITION_INFO).cast(),
                    size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
            // The file is removed when the handle closes (end of scope).
        }

        pub fn rename_no_replace(&self, target: &Path) -> io::Result<()> {
            let name: Vec<u16> = target.as_os_str().encode_wide().collect();
            let name_bytes = name.len() * 2;
            let total = size_of::<FILE_RENAME_INFO>() + name_bytes;
            // A u64 buffer keeps the struct correctly aligned.
            let mut buf = vec![0u64; total.div_ceil(8)];
            let info = buf.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            // SAFETY: `buf` is large enough and suitably aligned for a
            // FILE_RENAME_INFO followed by `name.len()` UTF-16 units; the
            // writes stay inside it.
            unsafe {
                (*info).Anonymous.ReplaceIfExists = false;
                (*info).RootDirectory = std::ptr::null_mut();
                (*info).FileNameLength = name_bytes as u32;
                let dst = std::ptr::addr_of_mut!((*info).FileName).cast::<u16>();
                std::ptr::copy_nonoverlapping(name.as_ptr(), dst, name.len());
            }
            // SAFETY: `buf` holds a complete FILE_RENAME_INFO of `total` bytes.
            let ok = unsafe {
                SetFileInformationByHandle(
                    self.raw(),
                    FileRenameInfo,
                    buf.as_ptr().cast(),
                    total as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    /// Unix stand-in used by development machines and tests. It checks
    /// with `lstat` right before acting; it is not race-free like the
    /// Windows version, which is the one that ships.
    #[derive(Debug)]
    pub struct Handle {
        path: PathBuf,
        dev: u64,
        ino: u64,
    }

    impl Handle {
        pub fn open(path: &Path) -> io::Result<Handle> {
            let md = std::fs::symlink_metadata(path)?;
            Ok(Handle {
                path: path.to_path_buf(),
                dev: md.dev(),
                ino: md.ino(),
            })
        }

        pub fn open_writable(path: &Path) -> io::Result<Handle> {
            Handle::open(path)
        }

        pub fn wipe(&self) -> io::Result<()> {
            use std::os::unix::fs::OpenOptionsExt;
            let f = std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&self.path)?;
            let md = f.metadata()?;
            if md.dev() != self.dev || md.ino() != self.ino || !md.is_file() {
                return Err(io::Error::other("item changed while being processed"));
            }
            if md.nlink() > 1 {
                return Err(io::Error::other(super::HARD_LINKED));
            }
            super::overwrite(&f, md.len())
        }

        fn current(&self) -> io::Result<std::fs::Metadata> {
            let md = std::fs::symlink_metadata(&self.path)?;
            if md.dev() != self.dev || md.ino() != self.ino {
                return Err(io::Error::other("item changed while being processed"));
            }
            Ok(md)
        }

        pub fn info(&self) -> io::Result<ItemInfo> {
            let md = self.current()?;
            Ok(ItemInfo {
                is_dir: md.is_dir(),
                is_special: md.file_type().is_symlink() || !(md.is_file() || md.is_dir()),
                is_system: false,
                is_cloud: false,
                size: md.len(),
                modified: md.mtime(),
                links: md.nlink() as u32,
            })
        }

        pub fn final_path(&self) -> io::Result<PathBuf> {
            let name = self
                .path
                .file_name()
                .ok_or_else(|| io::Error::other("path has no file name"))?;
            let parent = self.path.parent().unwrap_or(Path::new("/"));
            Ok(std::fs::canonicalize(parent)?.join(name))
        }

        pub fn delete(self) -> io::Result<()> {
            let md = self.current()?;
            if md.is_dir() {
                std::fs::remove_dir(&self.path)
            } else {
                std::fs::remove_file(&self.path)
            }
        }

        pub fn rename_no_replace(&self, target: &Path) -> io::Result<()> {
            let md = self.current()?;
            if md.is_dir() {
                if target.symlink_metadata().is_ok() {
                    return Err(io::ErrorKind::AlreadyExists.into());
                }
                return std::fs::rename(&self.path, target);
            }
            // A hard link fails if the target exists, so nothing is replaced.
            std::fs::hard_link(&self.path, target)?;
            std::fs::remove_file(&self.path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_and_rename_regular_files() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.txt");
        std::fs::write(&a, b"hello").unwrap();
        let h = open(&a).unwrap();
        let info = h.info().unwrap();
        assert!(!info.is_dir && !info.is_special && info.size == 5);
        assert!(Guard::same_path(
            &h.final_path().unwrap(),
            &std::fs::canonicalize(&a).unwrap()
        ));

        let b = tmp.path().join("b.txt");
        h.rename_no_replace(&b).unwrap();
        drop(h);
        assert!(!a.exists() && b.exists());

        // Never replaces an existing file.
        std::fs::write(&a, b"other").unwrap();
        let hb = open(&b).unwrap();
        assert!(hb.rename_no_replace(&a).is_err());
        drop(hb);
        assert_eq!(std::fs::read(&a).unwrap(), b"other");

        open(&b).unwrap().delete().unwrap();
        assert!(!b.exists());
    }

    #[test]
    fn wipe_overwrites_in_place_then_delete_removes() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("secret.txt");
        let body = vec![0xA5u8; 3 * 1024 * 1024 + 17];
        std::fs::write(&a, &body).unwrap();
        let h = open_for_wipe(&a).unwrap();
        h.wipe().unwrap();
        drop(h);
        let after = std::fs::read(&a).unwrap();
        assert_eq!(after.len(), body.len(), "same length");
        assert!(after.iter().all(|b| *b == 0), "contents overwritten");
        let h = open_for_wipe(&a).unwrap();
        h.wipe().unwrap();
        h.delete().unwrap();
        assert!(!a.exists());
        assert!(
            open_for_wipe(tmp.path()).and_then(|h| h.wipe()).is_err(),
            "folders are refused"
        );
    }

    #[test]
    fn wipe_refuses_hard_linked_files() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = tmp.path().join("document.txt");
        std::fs::write(&doc, b"keep me").unwrap();
        let link = tmp.path().join("temp-link.tmp");
        std::fs::hard_link(&doc, &link).unwrap();
        assert!(open_for_wipe(&link).unwrap().wipe().is_err());
        assert_eq!(std::fs::read(&doc).unwrap(), b"keep me");
    }

    #[test]
    fn open_read_identifies_files_and_refuses_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.bin");
        let b = tmp.path().join("b.bin");
        std::fs::write(&a, b"same").unwrap();
        std::fs::write(&b, b"same").unwrap();
        let (_, id_a) = open_read(&a).unwrap();
        let (_, id_b) = open_read(&b).unwrap();
        assert_ne!(id_a, id_b, "two copies are two files");
        let link = tmp.path().join("hard.bin");
        std::fs::hard_link(&a, &link).unwrap();
        assert_eq!(
            open_read(&link).unwrap().1,
            id_a,
            "a hard link is the same file"
        );
        assert!(open_read(tmp.path()).is_err(), "folders are refused");
        #[cfg(unix)]
        {
            let sym = tmp.path().join("sym.bin");
            std::os::unix::fs::symlink(&a, &sym).unwrap();
            assert!(open_read(&sym).is_err(), "symlinks are not followed");
        }
    }

    #[cfg(unix)]
    #[test]
    fn links_are_opened_as_themselves() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target.txt");
        std::fs::write(&target, b"keep").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let h = open(&link).unwrap();
        assert!(h.info().unwrap().is_special);
    }

    #[cfg(windows)]
    #[test]
    fn junctions_are_opened_as_themselves() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep.txt"), b"keep").unwrap();
        let link = tmp.path().join("junction");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success());
        let h = open(&link).unwrap();
        let info = h.info().unwrap();
        assert!(info.is_special);
        // The handle names the junction itself, not its target.
        assert!(Guard::same_path(
            &h.final_path().unwrap(),
            &std::fs::canonicalize(tmp.path()).unwrap().join("junction")
        ));
    }

    #[cfg(windows)]
    #[test]
    fn locked_files_report_in_use() {
        use std::os::windows::fs::OpenOptionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("locked.txt");
        std::fs::write(&f, b"x").unwrap();
        let _lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&f)
            .unwrap();
        let err = open(&f).unwrap_err();
        assert_eq!(describe_error(&err), "In use by another program");
    }

    use crate::safety::Guard;
}
