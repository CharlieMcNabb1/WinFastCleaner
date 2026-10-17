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
        ]),
    ))
    .expect("failed to run tauri-build");
}
