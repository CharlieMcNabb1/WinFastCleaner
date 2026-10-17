//! WinFastCleaner desktop app: a thin Tauri layer over `cleaner-core`.
//!
//! The frontend can only call the commands registered below, and only those
//! granted in `capabilities/main.json`. It refers to files by scan node IDs
//! or junk scan indexes, never by raw paths, except when choosing what to
//! scan and when editing the exclusions list (a list that only protects).

// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod automation_cmds;
mod cleaning;
mod commands;
mod files;
mod grew_cmds;
mod health_cmds;
mod privacy_cmds;
mod recover;
mod settings_cmds;
mod startup_cmds;
mod state;
mod updates_cmds;

/// Starts the tray monitor if it is turned on but not running (for example
/// right after an update, which closes it).
fn start_tray_if_wanted() {
    use cleaner_core::integration::{TRAY_PROGRAM, sibling, tray_running};
    let Some(data) = cleaner_core::paths::default_data_dir() else {
        return;
    };
    if cleaner_core::settings::Settings::load(&data).tray
        && !tray_running()
        && let Some(tray) = sibling(TRAY_PROGRAM)
    {
        let _ = std::process::Command::new(tray).spawn();
    }
}

fn main() {
    if cfg!(windows) {
        std::thread::spawn(start_tray_if_wanted);
    }
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
            commands::locate_node,
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
            files::start_find,
            files::start_dupes,
            files::cancel_find,
            files::files_plan,
            files::dupes_plan,
            grew_cmds::list_snapshots,
            grew_cmds::compare_snapshots,
            privacy_cmds::privacy_survey,
            privacy_cmds::privacy_clear,
            startup_cmds::startup_list,
            startup_cmds::startup_set_enabled,
            updates_cmds::updates_status,
            updates_cmds::updates_list,
            updates_cmds::updates_apply,
            updates_cmds::updates_cancel,
            health_cmds::health_check,
            automation_cmds::schedule_status,
            automation_cmds::schedule_save,
            automation_cmds::tray_status,
            automation_cmds::tray_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running WinFastCleaner");
}
