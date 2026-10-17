//! The Windows side of the tray: a hidden window, the notification-area
//! icon, its menu, and notifications. Checks run on a worker thread and
//! report back to the window, so the icon always responds.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::{
    ExtractIconExW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO,
    NIIF_RESPECT_QUIET_TIME, NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NIN_BALLOONUSERCLICK, NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, HICON, IDI_APPLICATION, KillTimer, LoadIconW,
    MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, SetTimer, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, WM_APP, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_DESTROY,
    WM_NULL, WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

use cleaner_core::integration::{TRAY_WINDOW_CLASS, sibling};
use cleaner_core::monitor::clip;

use crate::Outcome;

const WM_TRAY: u32 = WM_APP + 1;
/// The icon was chosen with the keyboard (NIN_SELECT | NINF_KEY).
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const WM_CHECKED: u32 = WM_APP + 2;
const ICON_ID: u32 = 1;
const TIMER_ID: usize = 1;
/// The first check waits a little, so it doesn't slow down signing in.
const FIRST_CHECK_MS: u32 = 2 * 60 * 1000;
const CHECK_EVERY_MS: u32 = 60 * 60 * 1000;

const ID_OPEN: usize = 1;
const ID_CHECK: usize = 2;
const ID_QUIT: usize = 3;

/// The app's main program, which the tray opens.
const APP_PROGRAM: &str = "WinFastCleaner.exe";

static CHECKING: AtomicBool = AtomicBool::new(false);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

struct State {
    icon: HICON,
    tip: String,
    first_check_done: bool,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Copies `s` into a fixed UTF-16 buffer, cut short and NUL-terminated.
fn fill(buf: &mut [u16], s: &str) {
    let units: Vec<u16> = s.encode_utf16().take(buf.len() - 1).collect();
    buf[..units.len()].copy_from_slice(&units);
    buf[units.len()] = 0;
}

fn data_dir() -> Option<PathBuf> {
    cleaner_core::paths::default_data_dir()
}

pub fn run() {
    let name = wide(r"Local\WinFastCleanerTrayMonitor");
    // SAFETY: a named mutex with default security; it lives as long as the
    // process and keeps a second copy from starting.
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    // SAFETY: reads this thread's last error, set by CreateMutexW.
    if mutex.is_null() || unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return;
    }
    if data_dir().is_none() {
        return;
    }

    let class = wide(TRAY_WINDOW_CLASS);
    let taskbar = wide("TaskbarCreated");
    // SAFETY: the strings are NUL-terminated and outlive the calls; the
    // window class points at `wndproc`, which lives for the whole program.
    let hwnd = unsafe {
        TASKBAR_CREATED.store(RegisterWindowMessageW(taskbar.as_ptr()), Ordering::Relaxed);
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            // SAFETY (fields): all other fields may be zero.
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            return;
        }
        // A hidden top-level window (never shown), so the menu behaves and
        // the "taskbar restarted" broadcast arrives.
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            wide("WinFastCleaner tray monitor").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return;
    }
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            icon: load_icon(),
            tip: "WinFastCleaner".into(),
            first_check_done: false,
        })
    });
    add_icon(hwnd);
    // SAFETY: a timer on our own window.
    unsafe { SetTimer(hwnd, TIMER_ID, FIRST_CHECK_MS, None) };

    // SAFETY: a standard message loop on the thread that owns the window.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// The app's own icon (from its program file), or Windows' generic one.
fn load_icon() -> HICON {
    if let Some(app) = sibling(APP_PROGRAM) {
        let path = wide(&app.display().to_string());
        let mut small: HICON = std::ptr::null_mut();
        // SAFETY: asks for one small icon into `small`; `path` is
        // NUL-terminated.
        let n = unsafe { ExtractIconExW(path.as_ptr(), 0, std::ptr::null_mut(), &mut small, 1) };
        if n > 0 && !small.is_null() {
            return small;
        }
    }
    // SAFETY: a stock system icon.
    unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) }
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    // SAFETY: NOTIFYICONDATAW is plain data; all zeros is a valid start.
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = ICON_ID;
    nid
}

fn add_icon(hwnd: HWND) {
    let mut nid = icon_data(hwnd);
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
    nid.uCallbackMessage = WM_TRAY;
    STATE.with(|s| {
        if let Some(st) = s.borrow().as_ref() {
            nid.hIcon = st.icon;
            fill(&mut nid.szTip, &st.tip);
        }
    });
    // SAFETY: `nid` is fully set up for our window.
    unsafe {
        Shell_NotifyIconW(NIM_ADD, &nid);
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }
}

fn set_tip(hwnd: HWND, tip: &str) {
    let mut nid = icon_data(hwnd);
    nid.uFlags = NIF_TIP | NIF_SHOWTIP;
    fill(&mut nid.szTip, tip);
    // SAFETY: modifies our own icon.
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

fn notify(hwnd: HWND, title: &str, body: &str, warning: bool) {
    let mut nid = icon_data(hwnd);
    nid.uFlags = NIF_INFO;
    fill(&mut nid.szInfoTitle, &clip(title, 63));
    fill(&mut nid.szInfo, &clip(body, 255));
    nid.dwInfoFlags = (if warning { NIIF_WARNING } else { NIIF_INFO }) | NIIF_RESPECT_QUIET_TIME;
    // SAFETY: modifies our own icon; Windows shows it as a notification.
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

fn remove_icon(hwnd: HWND) {
    let nid = icon_data(hwnd);
    // SAFETY: removes our own icon.
    unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
}

fn open_app() {
    if let Some(app) = sibling(APP_PROGRAM) {
        let _ = std::process::Command::new(app).spawn();
    }
}

fn show_menu(hwnd: HWND) {
    // SAFETY: a popup menu owned by this function and destroyed below; the
    // window is brought to the front first so the menu closes properly.
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        AppendMenuW(
            menu,
            MF_STRING,
            ID_OPEN,
            wide("Open WinFastCleaner").as_ptr(),
        );
        AppendMenuW(menu, MF_STRING, ID_CHECK, wide("Check now").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(
            menu,
            MF_STRING,
            ID_QUIT,
            wide("Quit until next sign-in").as_ptr(),
        );
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        if cmd > 0 {
            PostMessageW(hwnd, WM_COMMAND, cmd as usize, 0);
        }
    }
}

/// Runs a check on a worker thread; the result comes back as WM_CHECKED.
fn start_check(hwnd: HWND) {
    if CHECKING.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(data) = data_dir() else {
        CHECKING.store(false, Ordering::Release);
        return;
    };
    let target = hwnd as usize;
    let spawned = std::thread::Builder::new()
        .name("tray-check".into())
        .spawn(move || {
            let outcome = std::panic::catch_unwind(|| crate::check(&data)).unwrap_or(Outcome {
                tip: "WinFastCleaner".into(),
                alerts: Vec::new(),
                quit: false,
            });
            let ptr = Box::into_raw(Box::new(outcome));
            // SAFETY: hands the box to the window thread, which frees it; if
            // the window is gone the post fails and the box is freed here.
            if unsafe { PostMessageW(target as HWND, WM_CHECKED, 0, ptr as LPARAM) } == 0 {
                // SAFETY: the post failed, so nobody else owns `ptr`.
                drop(unsafe { Box::from_raw(ptr) });
                CHECKING.store(false, Ordering::Release);
            }
        });
    if spawned.is_err() {
        CHECKING.store(false, Ordering::Release);
    }
}

fn checked(hwnd: HWND, outcome: Outcome) {
    CHECKING.store(false, Ordering::Release);
    if outcome.quit {
        // SAFETY: closes our own window.
        unsafe { DestroyWindow(hwnd) };
        return;
    }
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.tip = outcome.tip.clone();
        }
    });
    set_tip(hwnd, &outcome.tip);
    match outcome.alerts.as_slice() {
        [] => {}
        [one] => notify(hwnd, &one.title, &one.body, one.warning),
        many => notify(
            hwnd,
            "WinFastCleaner needs your attention",
            &many
                .iter()
                .map(|a| a.title.as_str())
                .collect::<Vec<_>>()
                .join(". "),
            many.iter().any(|a| a.warning),
        ),
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            match (lparam as u32) & 0xFFFF {
                NIN_SELECT | NIN_KEYSELECT | NIN_BALLOONUSERCLICK => open_app(),
                WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match wparam & 0xFFFF {
                ID_OPEN => open_app(),
                ID_CHECK => start_check(hwnd),
                // SAFETY: closes our own window.
                ID_QUIT => unsafe {
                    DestroyWindow(hwnd);
                },
                _ => {}
            }
            0
        }
        WM_TIMER => {
            let first = STATE.with(|s| {
                s.borrow_mut()
                    .as_mut()
                    .map(|st| !std::mem::replace(&mut st.first_check_done, true))
                    .unwrap_or(false)
            });
            if first {
                // SAFETY: re-arms our own timer at the regular interval.
                unsafe {
                    KillTimer(hwnd, TIMER_ID);
                    SetTimer(hwnd, TIMER_ID, CHECK_EVERY_MS, None);
                }
            }
            start_check(hwnd);
            0
        }
        WM_CHECKED => {
            if lparam != 0 {
                // SAFETY: the pointer came from Box::into_raw in start_check
                // and is taken back exactly once, here.
                let outcome = unsafe { Box::from_raw(lparam as *mut Outcome) };
                checked(hwnd, *outcome);
            }
            0
        }
        WM_CLOSE => {
            // SAFETY: closes our own window.
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            remove_icon(hwnd);
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            0
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            // Explorer restarted: put the icon back.
            add_icon(hwnd);
            0
        }
        // SAFETY: default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
