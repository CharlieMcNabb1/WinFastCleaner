//! `wfc-tray`: WinFastCleaner's tray monitor.
//!
//! A small background program (no web view) that shows an icon in the
//! notification area and, every hour, runs the health check. It notifies
//! when a drive is low on space, when junk piles up, or when a check turns
//! red, each at most once a day (see `cleaner_core::monitor`). It only
//! reads: it never cleans or changes anything. It starts at sign-in when the
//! tray monitor is turned on in the app's Settings, and quits when it is
//! turned off.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The tray monitor only runs on Windows.");
    std::process::exit(1);
}

/// What one check found, for the tooltip and the notifications.
#[cfg_attr(not(windows), allow(dead_code))]
struct Outcome {
    tip: String,
    alerts: Vec<cleaner_core::monitor::Alert>,
    /// The tray was turned off in Settings.
    quit: bool,
}

/// Runs the health check (and, at most every six hours, a read-only junk
/// scan) and works out which notifications are due.
#[cfg_attr(not(windows), allow(dead_code))]
fn check(data: &std::path::Path) -> Outcome {
    use cleaner_core::health::{self, CheckId, Status};
    use cleaner_core::monitor::{self, MonitorState};
    use cleaner_core::progress::CancelToken;
    use cleaner_core::settings::Settings;

    const JUNK_EVERY: i64 = 6 * 60 * 60;
    let settings = Settings::load(data);
    if !settings.tray {
        return Outcome {
            tip: String::new(),
            alerts: Vec::new(),
            quit: true,
        };
    }
    let now = cleaner_core::scan::now_unix();
    let mut state = MonitorState::load(data);
    if state
        .junk_scanned
        .is_none_or(|t| t > now || now - t >= JUNK_EVERY)
    {
        health::refresh_junk(data, &settings, &CancelToken::new());
        state.junk_scanned = Some(now);
    }
    let checks = health::evaluate(&health::gather(data, &settings), &settings.health);
    let alerts = monitor::alerts(&checks, &mut state, now);
    let _ = state.save(data);

    let overall = match health::overall(&checks) {
        Status::Red => "a problem needs attention",
        Status::Yellow => "something needs attention",
        _ => "all good",
    };
    let mut tip = format!("WinFastCleaner: {overall}");
    if let Some(c) = checks.iter().find(|c| c.id == CheckId::DiskSpace) {
        tip.push('\n');
        tip.push_str(&c.summary);
    }
    Outcome {
        tip: monitor::clip(&tip, 127),
        alerts,
        quit: false,
    }
}
