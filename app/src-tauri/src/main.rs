//! WinFastCleaner desktop app: a thin Tauri layer over `cleaner-core`.
//!
//! The frontend can only call the commands registered below, and only those
//! granted in `capabilities/main.json`. It refers to files by scan node IDs
//! or junk scan indexes, never by raw paths, except when choosing what to
//! scan and when editing the exclusions list (a list that only protects).

// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cleaning;
mod commands;
mod recover;
mod settings_cmds;
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
            cleaning::start_junk_scan,
            cleaning::cancel_junk_scan,
            cleaning::junk_items,
            cleaning::clean_preview,
            cleaning::clean_manual,
            cleaning::clean_execute,
            cleaning::cancel_clean,
            cleaning::deep_clean,
            recover::recovery_overview,
            recover::recovery_batch,
            recover::recovery_conflicts,
            recover::recovery_restore,
            recover::recovery_delete,
            recover::recovery_empty,
            settings_cmds::get_settings,
            settings_cmds::save_settings,
            settings_cmds::exclude_node,
            settings_cmds::open_rules_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running WinFastCleaner");
}
