//! `wfcw`: the scheduled cleaning entry point, without a console window
//! (like `pythonw` next to `python`). Windows Task Scheduler runs
//! `wfcw.exe clean --scheduled`; anything else is refused. What it did is
//! written to the log and to `scheduled-last.json` in the data folder,
//! where the app shows it.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rules_dir = match args.as_slice() {
        [a, b] if a == "clean" && b == "--scheduled" => None,
        // For tests: use one folder's rules instead of the built-in ones.
        [a, b, c, d] if a == "clean" && b == "--scheduled" && c == "--rules-dir" => {
            Some(PathBuf::from(d))
        }
        _ => return ExitCode::from(2),
    };
    let Some(data) = cleaner_core::paths::default_data_dir() else {
        return ExitCode::FAILURE;
    };
    let last = cleaner_core::schedule::run(
        &data,
        rules_dir.as_deref(),
        &cleaner_core::progress::CancelToken::new(),
    );
    if last.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
