fn main() {
    // Declaring the app's commands in a manifest means each one must be
    // granted explicitly in `capabilities/`; anything not listed there is
    // rejected by Tauri before it reaches our code.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "app_info",
            "list_drives",
            "start_scan",
            "cancel_scan",
            "get_node",
            "list_children",
            "treemap",
            "open_in_explorer",
            "relaunch_elevated",
            "start_junk_scan",
            "cancel_junk_scan",
            "junk_items",
            "clean_preview",
            "clean_manual",
            "clean_execute",
            "cancel_clean",
            "deep_clean",
            "recovery_overview",
            "recovery_batch",
            "recovery_conflicts",
            "recovery_restore",
            "recovery_delete",
            "recovery_empty",
            "get_settings",
            "save_settings",
            "exclude_node",
            "open_rules_folder",
        ]),
    ))
    .expect("failed to run tauri-build");
}
