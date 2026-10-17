//! Minimal registry access (Windows only), for the startup manager and the
//! tray monitor's sign-in entry.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_BINARY, REG_EXPAND_SZ,
    REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegEnumValueW,
    RegOpenKeyExW, RegQueryInfoKeyW, RegQueryValueExW, RegSetValueExW,
};

pub use windows_sys::Win32::System::Registry::{
    KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
};

/// A registry root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

impl Hive {
    fn handle(self) -> HKEY {
        match self {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Hive::CurrentUser => "HKEY_CURRENT_USER",
            Hive::LocalMachine => "HKEY_LOCAL_MACHINE",
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

/// A registry value: its name, type, and raw data.
#[derive(Clone, Debug)]
pub struct Value {
    pub name: String,
    pub kind: u32,
    pub data: Vec<u8>,
}

impl Value {
    /// The text of a `REG_SZ` or `REG_EXPAND_SZ` value (up to the first NUL),
    /// and whether it needs expanding.
    pub fn text(&self) -> Option<(String, bool)> {
        if self.kind != REG_SZ && self.kind != REG_EXPAND_SZ {
            return None;
        }
        let units: Vec<u16> = self
            .data
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0)
            .collect();
        Some((String::from_utf16_lossy(&units), self.kind == REG_EXPAND_SZ))
    }
}

/// An open key, closed when dropped.
#[derive(Debug)]
pub struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key was opened by `open` or `create` and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

impl Key {
    /// Opens an existing key. `Ok(None)` when it doesn't exist; `Err` holds
    /// the Windows error code.
    pub fn open(hive: Hive, path: &str, access: u32) -> Result<Option<Key>, u32> {
        let w = wide(path);
        let mut key: HKEY = null_mut();
        // SAFETY: `w` is NUL-terminated and `key` is a valid out-pointer.
        let err = unsafe { RegOpenKeyExW(hive.handle(), w.as_ptr(), 0, access, &mut key) };
        match err {
            ERROR_SUCCESS => Ok(Some(Key(key))),
            ERROR_FILE_NOT_FOUND => Ok(None),
            e => Err(e),
        }
    }

    /// Opens a key, creating it if needed.
    pub fn create(hive: Hive, path: &str, access: u32) -> Result<Key, u32> {
        let w = wide(path);
        let mut key: HKEY = null_mut();
        // SAFETY: `w` is NUL-terminated, the class and security attributes
        // are null (defaults), and `key` is a valid out-pointer.
        let err = unsafe {
            RegCreateKeyExW(
                hive.handle(),
                w.as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                access,
                null(),
                &mut key,
                null_mut(),
            )
        };
        if err == ERROR_SUCCESS {
            Ok(Key(key))
        } else {
            Err(err)
        }
    }

    /// Every value of the key, with its data.
    pub fn values(&self) -> Vec<Value> {
        let (mut max_name, mut max_data) = (0u32, 0u32);
        // SAFETY: only the two requested out-pointers are non-null.
        let err = unsafe {
            RegQueryInfoKeyW(
                self.0,
                null_mut(),
                null_mut(),
                null(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut max_name,
                &mut max_data,
                null_mut(),
                null_mut(),
            )
        };
        if err != ERROR_SUCCESS {
            return Vec::new();
        }
        let mut out = Vec::new();
        for i in 0..10_000 {
            let mut name = vec![0u16; max_name as usize + 1];
            let mut name_len = name.len() as u32;
            let mut data = vec![0u8; max_data as usize + 2];
            let mut data_len = data.len() as u32;
            let mut kind = 0u32;
            // SAFETY: the buffers and their lengths match.
            let err = unsafe {
                RegEnumValueW(
                    self.0,
                    i,
                    name.as_mut_ptr(),
                    &mut name_len,
                    null(),
                    &mut kind,
                    data.as_mut_ptr(),
                    &mut data_len,
                )
            };
            if err != ERROR_SUCCESS {
                break;
            }
            name.truncate(name_len as usize);
            data.truncate(data_len as usize);
            out.push(Value {
                name: String::from_utf16_lossy(&name),
                kind,
                data,
            });
        }
        out
    }

    /// One value's data, if it exists.
    pub fn get(&self, name: &str) -> Option<Value> {
        let w = wide(name);
        let (mut kind, mut len) = (0u32, 0u32);
        // SAFETY: asks for the size only (null data pointer).
        let err = unsafe {
            RegQueryValueExW(self.0, w.as_ptr(), null(), &mut kind, null_mut(), &mut len)
        };
        if err != ERROR_SUCCESS {
            return None;
        }
        let mut data = vec![0u8; len as usize];
        // SAFETY: the buffer is `len` bytes long.
        let err = unsafe {
            RegQueryValueExW(
                self.0,
                w.as_ptr(),
                null(),
                &mut kind,
                data.as_mut_ptr(),
                &mut len,
            )
        };
        if err != ERROR_SUCCESS {
            return None;
        }
        data.truncate(len as usize);
        Some(Value {
            name: name.to_string(),
            kind,
            data,
        })
    }

    /// Writes a `REG_SZ` value.
    pub fn set_string(&self, name: &str, value: &str) -> Result<(), u32> {
        let w = wide(name);
        let d = wide(value);
        // SAFETY: both strings are NUL-terminated; the data length includes
        // the terminator, as the registry expects.
        let err = unsafe {
            RegSetValueExW(
                self.0,
                w.as_ptr(),
                0,
                REG_SZ,
                d.as_ptr().cast(),
                (d.len() * 2) as u32,
            )
        };
        if err == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(err)
        }
    }

    /// Deletes a value. A missing value is not an error.
    pub fn delete_value(&self, name: &str) -> Result<(), u32> {
        let w = wide(name);
        // SAFETY: `w` is NUL-terminated.
        let err = unsafe { RegDeleteValueW(self.0, w.as_ptr()) };
        match err {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            e => Err(e),
        }
    }

    /// Writes a `REG_BINARY` value.
    pub fn set_binary(&self, name: &str, data: &[u8]) -> Result<(), u32> {
        let w = wide(name);
        // SAFETY: `w` is NUL-terminated and `data` is passed with its length.
        let err = unsafe {
            RegSetValueExW(
                self.0,
                w.as_ptr(),
                0,
                REG_BINARY,
                data.as_ptr(),
                data.len() as u32,
            )
        };
        if err == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(err)
        }
    }
}

#[cfg(test)]
pub mod test_support {
    //! Writes throwaway values for tests (only ever under a test key).
    use super::*;

    pub fn set_string(key: &Key, name: &str, value: &str, expand: bool) {
        if !expand {
            key.set_string(name, value).unwrap();
            return;
        }
        let d = wide(value);
        // SAFETY: `d` is a NUL-terminated UTF-16 string of the size passed.
        let err = unsafe {
            RegSetValueExW(
                key.0,
                wide(name).as_ptr(),
                0,
                if expand { REG_EXPAND_SZ } else { REG_SZ },
                d.as_ptr().cast(),
                (d.len() * 2) as u32,
            )
        };
        assert_eq!(err, ERROR_SUCCESS);
    }

    /// Deletes a test key and everything under it.
    pub fn delete_tree(path: &str) {
        assert!(path.contains("WinFastCleaner-test-"), "not a test key");
        // SAFETY: `path` is NUL-terminated; only test keys get here.
        unsafe {
            windows_sys::Win32::System::Registry::RegDeleteTreeW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
            )
        };
    }
}
