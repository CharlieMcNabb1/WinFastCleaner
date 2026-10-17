//! Services set to start automatically (Windows only, read-only).
//!
//! Services that are part of Windows (their program is in the Windows
//! folder) are left out. Nothing here changes a service.

#[cfg(windows)]
pub fn auto_start() -> Result<Vec<crate::startup::ServiceItem>, String> {
    use std::path::Path;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::ERROR_MORE_DATA;
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, ENUM_SERVICE_STATUS_PROCESSW, EnumServicesStatusExW, OpenSCManagerW,
        OpenServiceW, QUERY_SERVICE_CONFIGW, QueryServiceConfig2W, QueryServiceConfigW,
        SC_ENUM_PROCESS_INFO, SC_HANDLE, SC_MANAGER_ENUMERATE_SERVICE, SERVICE_AUTO_START,
        SERVICE_CONFIG_DELAYED_AUTO_START_INFO, SERVICE_DELAYED_AUTO_START_INFO,
        SERVICE_QUERY_CONFIG, SERVICE_RUNNING, SERVICE_STATE_ALL, SERVICE_WIN32,
    };

    use crate::startup::{ServiceItem, program_from_command};

    struct Handle(SC_HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle was opened below and is closed once.
            unsafe { CloseServiceHandle(self.0) };
        }
    }

    fn text(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        // SAFETY: Windows returns NUL-terminated strings inside the buffers
        // that are still alive where this is called.
        let len = (0..).take_while(|&i| unsafe { *p.add(i) } != 0).count();
        // SAFETY: `len` units were just read above.
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) })
    }

    // SAFETY: null machine and database names mean the local defaults.
    let scm = unsafe { OpenSCManagerW(null(), null(), SC_MANAGER_ENUMERATE_SERVICE) };
    if scm.is_null() {
        return Err("The list of services could not be read.".into());
    }
    let scm = Handle(scm);

    // List every Win32 service (two calls: size, then data).
    let mut buf: Vec<u64> = Vec::new();
    let mut needed = 0u32;
    let mut count = 0u32;
    for _ in 0..4 {
        let mut resume = 0u32;
        // SAFETY: the buffer and its byte length match (u64 keeps it
        // aligned for the structures Windows writes).
        let ok = unsafe {
            EnumServicesStatusExW(
                scm.0,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                buf.as_mut_ptr().cast(),
                (buf.len() * 8) as u32,
                &mut needed,
                &mut count,
                &mut resume,
                null(),
            )
        };
        if ok != 0 {
            break;
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(ERROR_MORE_DATA as i32) {
            return Err("The list of services could not be read.".into());
        }
        buf = vec![0u64; (needed as usize).div_ceil(8) + 512];
        count = 0;
    }
    // SAFETY: Windows wrote `count` structures at the start of `buf`.
    let entries = unsafe {
        std::slice::from_raw_parts(
            buf.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(),
            count as usize,
        )
    };

    let windir = crate::platform::windows_dir();
    let mut out = Vec::new();
    for e in entries {
        let name = text(e.lpServiceName);
        let display_name = text(e.lpDisplayName);
        let wname: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: `wname` is a NUL-terminated name from the list above.
        let svc = unsafe { OpenServiceW(scm.0, wname.as_ptr(), SERVICE_QUERY_CONFIG) };
        if svc.is_null() {
            continue;
        }
        let svc = Handle(svc);
        let mut need = 0u32;
        // SAFETY: asks for the size only.
        unsafe { QueryServiceConfigW(svc.0, null_mut(), 0, &mut need) };
        if need == 0 || need > 64 * 1024 {
            continue;
        }
        let mut cfg = vec![0u64; (need as usize).div_ceil(8)];
        // SAFETY: the buffer is at least `need` bytes and aligned.
        if unsafe { QueryServiceConfigW(svc.0, cfg.as_mut_ptr().cast(), need, &mut need) } == 0 {
            continue;
        }
        // SAFETY: Windows filled a QUERY_SERVICE_CONFIGW at the start.
        let c = unsafe { &*cfg.as_ptr().cast::<QUERY_SERVICE_CONFIGW>() };
        if c.dwStartType != SERVICE_AUTO_START {
            continue;
        }
        let command = text(c.lpBinaryPathName);
        let program = program_from_command(&command).map(|p| system_root(&p, windir.as_deref()));
        let in_windows = match (&program, &windir) {
            (Some(p), Some(w)) => crate::safety::Guard::is_within(Path::new(p), w),
            _ => true,
        };
        if in_windows {
            continue;
        }
        let mut delayed = SERVICE_DELAYED_AUTO_START_INFO {
            fDelayedAutostart: 0,
        };
        let mut n = 0u32;
        // SAFETY: the out-buffer is one SERVICE_DELAYED_AUTO_START_INFO.
        unsafe {
            QueryServiceConfig2W(
                svc.0,
                SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                (&mut delayed as *mut SERVICE_DELAYED_AUTO_START_INFO).cast(),
                std::mem::size_of::<SERVICE_DELAYED_AUTO_START_INFO>() as u32,
                &mut n,
            )
        };
        out.push(ServiceItem {
            name,
            display_name,
            publisher: program
                .as_deref()
                .and_then(|p| crate::startup::publisher(Path::new(p))),
            program,
            running: e.ServiceStatusProcess.dwCurrentState == SERVICE_RUNNING,
            delayed: delayed.fDelayedAutostart != 0,
        });
    }
    out.sort_by_key(|s| s.display_name.to_lowercase());
    Ok(out)
}

/// Service paths sometimes start with `\SystemRoot\` or `System32\`.
#[cfg(windows)]
fn system_root(p: &str, windir: Option<&std::path::Path>) -> String {
    let Some(w) = windir else {
        return p.to_string();
    };
    let lower = p.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix(r"\systemroot\") {
        return w.join(&p[p.len() - rest.len()..]).display().to_string();
    }
    if lower.starts_with(r"system32\") || lower.starts_with(r"syswow64\") {
        return w.join(p).display().to_string();
    }
    p.to_string()
}

#[cfg(not(windows))]
pub fn auto_start() -> Result<Vec<crate::startup::ServiceItem>, String> {
    Ok(Vec::new())
}
