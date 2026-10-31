//! Tauri commands: the only entry points the frontend can call.
//!
//! Every input from the frontend is validated here. Actions on files take a
//! `(scan_id, node_id)` pair and resolve the path from the backend's own scan
//! tree, so the frontend can never point an action at an arbitrary path.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use cleaner_core::platform::{self, DriveInfo};
use cleaner_core::progress::{CancelToken, ScanProgress};
use cleaner_core::safety::Guard;
use cleaner_core::scan::{Scan, ScanOptions, ScanSummary, scan};
use cleaner_core::settings::{Exclusions, Settings};
use cleaner_core::snapshot;
use cleaner_core::tree::{NodeId, Tree};

use crate::state::{AppState, LoadedScan};

type CmdResult<T> = Result<T, String>;

/// Longest path accepted from the frontend (the Windows maximum).
const MAX_PATH_CHARS: usize = 32_767;
const MAX_PAGE: u32 = 2_000;
const MAX_TREEMAP_DEPTH: u8 = 3;
const MAX_TREEMAP_CHILDREN: u16 = 400;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: &'static str,
    version: &'static str,
    platform: &'static str,
    elevated: bool,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: cleaner_core::APP_NAME,
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        elevated: platform::is_elevated(),
    }
}

/// Drive queries can block (sleeping disks, network drives), so this runs
/// off the main thread.
#[tauri::command(async)]
pub fn list_drives() -> Vec<DriveInfo> {
    platform::drives()
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScanEvent {
    Progress {
        #[serde(flatten)]
        progress: ScanProgress,
    },
    Done {
        scan_id: u64,
        summary: ScanSummary,
        snapshot_saved: bool,
    },
    Cancelled {
        scan_id: u64,
    },
    Failed {
        scan_id: u64,
        message: String,
    },
}

/// Checks a path typed or picked in the UI before scanning it.
pub(crate) fn validate_scan_path(raw: &str) -> CmdResult<PathBuf> {
    if raw.is_empty() || raw.chars().count() > MAX_PATH_CHARS || raw.contains('\0') {
        return Err("That is not a valid folder path.".into());
    }
    if raw.starts_with(r"\\.\") || raw.starts_with("//./") {
        return Err("Device paths can't be scanned. Pick a drive or folder.".into());
    }
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err("Enter a full path, for example C:\\Users.".into());
    }
    Ok(path)
}

/// Starts a scan in the background and streams progress to `on_event`.
/// Returns the new scan's id.
#[tauri::command]
pub fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    allow_mft: bool,
    on_event: Channel<ScanEvent>,
) -> CmdResult<u64> {
    let path = validate_scan_path(&path)?;
    let (id, cancel) = {
        let mut s = state.lock();
        if s.running.is_some() {
            return Err("A scan is already running.".into());
        }
        let id = s.new_id();
        let cancel = CancelToken::new();
        s.running = Some((id, cancel.clone()));
        (id, cancel)
    };

    let spawned = std::thread::Builder::new()
        .name(format!("scan-{id}"))
        .spawn(move || {
            let progress = |p: &ScanProgress| {
                let _ = on_event.send(ScanEvent::Progress {
                    progress: p.clone(),
                });
            };
            let opts = ScanOptions { allow_mft };
            let result = catch_unwind(AssertUnwindSafe(|| scan(&path, &opts, &progress, &cancel)));
            let state = app.state::<AppState>();
            let event = match result {
                Ok(Ok(done)) => {
                    let snapshot_saved = save_snapshot(&done);
                    let summary = done.summary.clone();
                    let mut s = state.lock();
                    s.current = Some(Arc::new(LoadedScan { id, scan: done }));
                    s.running = None;
                    ScanEvent::Done {
                        scan_id: id,
                        summary,
                        snapshot_saved,
                    }
                }
                Ok(Err(cleaner_core::Error::Cancelled)) => {
                    state.lock().running = None;
                    ScanEvent::Cancelled { scan_id: id }
                }
                Ok(Err(e)) => {
                    state.lock().running = None;
                    ScanEvent::Failed {
                        scan_id: id,
                        message: e.to_string(),
                    }
                }
                Err(_) => {
                    state.lock().running = None;
                    ScanEvent::Failed {
                        scan_id: id,
                        message: "The scan stopped because of an internal error.".into(),
                    }
                }
            };
            let _ = on_event.send(event);
        });
    if let Err(e) = spawned {
        state.lock().running = None;
        return Err(format!("Could not start the scan: {e}"));
    }
    Ok(id)
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) {
    if let Some((_, cancel)) = &state.lock().running {
        cancel.cancel();
    }
}

pub(crate) fn save_snapshot(scan: &Scan) -> bool {
    let Some(data) = cleaner_core::paths::default_data_dir() else {
        return false;
    };
    let days = Settings::load(&data).snapshot_days;
    snapshot::record(&data, scan, days).is_ok()
}

pub(crate) fn loaded(state: &AppState, scan_id: u64) -> CmdResult<Arc<LoadedScan>> {
    match &state.lock().current {
        Some(s) if s.id == scan_id => Ok(Arc::clone(s)),
        _ => Err("That scan is no longer loaded. Please scan again.".into()),
    }
}

fn check_node(tree: &Tree, node_id: NodeId) -> CmdResult<()> {
    tree.get(node_id)
        .map(|_| ())
        .ok_or_else(|| "Unknown item.".to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    id: NodeId,
    name: String,
    is_dir: bool,
    size: u64,
    allocated: u64,
    files: u64,
    dirs: u64,
    modified: i64,
    flags: u16,
    child_count: usize,
    /// The safety guard never lets the cleaner touch this item.
    protected: bool,
    /// On the user's exclusions list.
    excluded: bool,
}

/// What the Disk view shows about whether items may be cleaned.
struct Marks {
    guard: &'static Guard,
    exclusions: Exclusions,
}

impl Marks {
    fn load() -> Marks {
        static GUARD: OnceLock<Guard> = OnceLock::new();
        let exclusions = cleaner_core::paths::default_data_dir()
            .map(|d| Exclusions::new(&Settings::load(&d).exclusions))
            .unwrap_or_default();
        Marks {
            guard: GUARD.get_or_init(Guard::system),
            exclusions,
        }
    }
}

fn view(tree: &Tree, id: NodeId, marks: &Marks) -> NodeView {
    let n = tree.node(id);
    let path = tree.path(id);
    NodeView {
        id,
        name: tree.name(id).into_owned(),
        is_dir: n.is_dir(),
        size: n.size(),
        allocated: n.allocated(),
        files: n.files(),
        dirs: n.dirs(),
        modified: n.modified(),
        flags: n.flags(),
        child_count: n.child_count(),
        protected: marks.guard.check(&path).is_err(),
        excluded: marks.exclusions.is_excluded(&path),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    id: NodeId,
    name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDetail {
    #[serde(flatten)]
    node: NodeView,
    path: String,
    lineage: Vec<Crumb>,
}

#[tauri::command(async)]
pub fn get_node(
    state: State<'_, AppState>,
    scan_id: u64,
    node_id: NodeId,
) -> CmdResult<NodeDetail> {
    let loaded = loaded(&state, scan_id)?;
    let tree = &loaded.scan.tree;
    check_node(tree, node_id)?;
    Ok(NodeDetail {
        node: view(tree, node_id, &Marks::load()),
        path: tree.path(node_id).display().to_string(),
        lineage: tree
            .lineage(node_id)
            .into_iter()
            .map(|id| Crumb {
                id,
                name: tree.name(id).into_owned(),
            })
            .collect(),
    })
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    Size,
    Allocated,
    Name,
    Modified,
    Files,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildrenPage {
    total: usize,
    items: Vec<NodeView>,
    /// Entries after this page.
    rest_count: usize,
    rest_size: u64,
}

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn list_children(
    state: State<'_, AppState>,
    scan_id: u64,
    node_id: NodeId,
    offset: u32,
    limit: u32,
    sort: SortKey,
    descending: bool,
    dirs_only: bool,
) -> CmdResult<ChildrenPage> {
    let loaded = loaded(&state, scan_id)?;
    let tree = &loaded.scan.tree;
    check_node(tree, node_id)?;
    let mut ids: Vec<NodeId> = tree
        .children(node_id)
        .iter()
        .copied()
        .filter(|&c| !dirs_only || tree.node(c).is_dir())
        .collect();
    // Children are stored largest first; other orders are sorted on demand.
    match sort {
        SortKey::Size => {}
        SortKey::Allocated => ids.sort_by_key(|&c| std::cmp::Reverse(tree.node(c).allocated())),
        SortKey::Name => ids.sort_by_cached_key(|&c| tree.name(c).to_lowercase()),
        SortKey::Modified => ids.sort_by_key(|&c| std::cmp::Reverse(tree.node(c).modified())),
        SortKey::Files => ids.sort_by_key(|&c| std::cmp::Reverse(tree.node(c).files())),
    }
    let ascending_by_default = matches!(sort, SortKey::Name);
    if descending == ascending_by_default {
        ids.reverse();
    }
    let total = ids.len();
    let start = (offset as usize).min(total);
    let end = start
        .saturating_add(limit.min(MAX_PAGE) as usize)
        .min(total);
    let rest = &ids[end..];
    let marks = Marks::load();
    Ok(ChildrenPage {
        total,
        items: ids[start..end]
            .iter()
            .map(|&c| view(tree, c, &marks))
            .collect(),
        rest_count: rest.len(),
        rest_size: rest.iter().map(|&c| tree.node(c).size()).sum(),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreemapNode {
    id: NodeId,
    name: String,
    size: u64,
    is_dir: bool,
    flags: u16,
    children: Vec<TreemapNode>,
    /// Size of children too small to show individually.
    other_size: u64,
    other_count: usize,
}

fn treemap_node(tree: &Tree, id: NodeId, depth: u8, max_children: usize) -> TreemapNode {
    let n = tree.node(id);
    let mut children = Vec::new();
    let (mut other_size, mut other_count) = (0, 0);
    if depth > 0 {
        for (i, &c) in tree.children(id).iter().enumerate() {
            let size = tree.node(c).size();
            if size == 0 {
                break; // sorted largest first
            }
            if i < max_children {
                children.push(treemap_node(tree, c, depth - 1, max_children));
            } else {
                other_size += size;
                other_count += 1;
            }
        }
    }
    TreemapNode {
        id,
        name: tree.name(id).into_owned(),
        size: n.size(),
        is_dir: n.is_dir(),
        flags: n.flags(),
        children,
        other_size,
        other_count,
    }
}

#[tauri::command(async)]
pub fn treemap(
    state: State<'_, AppState>,
    scan_id: u64,
    node_id: NodeId,
    depth: u8,
    max_children: u16,
) -> CmdResult<TreemapNode> {
    let loaded = loaded(&state, scan_id)?;
    let tree = &loaded.scan.tree;
    check_node(tree, node_id)?;
    Ok(treemap_node(
        tree,
        node_id,
        depth.clamp(1, MAX_TREEMAP_DEPTH),
        max_children.clamp(1, MAX_TREEMAP_CHILDREN) as usize,
    ))
}

/// Finds a path in a Disk scan (for "Show in Disk" links). Read-only.
#[tauri::command(async)]
pub fn locate_node(
    state: State<'_, AppState>,
    scan_id: u64,
    path: String,
) -> CmdResult<Option<NodeId>> {
    let path = validate_scan_path(&path)?;
    let loaded = loaded(&state, scan_id)?;
    Ok(loaded.scan.tree.find(&path))
}

/// Shows the item selected in File Explorer.
#[tauri::command(async)]
pub fn open_in_explorer(
    state: State<'_, AppState>,
    scan_id: u64,
    node_id: NodeId,
) -> CmdResult<()> {
    let loaded = loaded(&state, scan_id)?;
    let tree = &loaded.scan.tree;
    check_node(tree, node_id)?;
    let path = tree.path(node_id);
    if path.symlink_metadata().is_err() {
        return Err("That item no longer exists.".into());
    }
    reveal(&path)
}

#[cfg(windows)]
fn reveal(path: &Path) -> CmdResult<()> {
    use std::os::windows::process::CommandExt;
    let display = path.display().to_string();
    // Quotes cannot appear in Windows paths; refuse anything odd rather than
    // build a command line from it.
    if display.contains('"') {
        return Err("That path can't be opened.".into());
    }
    let explorer = platform::windows_dir()
        .map(|w| w.join("explorer.exe"))
        .ok_or_else(|| "Could not find File Explorer.".to_string())?;
    std::process::Command::new(explorer)
        .raw_arg(format!("/select,\"{display}\""))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open File Explorer: {e}"))
}

/// Opens the item's folder (Linux file managers have no common way to
/// select one item).
#[cfg(not(windows))]
fn reveal(path: &Path) -> CmdResult<()> {
    match path.parent() {
        Some(parent) if !path.is_dir() => open_folder(parent),
        _ => open_folder(path),
    }
}

/// Opens a folder in File Explorer.
#[cfg(windows)]
pub(crate) fn open_folder(path: &Path) -> CmdResult<()> {
    use std::os::windows::process::CommandExt;
    let display = path.display().to_string();
    if display.contains('"') {
        return Err("That folder can't be opened.".into());
    }
    let explorer = platform::windows_dir()
        .map(|w| w.join("explorer.exe"))
        .ok_or_else(|| "Could not find File Explorer.".to_string())?;
    std::process::Command::new(explorer)
        .raw_arg(format!("\"{display}\""))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open File Explorer: {e}"))
}

/// Opens a folder in the user's file manager through `xdg-open`, from its
/// fixed location (never through `PATH`), with the path as one argument.
#[cfg(not(windows))]
pub(crate) fn open_folder(path: &Path) -> CmdResult<()> {
    let xdg_open = ["/usr/bin/xdg-open", "/run/current-system/sw/bin/xdg-open"]
        .into_iter()
        .map(Path::new)
        .find(|p| p.is_file())
        .ok_or("Could not find xdg-open to open the folder.")?;
    std::process::Command::new(xdg_open)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open the folder: {e}"))
}

/// Restarts the app with administrator rights (one UAC prompt), which
/// enables the fast MFT scan.
#[tauri::command]
pub fn relaunch_elevated(app: AppHandle) -> CmdResult<()> {
    if platform::is_elevated() {
        return Err("Already running as administrator.".into());
    }
    elevate()?;
    app.exit(0);
    Ok(())
}

#[cfg(windows)]
fn elevate() -> CmdResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let verb: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();
    // SAFETY: all strings are NUL-terminated UTF-16 that outlive the call;
    // null parameters/directory are allowed by the API.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            exe.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success; the user may also have declined UAC.
    if result as isize > 32 {
        Ok(())
    } else {
        Err("Administrator access was not granted.".into())
    }
}

#[cfg(not(windows))]
fn elevate() -> CmdResult<()> {
    Err("Restarting as administrator is only available on Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests write fresh files to temp folders; the walker sees them
    /// immediately, while the on-disk MFT may lag behind, so tests that check
    /// exact numbers always walk.
    const WALK_ONLY: ScanOptions = ScanOptions { allow_mft: false };

    #[test]
    fn scan_paths_are_validated() {
        assert!(validate_scan_path("").is_err());
        assert!(validate_scan_path("relative/path").is_err());
        assert!(validate_scan_path("a\0b").is_err());
        assert!(validate_scan_path(r"\\.\PhysicalDrive0").is_err());
        assert!(validate_scan_path(&"x".repeat(40_000)).is_err());
        #[cfg(windows)]
        assert!(validate_scan_path(r"C:\Users").is_ok());
        #[cfg(unix)]
        assert!(validate_scan_path("/home").is_ok());
    }

    #[test]
    fn treemap_groups_small_children() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..10 {
            std::fs::write(tmp.path().join(format!("f{i}")), vec![0u8; 100 * (i + 1)]).unwrap();
        }
        let done = scan(tmp.path(), &WALK_ONLY, &|_| {}, &CancelToken::new()).unwrap();
        let t = treemap_node(&done.tree, 0, 1, 3);
        assert_eq!(t.children.len(), 3);
        assert_eq!(t.other_count, 7);
        assert_eq!(t.children[0].size, 1000);
        assert_eq!(t.other_size, (1..=7).map(|i| 100 * i).sum::<u64>());
    }
}
