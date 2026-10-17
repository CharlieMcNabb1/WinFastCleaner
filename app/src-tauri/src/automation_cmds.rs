//! Scheduled cleaning and the tray monitor. Each change updates both the
//! settings and Windows (the Task Scheduler task, the tray's sign-in entry),
//! and Windows first: if that fails, the settings stay as they were.

use serde::Serialize;

use cleaner_core::integration::{self, TRAY_PROGRAM};
use cleaner_core::rules::{self, Tier};
use cleaner_core::schedule::{self, LastRun, Schedule, TASK_PROGRAM};
use cleaner_core::settings::Settings;

use crate::cleaning::{context, data_dir};

type CmdResult<T> = Result<T, String>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleChoice {
    id: String,
    name: String,
    group: String,
    tier: Tier,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleView {
    schedule: Schedule,
    /// Whether the Task Scheduler task exists (None when it can't be read).
    task_exists: Option<bool>,
    /// The scheduled cleaner program ships next to the app.
    program_found: bool,
    last: Option<LastRun>,
    /// The rules a schedule can use (safe and optional ones).
    rules: Vec<RuleChoice>,
}

fn schedule_view(s: Schedule) -> CmdResult<ScheduleView> {
    let (data, _, ctx) = context()?;
    let set = rules::load(
        Some(&cleaner_core::paths::user_rules_dir(&data)),
        &ctx.vars,
        &ctx.guard,
    );
    #[cfg(windows)]
    let task_exists = schedule::task_state().ok().map(|t| t.is_some());
    #[cfg(not(windows))]
    let task_exists = None;
    Ok(ScheduleView {
        schedule: s,
        task_exists,
        program_found: integration::sibling(TASK_PROGRAM).is_some(),
        last: schedule::load_last(&data),
        rules: set
            .rules
            .into_iter()
            .filter(|r| r.tier != Tier::Red)
            .map(|r| RuleChoice {
                id: r.id,
                name: r.name,
                group: r.group,
                tier: r.tier,
            })
            .collect(),
    })
}

#[tauri::command(async)]
pub fn schedule_status() -> CmdResult<ScheduleView> {
    schedule_view(Settings::load(&data_dir()?).schedule)
}

/// Saves the schedule and creates, updates, or removes the task to match.
#[tauri::command(async)]
pub fn schedule_save(schedule: Schedule) -> CmdResult<ScheduleView> {
    schedule.validate()?;
    let data = data_dir()?;
    let mut settings = Settings::load(&data);
    apply_schedule(&schedule)?;
    settings.schedule = schedule.clone();
    settings.save(&data).map_err(|e| e.to_string())?;
    schedule_view(schedule)
}

#[cfg(windows)]
fn apply_schedule(s: &Schedule) -> CmdResult<()> {
    let program = integration::sibling(TASK_PROGRAM).unwrap_or_default();
    schedule::apply(s, &program)
}

#[cfg(not(windows))]
fn apply_schedule(s: &Schedule) -> CmdResult<()> {
    if s.enabled {
        Err("Scheduled cleaning only works on Windows.".into())
    } else {
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayView {
    enabled: bool,
    running: bool,
    program_found: bool,
}

fn tray_view(enabled: bool) -> TrayView {
    TrayView {
        enabled,
        running: integration::tray_running(),
        program_found: integration::sibling(TRAY_PROGRAM).is_some(),
    }
}

#[tauri::command(async)]
pub fn tray_status() -> CmdResult<TrayView> {
    Ok(tray_view(Settings::load(&data_dir()?).tray))
}

/// Turns the tray monitor on (adds its sign-in entry and starts it now) or
/// off (removes the entry and closes it).
#[tauri::command(async)]
pub fn tray_set(enabled: bool) -> CmdResult<TrayView> {
    let data = data_dir()?;
    let mut settings = Settings::load(&data);
    if enabled {
        let program = integration::sibling(TRAY_PROGRAM).ok_or_else(|| {
            format!("The tray monitor ({TRAY_PROGRAM}) is missing next to the app. Reinstall WinFastCleaner.")
        })?;
        integration::set_tray_autostart(Some(&program))?;
        settings.tray = true;
        settings.save(&data).map_err(|e| e.to_string())?;
        if !integration::tray_running() {
            std::process::Command::new(&program)
                .spawn()
                .map_err(|e| format!("The tray monitor could not be started: {e}"))?;
        }
    } else {
        settings.tray = false;
        settings.save(&data).map_err(|e| e.to_string())?;
        integration::set_tray_autostart(None)?;
        integration::close_tray();
    }
    Ok(tray_view(enabled))
}
