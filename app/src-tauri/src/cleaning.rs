//! Cleaner commands: junk scan, preview, clean, cleaning items picked in the
//! Disk view, and the separate "Deep clean" (Windows component store).
//!
//! The frontend never sends file paths. It picks rules and refers to files
//! by their index in the backend's own junk scan (or by Disk view node), the
//! backend builds the exact plan, and only a plan the user has previewed can
//! be executed, once.

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use cleaner_core::cleaner::{
    self, CleanPlan, CleanReport, Context, ExecOptions, PlanItem, Rejected, RuleStatus,
};
use cleaner_core::progress::CancelToken;
use cleaner_core::recovery::RecoveryStore;
use cleaner_core::rules::{self, RuleError, RuleSource, Tier};
use cleaner_core::settings::{Exclusions, Settings};
use cleaner_core::tree::NodeId;

use crate::state::{AppState, LoadedJunk, PreviewedPlan};

type CmdResult<T> = Result<T, String>;

const MAX_ITEMS_PAGE: u32 = 1_000;
const MAX_SELECTION: usize = 2_000;
const MAX_MANUAL_NODES: usize = 10_000;
const MANUAL_SAMPLE: usize = 100;

/// The per-user data folder.
pub fn data_dir() -> CmdResult<PathBuf> {
    cleaner_core::paths::default_data_dir()
        .ok_or_else(|| "Could not find your app data folder (LOCALAPPDATA).".to_string())
}

/// The cleaner's view of the system, with the user's current exclusions.
pub fn context() -> CmdResult<(PathBuf, Settings, Context)> {
    let data = data_dir()?;
    let settings = Settings::load(&data);
    let ctx = Context::system(Exclusions::new(&settings.exclusions));
    Ok((data, settings, ctx))
}

// ------------------------------------------------------------ junk scan

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleView {
    id: String,
    name: String,
    group: String,
    tier: Tier,
    description: String,
    lose: String,
    requires_admin: bool,
    close_apps: Vec<String>,
    min_age_days: Option<u32>,
    user_rule: bool,
    status: RuleStatus,
    files: usize,
    size: u64,
    /// Matches left out because they are protected, excluded, links, or
    /// system or cloud files.
    hidden: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkView {
    scan_id: u64,
    rules: Vec<RuleView>,
    errors: Vec<RuleError>,
    scanned_at: i64,
    duration_ms: u64,
    elevated: bool,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum JunkEvent {
    Done { result: JunkView },
    Cancelled,
    Failed { message: String },
}

fn junk_view(loaded: &LoadedJunk, duration_ms: u64, elevated: bool) -> JunkView {
    JunkView {
        scan_id: loaded.id,
        rules: loaded
            .scan
            .rules
            .iter()
            .map(|r| RuleView {
                id: r.rule.id.clone(),
                name: r.rule.name.clone(),
                group: r.rule.group.clone(),
                tier: r.rule.tier,
                description: r.rule.description.clone(),
                lose: r.rule.lose.clone(),
                requires_admin: r.rule.requires_admin,
                close_apps: r.rule.close_apps.clone(),
                min_age_days: r.rule.min_age_days,
                user_rule: matches!(r.rule.source, RuleSource::User(_)),
                status: r.status.clone(),
                files: r.items.len(),
                size: r.size,
                hidden: r.hidden,
            })
            .collect(),
        errors: loaded.scan.errors.clone(),
        scanned_at: loaded.scan.scanned_at,
        duration_ms,
        elevated,
    }
}

/// Finds junk with every rule, in the background.
#[tauri::command]
pub fn start_junk_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    on_event: Channel<JunkEvent>,
) -> CmdResult<()> {
    let cancel = {
        let mut s = state.lock();
        if s.junk_running.is_some() {
            return Err("A junk scan is already running.".into());
        }
        let cancel = CancelToken::new();
        s.junk_running = Some(cancel.clone());
        cancel
    };
    let spawned = std::thread::Builder::new()
        .name("junk-scan".into())
        .spawn(move || {
            let started = Instant::now();
            let result = catch_unwind(AssertUnwindSafe(|| -> CmdResult<_> {
                let (data, _settings, ctx) = context()?;
                let set = rules::load(
                    Some(&cleaner_core::paths::user_rules_dir(&data)),
                    &ctx.vars,
                    &ctx.guard,
                );
                let mut scan = cleaner::scan(&set.rules, &ctx, &cancel);
                scan.errors = set.errors;
                Ok((scan, ctx.elevated))
            }));
            let state = app.state::<AppState>();
            let mut s = state.lock();
            s.junk_running = None;
            let event = match result {
                _ if cancel.is_cancelled() => JunkEvent::Cancelled,
                Ok(Ok((scan, elevated))) => {
                    let id = s.new_id();
                    let loaded = Arc::new(LoadedJunk { id, scan });
                    s.junk = Some(Arc::clone(&loaded));
                    drop(s);
                    JunkEvent::Done {
                        result: junk_view(&loaded, started.elapsed().as_millis() as u64, elevated),
                    }
                }
                Ok(Err(message)) => JunkEvent::Failed { message },
                Err(_) => JunkEvent::Failed {
                    message: "The junk scan stopped because of an internal error.".into(),
                },
            };
            let _ = on_event.send(event);
        });
    if let Err(e) = spawned {
        state.lock().junk_running = None;
        return Err(format!("Could not start the junk scan: {e}"));
    }
    Ok(())
}

#[tauri::command]
pub fn cancel_junk_scan(state: State<'_, AppState>) {
    if let Some(cancel) = &state.lock().junk_running {
        cancel.cancel();
    }
}

fn loaded_junk(state: &AppState, scan_id: u64) -> CmdResult<Arc<LoadedJunk>> {
    match &state.lock().junk {
        Some(j) if j.id == scan_id => Ok(Arc::clone(j)),
        _ => Err("These results are out of date. Please scan again.".into()),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkItemView {
    index: u32,
    path: String,
    size: u64,
    modified: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkItemsPage {
    total: usize,
    items: Vec<JunkItemView>,
}

/// The exact files a rule found (largest first), for the preview.
#[tauri::command(async)]
pub fn junk_items(
    state: State<'_, AppState>,
    scan_id: u64,
    rule_id: String,
    offset: u32,
    limit: u32,
) -> CmdResult<JunkItemsPage> {
    let junk = loaded_junk(&state, scan_id)?;
    let r = junk
        .scan
        .rules
        .iter()
        .find(|r| r.rule.id == rule_id)
        .ok_or("Unknown rule.")?;
    let start = (offset as usize).min(r.items.len());
    let end = start
        .saturating_add(limit.min(MAX_ITEMS_PAGE) as usize)
        .min(r.items.len());
    Ok(JunkItemsPage {
        total: r.items.len(),
        items: r.items[start..end]
            .iter()
            .enumerate()
            .map(|(i, item)| JunkItemView {
                index: (start + i) as u32,
                path: item.path.display().to_string(),
                size: item.size,
                modified: item.modified,
            })
            .collect(),
    })
}

// ------------------------------------------------------------ preview

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSelection {
    rule_id: String,
    /// Indexes of files the user unticked in the preview.
    #[serde(default)]
    deselected: Vec<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotReady {
    rule_id: String,
    status: RuleStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    plan_id: u64,
    count: usize,
    total_size: u64,
    /// Deleted directly (green items, unless they go to the bin too).
    delete_count: usize,
    delete_bytes: u64,
    /// Moved to the recovery bin.
    bin_count: usize,
    bin_bytes: u64,
    not_ready: Vec<NotReady>,
}

fn plan_view(plan_id: u64, plan: &CleanPlan, green_to_bin: bool) -> PlanView {
    let (mut delete_count, mut delete_bytes, mut bin_count, mut bin_bytes) = (0, 0, 0, 0);
    for item in &plan.items {
        if item.tier == Tier::Green && !green_to_bin {
            delete_count += 1;
            delete_bytes += item.size;
        } else {
            bin_count += 1;
            bin_bytes += item.size;
        }
    }
    PlanView {
        plan_id,
        count: plan.count(),
        total_size: plan.total_size,
        delete_count,
        delete_bytes,
        bin_count,
        bin_bytes,
        not_ready: plan
            .not_ready
            .iter()
            .map(|(rule_id, status)| NotReady {
                rule_id: rule_id.clone(),
                status: status.clone(),
            })
            .collect(),
    }
}

fn store_plan(
    state: &AppState,
    plan: CleanPlan,
    label: &'static str,
    green_to_bin: bool,
) -> PlanView {
    let mut s = state.lock();
    let id = s.new_id();
    let view = plan_view(id, &plan, green_to_bin);
    s.plan = Some(PreviewedPlan {
        id,
        plan,
        label,
        green_to_bin,
    });
    view
}

/// Builds the exact plan for the selected rules (minus unticked files).
#[tauri::command(async)]
pub fn clean_preview(
    state: State<'_, AppState>,
    scan_id: u64,
    selection: Vec<RuleSelection>,
) -> CmdResult<PlanView> {
    if selection.len() > MAX_SELECTION {
        return Err("Too many rules selected.".into());
    }
    let junk = loaded_junk(&state, scan_id)?;
    let mut ids = Vec::with_capacity(selection.len());
    let mut deselected = HashSet::new();
    for sel in &selection {
        let r = junk
            .scan
            .rules
            .iter()
            .find(|r| r.rule.id == sel.rule_id)
            .ok_or("Unknown rule.")?;
        for &i in &sel.deselected {
            let item = r.items.get(i as usize).ok_or("Unknown file.")?;
            deselected.insert(item.path.clone());
        }
        ids.push(sel.rule_id.clone());
    }
    let plan = cleaner::plan_rules(&junk.scan, &ids, &deselected);
    let green_to_bin = Settings::load(&data_dir()?).green_to_bin;
    Ok(store_plan(&state, plan, "Cleaner", green_to_bin))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualItem {
    path: String,
    size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualPlanView {
    plan: PlanView,
    /// The largest files in the plan.
    sample: Vec<ManualItem>,
    rejected: Vec<Rejected>,
}

/// Plans items picked in the Disk view. They are always red tier and always
/// go to the recovery bin.
#[tauri::command(async)]
pub fn clean_manual(
    state: State<'_, AppState>,
    scan_id: u64,
    node_ids: Vec<NodeId>,
) -> CmdResult<ManualPlanView> {
    if node_ids.is_empty() || node_ids.len() > MAX_MANUAL_NODES {
        return Err("Select between 1 and 10,000 items.".into());
    }
    let loaded = crate::commands::loaded(&state, scan_id)?;
    let tree = &loaded.scan.tree;
    let mut paths = Vec::with_capacity(node_ids.len());
    for id in node_ids {
        tree.get(id).ok_or("Unknown item.")?;
        paths.push(tree.path(id));
    }
    let (_, settings, ctx) = context()?;
    let (plan, rejected) = cleaner::plan_manual(&paths, &ctx);
    Ok(manual_view(
        &state,
        plan,
        rejected,
        "Disk view",
        settings.green_to_bin,
    ))
}

/// Stores a plan of hand-picked (red) items and describes it for the UI.
pub(crate) fn manual_view(
    state: &AppState,
    plan: CleanPlan,
    rejected: Vec<Rejected>,
    label: &'static str,
    green_to_bin: bool,
) -> ManualPlanView {
    let mut largest: Vec<&PlanItem> = plan.items.iter().collect();
    largest.sort_by_key(|i| std::cmp::Reverse(i.size));
    let sample = largest
        .iter()
        .take(MANUAL_SAMPLE)
        .map(|i| ManualItem {
            path: i.path.display().to_string(),
            size: i.size,
        })
        .collect();
    let view = store_plan(state, plan, label, green_to_bin);
    ManualPlanView {
        plan: view,
        sample,
        rejected,
    }
}

// ------------------------------------------------------------ execute

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CleanEvent {
    Progress { done: u64, total: u64 },
    Done { report: CleanReport },
    Failed { message: String },
}

/// Cleans exactly the previewed plan `plan_id`, in the background.
#[tauri::command]
pub fn clean_execute(
    app: AppHandle,
    state: State<'_, AppState>,
    plan_id: u64,
    on_event: Channel<CleanEvent>,
) -> CmdResult<()> {
    let (previewed, cancel) = {
        let mut s = state.lock();
        if s.cleaning.is_some() {
            return Err("A cleanup is already running.".into());
        }
        match s.plan.take() {
            Some(p) if p.id == plan_id => {
                let cancel = CancelToken::new();
                s.cleaning = Some(cancel.clone());
                (p, cancel)
            }
            other => {
                s.plan = other;
                return Err("This preview is out of date. Please review it again.".into());
            }
        }
    };
    let spawned = std::thread::Builder::new()
        .name("clean".into())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| -> CmdResult<CleanReport> {
                // A fresh look at running programs and the time, right
                // before acting.
                let (data, settings, ctx) = context()?;
                let store = RecoveryStore::open(&data, settings.bin);
                let opts = ExecOptions {
                    green_to_bin: previewed.green_to_bin,
                    label: previewed.label.to_string(),
                };
                let progress = |done: u64, total: u64| {
                    let _ = on_event.send(CleanEvent::Progress { done, total });
                };
                Ok(cleaner::execute(
                    &previewed.plan,
                    &ctx,
                    &store,
                    &data,
                    &opts,
                    &progress,
                    &cancel,
                ))
            }));
            app.state::<AppState>().lock().cleaning = None;
            let event = match result {
                Ok(Ok(report)) => CleanEvent::Done { report },
                Ok(Err(message)) => CleanEvent::Failed { message },
                Err(_) => CleanEvent::Failed {
                    message: "The cleanup stopped because of an internal error. Files not yet \
                              processed were left alone."
                        .into(),
                },
            };
            let _ = on_event.send(event);
        });
    if let Err(e) = spawned {
        state.lock().cleaning = None;
        return Err(format!("Could not start the cleanup: {e}"));
    }
    Ok(())
}

#[tauri::command]
pub fn cancel_clean(state: State<'_, AppState>) {
    if let Some(cancel) = &state.lock().cleaning {
        cancel.cancel();
    }
}

// ------------------------------------------------------------ deep clean

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DeepCleanEvent {
    Progress {
        percent: f32,
    },
    Done {
        freed_bytes: u64,
        restart_needed: bool,
    },
    Failed {
        message: String,
    },
}

/// Runs Windows' own component store cleanup
/// (`DISM /Online /Cleanup-Image /StartComponentCleanup`). Slow, needs
/// administrator rights, and never part of normal cleaning.
#[tauri::command]
pub fn deep_clean(
    app: AppHandle,
    state: State<'_, AppState>,
    on_event: Channel<DeepCleanEvent>,
) -> CmdResult<()> {
    if !cleaner_core::platform::is_elevated() {
        return Err(
            "Deep clean needs administrator rights. Restart as administrator first.".into(),
        );
    }
    {
        let mut s = state.lock();
        if s.deep_cleaning {
            return Err("Deep clean is already running.".into());
        }
        s.deep_cleaning = true;
    }
    let spawned = std::thread::Builder::new()
        .name("deep-clean".into())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                deep::run(&|percent| {
                    let _ = on_event.send(DeepCleanEvent::Progress { percent });
                })
            }));
            app.state::<AppState>().lock().deep_cleaning = false;
            let event = match result {
                Ok(Ok((freed_bytes, restart_needed))) => DeepCleanEvent::Done {
                    freed_bytes,
                    restart_needed,
                },
                Ok(Err(message)) => DeepCleanEvent::Failed { message },
                Err(_) => DeepCleanEvent::Failed {
                    message: "Deep clean stopped because of an internal error.".into(),
                },
            };
            let _ = on_event.send(event);
        });
    if let Err(e) = spawned {
        state.lock().deep_cleaning = false;
        return Err(format!("Could not start deep clean: {e}"));
    }
    Ok(())
}

/// The last percentage in DISM's progress output, such as
/// `[=====     10.0%       ]`.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_dism_percent(text: &str) -> Option<f32> {
    let end = text.rfind('%')?;
    let start = text[..end]
        .rfind(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map_or(0, |i| i + 1);
    text[start..end]
        .parse::<f32>()
        .ok()
        .filter(|p| (0.0..=100.0).contains(p))
}

#[cfg(windows)]
mod deep {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    use cleaner_core::platform;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const SUCCESS_RESTART_REQUIRED: i32 = 3010;

    pub fn run(progress: &dyn Fn(f32)) -> Result<(u64, bool), String> {
        let windows = platform::windows_dir().ok_or("Could not find the Windows folder.")?;
        // A fixed program from the Windows folder with fixed arguments;
        // nothing from the user or the UI is passed to it.
        let dism = windows.join("System32").join("Dism.exe");
        let drive = cleaner_core::recovery::drive_root(&windows).unwrap_or_else(|| windows.clone());
        let free_before = platform::volume_space(&drive).map(|s| s.free).unwrap_or(0);
        let mut child = Command::new(&dism)
            .args([
                "/Online",
                "/Cleanup-Image",
                "/StartComponentCleanup",
                "/English",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Could not start DISM: {e}"))?;
        if let Some(mut out) = child.stdout.take() {
            let mut buf = [0u8; 512];
            let mut line = String::new();
            while let Ok(n) = out.read(&mut buf) {
                if n == 0 {
                    break;
                }
                line.push_str(&String::from_utf8_lossy(&buf[..n]));
                if let Some(p) = super::parse_dism_percent(&line) {
                    progress(p);
                }
                // Keep only the current line.
                if let Some(i) = line.rfind(['\r', '\n']) {
                    line.drain(..=i);
                }
            }
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        let free_after = platform::volume_space(&drive).map(|s| s.free).unwrap_or(0);
        match status.code() {
            Some(0) => Ok((free_after.saturating_sub(free_before), false)),
            Some(SUCCESS_RESTART_REQUIRED) => Ok((free_after.saturating_sub(free_before), true)),
            Some(code) => Err(format!(
                "DISM stopped with error {code:#x}. Details are in {}.",
                windows.join(r"Logs\DISM\dism.log").display()
            )),
            None => Err("DISM was stopped.".into()),
        }
    }
}

#[cfg(not(windows))]
mod deep {
    pub fn run(_progress: &dyn Fn(f32)) -> Result<(u64, bool), String> {
        Err("Deep clean is only available on Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dism_progress_is_parsed() {
        assert_eq!(
            parse_dism_percent("[==         10.0%            ]"),
            Some(10.0)
        );
        assert_eq!(
            parse_dism_percent("[=====  25.5%  ]\r[======  30.1%  ]"),
            Some(30.1)
        );
        assert_eq!(parse_dism_percent("[100%]"), Some(100.0));
        assert_eq!(parse_dism_percent("Deployment Image Servicing"), None);
        assert_eq!(parse_dism_percent("500%"), None);
    }

    #[test]
    fn plan_view_splits_deleted_and_binned() {
        let item = |tier, size| PlanItem {
            path: PathBuf::from(format!("/x/{size}")),
            size,
            modified: 0,
            tier,
            rule: None,
            cleanup: None,
            close_apps: Vec::new(),
        };
        let plan = CleanPlan {
            items: vec![
                item(Tier::Green, 10),
                item(Tier::Yellow, 20),
                item(Tier::Red, 30),
            ],
            total_size: 60,
            not_ready: vec![("a".into(), RuleStatus::NeedsAdmin)],
        };
        let v = plan_view(7, &plan, false);
        assert_eq!((v.delete_count, v.delete_bytes), (1, 10));
        assert_eq!((v.bin_count, v.bin_bytes), (2, 50));
        assert_eq!(v.not_ready.len(), 1);
        let v = plan_view(7, &plan, true);
        assert_eq!((v.delete_count, v.bin_count), (0, 3));
    }
}
