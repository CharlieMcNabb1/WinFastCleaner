//! Small COM helpers (Windows only): Task Scheduler, WMI, and shell links.
//!
//! Each call runs on its own short-lived thread with COM initialized for
//! that thread, so it never interferes with COM set up elsewhere in the
//! process (the UI thread, for example).

use std::collections::HashMap;
use std::path::Path;

use windows::Win32::Foundation::{E_ACCESSDENIED, VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoSetProxyBlanket, CoTaskMemFree, CoUninitialize, EOAC_NONE, IPersistFile,
    RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, STGM_READ,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, ITaskFolder, ITaskService, TASK_ACTION_EXEC, TASK_ACTION_TYPE,
    TASK_CREATE_OR_UPDATE, TASK_ENUM_HIDDEN, TASK_LOGON_INTERACTIVE_TOKEN, TASK_TRIGGER_LOGON,
    TASK_TRIGGER_TYPE2, TaskScheduler,
};
use windows::Win32::System::Variant::{VARIANT, VT_I4, VariantClear, VariantToStringAlloc};
use windows::Win32::System::Wmi::{
    IWbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE,
    WBEM_INFINITE, WbemLocator,
};
use windows::Win32::UI::Shell::{IShellLinkW, SLGP_RAWPATH, ShellLink};
use windows::core::{BSTR, HSTRING, Interface, PCWSTR};

/// Runs `f` on a new thread with COM initialized (multithreaded).
fn with_com<T: Send>(f: impl FnOnce() -> Result<T, String> + Send) -> Result<T, String> {
    std::thread::scope(|s| {
        s.spawn(|| {
            // SAFETY: initializes COM for this new thread only; balanced by
            // CoUninitialize below when it succeeded.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_err() {
                return Err(format!("COM could not start ({hr:?})"));
            }
            let r = f();
            // SAFETY: matches the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
            r
        })
        .join()
        .unwrap_or_else(|_| Err("internal error".into()))
    })
}

fn err(e: windows::core::Error) -> String {
    e.message().trim().to_string()
}

fn variant_i32(v: i32) -> VARIANT {
    let mut var = VARIANT::default();
    // SAFETY: writes a VT_I4 value into a freshly zeroed VARIANT.
    unsafe {
        (*var.Anonymous.Anonymous).vt = VT_I4;
        (*var.Anonymous.Anonymous).Anonymous.lVal = v;
    }
    var
}

/// The value of a VARIANT as text, if it has one.
fn variant_text(v: &VARIANT) -> Option<String> {
    // SAFETY: `v` is a valid VARIANT; the returned buffer is freed below.
    let p = unsafe { VariantToStringAlloc(v) }.ok()?;
    // SAFETY: `p` is a NUL-terminated string allocated by COM.
    let s = unsafe { p.to_string() }.ok();
    // SAFETY: frees the buffer allocated above, once.
    unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
    s.filter(|s| !s.is_empty())
}

// ------------------------------------------------------------ tasks

/// A scheduled task that runs at sign-in.
#[derive(Clone, Debug)]
pub struct LogonTask {
    /// Full task path, such as `\Vendor\Updater`.
    pub path: String,
    pub name: String,
    pub enabled: bool,
    pub command: Option<String>,
    pub arguments: Option<String>,
    pub author: Option<String>,
}

fn connect() -> Result<ITaskService, String> {
    // SAFETY: COM is initialized on this thread (see `with_com`).
    let service: ITaskService =
        unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }.map_err(err)?;
    let none = VARIANT::default();
    // SAFETY: connects to the local Task Scheduler as the current user.
    unsafe { service.Connect(&none, &none, &none, &none) }.map_err(err)?;
    Ok(service)
}

/// Tasks with a logon trigger, except Windows' own (under `\Microsoft`),
/// which are never listed so they can't be switched off by accident.
pub fn logon_tasks() -> Result<Vec<LogonTask>, String> {
    with_com(|| {
        let service = connect()?;
        // SAFETY: valid service; "\" is the root folder.
        let root = unsafe { service.GetFolder(&BSTR::from("\\")) }.map_err(err)?;
        let mut out = Vec::new();
        walk_folder(&root, &mut out, 0);
        Ok(out)
    })
}

fn walk_folder(folder: &ITaskFolder, out: &mut Vec<LogonTask>, depth: u32) {
    if depth > 16 {
        return;
    }
    // SAFETY: `folder` and the objects it returns are valid
    // COM interfaces on this thread; failures skip the item.
    unsafe {
        let Ok(path) = folder.Path() else { return };
        let path = path.to_string();
        let lower = path.to_ascii_lowercase();
        if lower == "\\microsoft" || lower.starts_with("\\microsoft\\") {
            return;
        }
        if let Ok(tasks) = folder.GetTasks(TASK_ENUM_HIDDEN.0) {
            let count = tasks.Count().unwrap_or(0);
            for i in 1..=count {
                if let Ok(t) = tasks.get_Item(&variant_i32(i))
                    && let Some(task) = read_task(&t)
                {
                    out.push(task);
                }
            }
        }
        if let Ok(subs) = folder.GetFolders(0) {
            let count = subs.Count().unwrap_or(0);
            for i in 1..=count {
                if let Ok(f) = subs.get_Item(&variant_i32(i)) {
                    walk_folder(&f, out, depth + 1);
                }
            }
        }
    }
}

/// # Safety
/// `t` must be a valid interface on a COM-initialized thread.
unsafe fn read_task(
    t: &windows::Win32::System::TaskScheduler::IRegisteredTask,
) -> Option<LogonTask> {
    // SAFETY: see the function's contract.
    unsafe {
        let def = t.Definition().ok()?;
        let triggers = def.Triggers().ok()?;
        let mut n = 0;
        triggers.Count(&mut n).ok()?;
        let mut logon = false;
        for i in 1..=n {
            if let Ok(trig) = triggers.get_Item(i) {
                let mut ty = TASK_TRIGGER_TYPE2::default();
                if trig.Type(&mut ty).is_ok() && ty == TASK_TRIGGER_LOGON {
                    logon = true;
                }
            }
        }
        if !logon {
            return None;
        }
        let (mut command, mut arguments) = (None, None);
        if let Ok(actions) = def.Actions() {
            let mut n = 0;
            let _ = actions.Count(&mut n);
            for i in 1..=n {
                let Ok(a) = actions.get_Item(i) else { continue };
                let mut ty = TASK_ACTION_TYPE::default();
                if a.Type(&mut ty).is_ok() && ty == TASK_ACTION_EXEC {
                    if let Ok(exec) = a.cast::<IExecAction>() {
                        let mut p = BSTR::new();
                        let mut args = BSTR::new();
                        let _ = exec.Path(&mut p);
                        let _ = exec.Arguments(&mut args);
                        command = Some(p.to_string()).filter(|s| !s.is_empty());
                        arguments = Some(args.to_string()).filter(|s| !s.is_empty());
                    }
                    break;
                }
            }
        }
        let mut author = BSTR::new();
        if let Ok(info) = def.RegistrationInfo() {
            let _ = info.Author(&mut author);
        }
        Some(LogonTask {
            path: t.Path().ok()?.to_string(),
            name: t.Name().ok()?.to_string(),
            enabled: t.Enabled().ok()?.as_bool(),
            command,
            arguments,
            author: Some(author.to_string()).filter(|s| !s.is_empty()),
        })
    }
}

fn split_task_path(path: &str) -> Result<(String, String), String> {
    let i = path.rfind('\\').ok_or("Not a task path.")?;
    let folder = if i == 0 { "\\" } else { &path[..i] };
    let name = &path[i + 1..];
    if name.is_empty() {
        return Err("Not a task path.".into());
    }
    Ok((folder.to_string(), name.to_string()))
}

/// Turns a task on or off (the same switch as in Task Scheduler).
pub fn set_task_enabled(path: &str, enabled: bool) -> Result<(), String> {
    let (folder, name) = split_task_path(path)?;
    with_com(move || {
        let service = connect()?;
        // SAFETY: valid interfaces on this COM thread.
        unsafe {
            let f = service.GetFolder(&BSTR::from(folder)).map_err(err)?;
            let t = f.GetTask(&BSTR::from(name)).map_err(err)?;
            t.SetEnabled(if enabled { VARIANT_TRUE } else { VARIANT_FALSE })
                .map_err(|e| {
                    if e.code() == E_ACCESSDENIED {
                        "Windows needs administrator rights to change this task. \
                         Restart as administrator and try again."
                            .to_string()
                    } else {
                        err(e)
                    }
                })
        }
    })
}

/// Creates or replaces a task in `folder` (created if needed) from Task
/// Scheduler XML. It runs as the current user, only when signed in.
pub fn register_task(folder: &str, name: &str, xml: &str) -> Result<(), String> {
    let (folder, name, xml) = (folder.to_string(), name.to_string(), xml.to_string());
    with_com(move || {
        let service = connect()?;
        let none = VARIANT::default();
        // SAFETY: valid interfaces on this COM thread.
        unsafe {
            let root = service.GetFolder(&BSTR::from("\\")).map_err(err)?;
            let f = match service.GetFolder(&BSTR::from(folder.as_str())) {
                Ok(f) => f,
                Err(_) => root
                    .CreateFolder(&BSTR::from(folder.trim_start_matches('\\')), &none)
                    .map_err(err)?,
            };
            f.RegisterTask(
                &BSTR::from(name),
                &BSTR::from(xml),
                TASK_CREATE_OR_UPDATE.0,
                &none,
                &none,
                TASK_LOGON_INTERACTIVE_TOKEN,
                &none,
            )
            .map(|_| ())
            .map_err(err)
        }
    })
}

/// Deletes a task (and its folder when that is left empty). Missing tasks
/// are not an error.
pub fn delete_task(folder: &str, name: &str) -> Result<(), String> {
    let (folder, name) = (folder.to_string(), name.to_string());
    with_com(move || {
        let service = connect()?;
        // SAFETY: valid interfaces on this COM thread.
        unsafe {
            let Ok(f) = service.GetFolder(&BSTR::from(folder.as_str())) else {
                return Ok(());
            };
            let _ = f.DeleteTask(&BSTR::from(name), 0);
            let empty = f
                .GetTasks(TASK_ENUM_HIDDEN.0)
                .and_then(|t| t.Count())
                .unwrap_or(1)
                == 0
                && f.GetFolders(0).and_then(|t| t.Count()).unwrap_or(1) == 0;
            if empty && let Ok(root) = service.GetFolder(&BSTR::from("\\")) {
                let _ = root.DeleteFolder(&BSTR::from(folder.trim_start_matches('\\')), 0);
            }
            Ok(())
        }
    })
}

/// Starts a task now, as Task Scheduler would.
pub fn run_task(folder: &str, name: &str) -> Result<(), String> {
    let (folder, name) = (folder.to_string(), name.to_string());
    with_com(move || {
        let service = connect()?;
        // SAFETY: valid interfaces on this COM thread; an empty VARIANT
        // means no parameters.
        unsafe {
            let f = service.GetFolder(&BSTR::from(folder)).map_err(err)?;
            let t = f.GetTask(&BSTR::from(name)).map_err(err)?;
            t.Run(&VARIANT::default()).map(|_| ()).map_err(err)
        }
    })
}

/// Whether a task exists, and whether it is enabled.
pub fn task_state(folder: &str, name: &str) -> Result<Option<bool>, String> {
    let (folder, name) = (folder.to_string(), name.to_string());
    with_com(move || {
        let service = connect()?;
        // SAFETY: valid interfaces on this COM thread.
        unsafe {
            let Ok(f) = service.GetFolder(&BSTR::from(folder.as_str())) else {
                return Ok(None);
            };
            match f.GetTask(&BSTR::from(name)) {
                Ok(t) => Ok(Some(t.Enabled().map_err(err)?.as_bool())),
                Err(_) => Ok(None),
            }
        }
    })
}

// ------------------------------------------------------------ WMI

/// Runs a WQL query and returns the requested properties of each result as
/// text (missing or null values are `None`).
pub fn wmi_query(
    namespace: &str,
    query: &str,
    props: &[&str],
) -> Result<Vec<HashMap<String, Option<String>>>, String> {
    let (namespace, query) = (namespace.to_string(), query.to_string());
    let props: Vec<String> = props.iter().map(|p| p.to_string()).collect();
    with_com(move || {
        // SAFETY: COM is initialized on this thread; every interface
        // used comes from the calls before it.
        unsafe {
            let locator: IWbemLocator =
                CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).map_err(err)?;
            let services = locator
                .ConnectServer(
                    &BSTR::from(namespace),
                    &BSTR::new(),
                    &BSTR::new(),
                    &BSTR::new(),
                    0,
                    &BSTR::new(),
                    None,
                )
                .map_err(err)?;
            // RPC_C_AUTHN_WINNT = 10, RPC_C_AUTHZ_NONE = 0.
            CoSetProxyBlanket(
                &services,
                10,
                0,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )
            .map_err(err)?;
            let rows = services
                .ExecQuery(
                    &BSTR::from("WQL"),
                    &BSTR::from(query),
                    WBEM_GENERIC_FLAG_TYPE(
                        WBEM_FLAG_FORWARD_ONLY.0 | WBEM_FLAG_RETURN_IMMEDIATELY.0,
                    ),
                    None,
                )
                .map_err(err)?;
            let mut out = Vec::new();
            loop {
                let mut objs = [None];
                let mut got = 0u32;
                let hr = rows.Next(WBEM_INFINITE, &mut objs, &mut got);
                if hr.is_err() || got == 0 {
                    break;
                }
                let Some(obj) = objs[0].take() else { break };
                let mut row = HashMap::new();
                for p in &props {
                    let mut v = VARIANT::default();
                    let value = obj
                        .Get(&HSTRING::from(p.as_str()), 0, &mut v, None, None)
                        .ok()
                        .and_then(|()| variant_text(&v));
                    let _ = VariantClear(&mut v);
                    row.insert(p.clone(), value);
                }
                out.push(row);
            }
            Ok(out)
        }
    })
}

// ------------------------------------------------------------ shell links

/// The target and arguments of a `.lnk` shortcut.
pub fn shortcut_target(lnk: &Path) -> Option<(String, String)> {
    let lnk = lnk.to_path_buf();
    with_com(move || {
        // SAFETY: COM is initialized on this thread; the buffers are
        // sized and passed with their lengths.
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(err)?;
            let file: IPersistFile = link.cast().map_err(err)?;
            file.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ)
                .map_err(err)?;
            let mut path = vec![0u16; 1024];
            link.GetPath(&mut path, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
                .map_err(err)?;
            let mut args = vec![0u16; 1024];
            let _ = link.GetArguments(&mut args);
            let text = |b: &[u16]| {
                let n = b.iter().position(|c| *c == 0).unwrap_or(b.len());
                String::from_utf16_lossy(&b[..n])
            };
            Ok((text(&path), text(&args)))
        }
    })
    .ok()
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    /// Writes a `.lnk` shortcut (tests only).
    pub fn create_shortcut(lnk: &Path, target: &Path, args: &str) -> Result<(), String> {
        let (lnk, target, args) = (lnk.to_path_buf(), target.to_path_buf(), args.to_string());
        with_com(move || {
            // SAFETY: COM is initialized on this thread; the strings outlive
            // the calls.
            unsafe {
                let link: IShellLinkW =
                    CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(err)?;
                link.SetPath(&HSTRING::from(target.as_os_str()))
                    .map_err(err)?;
                link.SetArguments(&HSTRING::from(args)).map_err(err)?;
                let file: IPersistFile = link.cast().map_err(err)?;
                file.Save(&HSTRING::from(lnk.as_os_str()), true)
                    .map_err(err)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_paths_split() {
        assert_eq!(
            split_task_path(r"\Vendor\Updater").unwrap(),
            (r"\Vendor".to_string(), "Updater".to_string())
        );
        assert_eq!(
            split_task_path(r"\Top").unwrap(),
            (r"\".to_string(), "Top".to_string())
        );
        assert!(split_task_path(r"\Vendor\").is_err());
    }

    /// Read-only: lists logon tasks and queries WMI on the test machine.
    #[test]
    fn reads_tasks_and_wmi() {
        let tasks = logon_tasks().unwrap();
        assert!(
            tasks
                .iter()
                .all(|t| !t.path.to_ascii_lowercase().starts_with(r"\microsoft\"))
        );
        let os = wmi_query(
            r"ROOT\CIMV2",
            "SELECT Caption FROM Win32_OperatingSystem",
            &["Caption"],
        )
        .unwrap();
        assert_eq!(os.len(), 1);
        assert!(
            os[0]["Caption"]
                .as_deref()
                .unwrap_or("")
                .contains("Windows")
        );
    }

    /// Registers a throwaway task, toggles it, and deletes it.
    #[test]
    fn tasks_can_be_registered_toggled_and_deleted() {
        let folder = format!(r"\WinFastCleaner-test-{}", std::process::id());
        let xml = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers><LogonTrigger><Enabled>true</Enabled></LogonTrigger></Triggers>
  <Settings><Enabled>true</Enabled></Settings>
  <Actions Context="Author"><Exec><Command>cmd.exe</Command><Arguments>/c exit</Arguments></Exec></Actions>
</Task>"#;
        register_task(&folder, "logon", xml).unwrap();
        let path = format!(r"{folder}\logon");
        let listed = logon_tasks().unwrap();
        let t = listed.iter().find(|t| t.path == path).expect("listed");
        assert!(t.enabled);
        assert_eq!(t.command.as_deref(), Some("cmd.exe"));
        set_task_enabled(&path, false).unwrap();
        assert_eq!(task_state(&folder, "logon").unwrap(), Some(false));
        set_task_enabled(&path, true).unwrap();
        assert_eq!(task_state(&folder, "logon").unwrap(), Some(true));
        delete_task(&folder, "logon").unwrap();
        assert_eq!(task_state(&folder, "logon").unwrap(), None);
    }
}
