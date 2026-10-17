//! Settings commands: the cleaner options, recovery bin limits, exclusions,
//! health thresholds, the theme, and the custom rules folder. (Scheduled
//! cleaning and the tray monitor are in `automation_cmds`.)

use serde::Serialize;
use tauri::State;

use cleaner_core::rules::{self, RuleError};
use cleaner_core::safety::Guard;
use cleaner_core::settings::{Exclusion, ExclusionKind, Settings};
use cleaner_core::tree::NodeId;

use crate::cleaning::{context, data_dir};
use crate::state::AppState;

type CmdResult<T> = Result<T, String>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    settings: Settings,
    data_dir: String,
    rules_dir: String,
    user_rules: usize,
    rule_errors: Vec<RuleError>,
}

fn view(settings: Settings) -> CmdResult<SettingsView> {
    let (data, _, ctx) = context()?;
    let rules_dir = cleaner_core::paths::user_rules_dir(&data);
    let set = rules::load_dir(&rules_dir, &ctx.vars, &ctx.guard);
    Ok(SettingsView {
        settings,
        data_dir: data.display().to_string(),
        rules_dir: rules_dir.display().to_string(),
        user_rules: set.rules.len(),
        rule_errors: set.errors,
    })
}

#[tauri::command(async)]
pub fn get_settings() -> CmdResult<SettingsView> {
    view(Settings::load(&data_dir()?))
}

/// Validates and saves settings. Invalid values are refused, not adjusted.
/// The schedule and the tray monitor are kept as they are: they change
/// only through their own commands, which also update Windows (the task
/// and the sign-in entry), so the two can't drift apart.
#[tauri::command(async)]
pub fn save_settings(mut settings: Settings) -> CmdResult<SettingsView> {
    let data = data_dir()?;
    let current = Settings::load(&data);
    settings.schedule = current.schedule;
    settings.tray = current.tray;
    settings.save(&data).map_err(|e| e.to_string())?;
    view(settings)
}

/// Adds a Disk view item to the exclusions ("never touch") list.
#[tauri::command(async)]
pub fn exclude_node(state: State<'_, AppState>, scan_id: u64, node_id: NodeId) -> CmdResult<()> {
    let loaded = crate::commands::loaded(&state, scan_id)?;
    let tree = &loaded.scan.tree;
    let node = tree.get(node_id).ok_or("Unknown item.")?;
    let path = tree.path(node_id);
    let data = data_dir()?;
    let mut settings = Settings::load(&data);
    let already = settings.exclusions.iter().any(|e| {
        e.kind != ExclusionKind::Pattern && Guard::same_path(std::path::Path::new(&e.value), &path)
    });
    if already {
        return Ok(());
    }
    settings.exclusions.push(Exclusion {
        kind: if node.is_dir() {
            ExclusionKind::Folder
        } else {
            ExclusionKind::File
        },
        value: path.display().to_string(),
    });
    settings.save(&data).map_err(|e| e.to_string())
}

/// Opens the custom rules folder in File Explorer (creating it if needed).
#[tauri::command(async)]
pub fn open_rules_folder() -> CmdResult<()> {
    let dir = cleaner_core::paths::user_rules_dir(&data_dir()?);
    cleaner_core::paths::ensure_private_dir(&dir).map_err(|e| e.to_string())?;
    crate::commands::open_folder(&dir)
}
