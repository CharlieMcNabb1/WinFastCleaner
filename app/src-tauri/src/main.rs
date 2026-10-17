//! WinFastCleaner desktop app: a thin Tauri layer over `cleaner-core`.
//!
//! The frontend can only call the commands in `commands.rs`, and only those
//! granted in `capabilities/main.json`. It refers to files by scan node IDs,
//! never by raw paths, except when choosing what to scan.

// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod state;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list_drives,
            commands::start_scan,
            commands::cancel_scan,
            commands::get_node,
            commands::list_children,
            commands::treemap,
            commands::open_in_explorer,
            commands::relaunch_elevated,
        ])
        .run(tauri::generate_context!())
        .expect("error while running WinFastCleaner");
}
