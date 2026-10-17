//! Files tab: large and old files, and duplicates.
//!
//! A search scans the chosen drive or folder (or reuses the Disk tab's scan
//! of the same root), then runs the finder. Results stay in the backend; the
//! UI picks files by index, and the picks become a red-tier plan that goes
//! through the recovery bin like every other hand-picked item.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use cleaner_core::dupes::{self, DupeOptions, DupePhase, DupeProgress};
use cleaner_core::filetypes::FileKind;
use cleaner_core::finder::{self, FinderOptions};
use cleaner_core::progress::{CancelToken, ScanProgress};
use cleaner_core::scan::{ScanOptions, scan};

use crate::cleaning::{ManualPlanView, context, manual_view};
use crate::commands::{save_snapshot, validate_scan_path};
use crate::state::{AppState, LoadedDupes, LoadedFind, LoadedScan};

type CmdResult<T> = Result<T, String>;

const MAX_LIMIT: u32 = 5_000;
const MAX_PICKS: usize = 100_000;
const TB: u64 = 1 << 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FindMode {
    Large,
    Old,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindRequest {
    mode: FindMode,
    min_size: u64,
    /// For old files: unused for at least this many days.
    older_than_days: Option<u32>,
    #[serde(default)]
    kinds: Vec<FileKind>,
    limit: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundView {
    index: u32,
    path: String,
    size: u64,
    modified: i64,
    last_used: i64,
    kind: FileKind,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindView {
    find_id: u64,
    root: String,
    reused_scan: bool,
    scanned_at: i64,
    files: Vec<FoundView>,
    more: u64,
    hidden: u64,
    recently_opened: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeFileView {
    index: u32,
    path: String,
    modified: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeGroupView {
    index: u32,
    size: u64,
    hash: String,
    wasted: u64,
    files: Vec<DupeFileView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupesView {
    dupes_id: u64,
    root: String,
    reused_scan: bool,
    scanned_at: i64,
    groups: Vec<DupeGroupView>,
    wasted: u64,
    files_hashed: u64,
    bytes_hashed: u64,
    unreadable: u64,
    hidden: u64,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FindEvent {
    Scanning {
        files: u64,
        bytes: u64,
    },
    Comparing {
        phase: DupePhase,
        done: u64,
        total: u64,
    },
    Files {
        result: FindView,
    },
    Dupes {
        result: DupesView,
    },
    Cancelled,
    Failed {
        message: String,
    },
}

fn finder_options(req: &FindRequest) -> CmdResult<FinderOptions> {
    if req.min_size > TB || req.limit == 0 || req.limit > MAX_LIMIT || req.kinds.len() > 16 {
        return Err("Those search settings are out of range.".into());
    }
    let older_than_days = match req.mode {
        FindMode::Large => None,
        FindMode::Old => match req.older_than_days {
            Some(d) if (1..=36_500).contains(&d) => Some(d),
            _ => return Err("Pick how old the files must be.".into()),
        },
    };
    Ok(FinderOptions {
        min_size: req.min_size,
        older_than_days,
        kinds: req.kinds.clone(),
        limit: req.limit as usize,
    })
}

/// The scan to search: the Disk tab's scan of the same root when allowed,
/// otherwise a fresh scan (which also saves a snapshot).
fn scan_for(
    state: &AppState,
    path: &std::path::Path,
    rescan: bool,
    cancel: &CancelToken,
    on_event: &Channel<FindEvent>,
) -> CmdResult<(Arc<LoadedScan>, bool)> {
    if !rescan {
        let root = cleaner_core::scan::resolve_root(path).map_err(|e| e.to_string())?;
        let current = state.lock().current.clone();
        if let Some(c) = current
            && cleaner_core::grew::same_root(&c.scan.summary.root, &root.display().to_string())
        {
            return Ok((c, true));
        }
    }
    let progress = |p: &ScanProgress| {
        let _ = on_event.send(FindEvent::Scanning {
            files: p.files,
            bytes: p.bytes,
        });
    };
    let done = scan(path, &ScanOptions::default(), &progress, cancel).map_err(|e| match e {
        cleaner_core::Error::Cancelled => "cancelled".to_string(),
        e => e.to_string(),
    })?;
    save_snapshot(&done);
    Ok((Arc::new(LoadedScan { id: 0, scan: done }), false))
}

/// Runs `work` on a background thread as the one Files tab search.
fn spawn_search(
    app: AppHandle,
    state: &AppState,
    on_event: Channel<FindEvent>,
    work: impl FnOnce(&AppState, &CancelToken, &Channel<FindEvent>) -> CmdResult<FindEvent>
    + Send
    + 'static,
) -> CmdResult<()> {
    let cancel = {
        let mut s = state.lock();
        if s.finding.is_some() {
            return Err("A search is already running.".into());
        }
        let cancel = CancelToken::new();
        s.finding = Some(cancel.clone());
        cancel
    };
    let spawned = std::thread::Builder::new()
        .name("files-search".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let result = catch_unwind(AssertUnwindSafe(|| work(&state, &cancel, &on_event)));
            state.lock().finding = None;
            let event = match result {
                _ if cancel.is_cancelled() => FindEvent::Cancelled,
                Ok(Ok(e)) => e,
                Ok(Err(message)) => FindEvent::Failed { message },
                Err(_) => FindEvent::Failed {
                    message: "The search stopped because of an internal error.".into(),
                },
            };
            let _ = on_event.send(event);
        });
    if let Err(e) = spawned {
        state.lock().finding = None;
        return Err(format!("Could not start the search: {e}"));
    }
    Ok(())
}

/// Finds large or old files under `path`.
#[tauri::command]
pub fn start_find(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    request: FindRequest,
    rescan: bool,
    on_event: Channel<FindEvent>,
) -> CmdResult<()> {
    let path = validate_scan_path(&path)?;
    let opts = finder_options(&request)?;
    spawn_search(app, &state, on_event, move |state, cancel, on_event| {
        let (loaded, reused) = scan_for(state, &path, rescan, cancel, on_event)?;
        let (_, _, ctx) = context()?;
        let result = finder::find(&loaded.scan.tree, &opts, &ctx, cancel);
        let mut s = state.lock();
        let id = s.new_id();
        let view = FindView {
            find_id: id,
            root: loaded.scan.summary.root.clone(),
            reused_scan: reused,
            scanned_at: loaded.scan.summary.started_at,
            files: result
                .files
                .iter()
                .enumerate()
                .map(|(i, f)| FoundView {
                    index: i as u32,
                    path: f.path.display().to_string(),
                    size: f.size,
                    modified: f.modified,
                    last_used: f.last_used,
                    kind: f.kind,
                })
                .collect(),
            more: result.more,
            hidden: result.hidden,
            recently_opened: result.recently_opened,
        };
        s.found = Some(Arc::new(LoadedFind { id, result }));
        Ok(FindEvent::Files { result: view })
    })
}

/// Finds duplicate files under `path`.
#[tauri::command]
pub fn start_dupes(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    min_size: u64,
    kinds: Vec<FileKind>,
    rescan: bool,
    on_event: Channel<FindEvent>,
) -> CmdResult<()> {
    let path = validate_scan_path(&path)?;
    if min_size == 0 || min_size > TB || kinds.len() > 16 {
        return Err("Those search settings are out of range.".into());
    }
    let opts = DupeOptions { min_size, kinds };
    spawn_search(app, &state, on_event, move |state, cancel, on_event| {
        let (loaded, reused) = scan_for(state, &path, rescan, cancel, on_event)?;
        let (_, _, ctx) = context()?;
        let progress = |p: DupeProgress| {
            let _ = on_event.send(FindEvent::Comparing {
                phase: p.phase,
                done: p.done,
                total: p.total,
            });
        };
        let result = dupes::find(&loaded.scan.tree, &opts, &ctx, &progress, cancel)
            .map_err(|e| e.to_string())?;
        let mut s = state.lock();
        let id = s.new_id();
        let view = DupesView {
            dupes_id: id,
            root: loaded.scan.summary.root.clone(),
            reused_scan: reused,
            scanned_at: loaded.scan.summary.started_at,
            groups: result
                .groups
                .iter()
                .enumerate()
                .map(|(gi, g)| DupeGroupView {
                    index: gi as u32,
                    size: g.size,
                    hash: g.hash.clone(),
                    wasted: g.wasted(),
                    files: g
                        .files
                        .iter()
                        .enumerate()
                        .map(|(fi, f)| DupeFileView {
                            index: fi as u32,
                            path: f.path.display().to_string(),
                            modified: f.modified,
                        })
                        .collect(),
                })
                .collect(),
            wasted: result.wasted,
            files_hashed: result.files_hashed,
            bytes_hashed: result.bytes_hashed,
            unreadable: result.unreadable,
            hidden: result.hidden,
        };
        s.dupes = Some(Arc::new(LoadedDupes { id, result }));
        Ok(FindEvent::Dupes { result: view })
    })
}

#[tauri::command]
pub fn cancel_find(state: State<'_, AppState>) {
    if let Some(cancel) = &state.lock().finding {
        cancel.cancel();
    }
}

/// Plans moving the picked large/old files to the recovery bin.
#[tauri::command(async)]
pub fn files_plan(
    state: State<'_, AppState>,
    find_id: u64,
    indexes: Vec<u32>,
) -> CmdResult<ManualPlanView> {
    if indexes.is_empty() || indexes.len() > MAX_PICKS {
        return Err("Pick at least one file.".into());
    }
    let found = match &state.lock().found {
        Some(f) if f.id == find_id => Arc::clone(f),
        _ => return Err("These results are out of date. Please search again.".into()),
    };
    let mut paths = Vec::with_capacity(indexes.len());
    for i in indexes {
        let f = found.result.files.get(i as usize).ok_or("Unknown file.")?;
        paths.push(f.path.clone());
    }
    let (_, settings, ctx) = context()?;
    let (plan, rejected) = cleaner_core::cleaner::plan_manual(&paths, &ctx);
    Ok(manual_view(
        &state,
        plan,
        rejected,
        "Large and old files",
        settings.green_to_bin,
    ))
}

/// Plans moving the picked duplicate copies to the recovery bin. Refused if
/// any group would lose every copy.
#[tauri::command(async)]
pub fn dupes_plan(
    state: State<'_, AppState>,
    dupes_id: u64,
    picks: Vec<(u32, u32)>,
) -> CmdResult<ManualPlanView> {
    if picks.is_empty() || picks.len() > MAX_PICKS {
        return Err("Pick at least one copy.".into());
    }
    let found = match &state.lock().dupes {
        Some(d) if d.id == dupes_id => Arc::clone(d),
        _ => return Err("These results are out of date. Please search again.".into()),
    };
    let picks: Vec<(usize, usize)> = picks
        .iter()
        .map(|&(g, f)| (g as usize, f as usize))
        .collect();
    let (_, settings, ctx) = context()?;
    let (plan, rejected) = dupes::plan_removal(&found.result.groups, &picks, &ctx)?;
    Ok(manual_view(
        &state,
        plan,
        rejected,
        "Duplicates",
        settings.green_to_bin,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(mode: FindMode, older: Option<u32>, limit: u32) -> FindRequest {
        FindRequest {
            mode,
            min_size: 1 << 20,
            older_than_days: older,
            kinds: Vec::new(),
            limit,
        }
    }

    #[test]
    fn requests_are_checked() {
        assert!(finder_options(&req(FindMode::Large, None, 200)).is_ok());
        assert_eq!(
            finder_options(&req(FindMode::Large, Some(30), 200))
                .unwrap()
                .older_than_days,
            None,
            "large ignores age"
        );
        assert!(finder_options(&req(FindMode::Old, None, 200)).is_err());
        assert!(finder_options(&req(FindMode::Old, Some(365), 200)).is_ok());
        assert!(finder_options(&req(FindMode::Large, None, 0)).is_err());
        assert!(finder_options(&req(FindMode::Large, None, 999_999)).is_err());
    }
}
