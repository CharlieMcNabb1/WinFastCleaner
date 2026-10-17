//! Updates tab: app updates through winget. The frontend picks apps by
//! winget id, and only ids from the latest listing are accepted.

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use cleaner_core::health::{self, UpdatesSummary};
use cleaner_core::progress::CancelToken;
use cleaner_core::winget::{self, Upgrade, UpgradeResult};

use crate::cleaning::data_dir;
use crate::state::AppState;

type CmdResult<T> = Result<T, String>;

const MAX_PICKS: usize = 500;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatesStatus {
    installed: bool,
    /// How to get winget, when it is missing.
    missing_message: Option<&'static str>,
    last: Option<UpdatesSummary>,
}

#[tauri::command(async)]
pub fn updates_status() -> UpdatesStatus {
    let installed = winget::find().is_some();
    UpdatesStatus {
        installed,
        missing_message: (!installed).then_some(winget::MISSING),
        last: data_dir().ok().and_then(|d| health::load_updates(&d)),
    }
}

fn winget_path() -> CmdResult<std::path::PathBuf> {
    winget::find().ok_or_else(|| winget::MISSING.to_string())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Asks winget which apps have updates (can take a minute). Cancellable.
#[tauri::command(async)]
pub fn updates_list(state: State<'_, AppState>) -> CmdResult<Vec<Upgrade>> {
    let winget = winget_path()?;
    let cancel = {
        let mut s = state.lock();
        if s.updates_busy.is_some() {
            return Err("winget is already running.".into());
        }
        let c = CancelToken::new();
        s.updates_busy = Some(c.clone());
        c
    };
    let result = catch_unwind(AssertUnwindSafe(|| winget::list(&winget, &cancel)))
        .unwrap_or_else(|_| Err("The update check stopped because of an internal error.".into()));
    let mut s = state.lock();
    s.updates_busy = None;
    let rows = result?;
    if let Ok(d) = data_dir() {
        let _ = health::save_updates(
            &d,
            &UpdatesSummary {
                checked_at: now(),
                count: rows.len() as u32,
            },
        );
    }
    s.upgrades = Some(rows.clone());
    Ok(rows)
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UpdateEvent {
    Started {
        id: String,
        index: usize,
        total: usize,
    },
    Finished {
        result: UpgradeResult,
    },
    Done {
        cancelled: bool,
    },
}

/// Updates the picked apps one at a time. Cancel stops before the next app
/// (an installer is never stopped halfway).
#[tauri::command]
pub fn updates_apply(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    on_event: Channel<UpdateEvent>,
) -> CmdResult<()> {
    let winget = winget_path()?;
    let unique: HashSet<&String> = ids.iter().collect();
    if ids.is_empty() || ids.len() > MAX_PICKS || unique.len() != ids.len() {
        return Err("Pick each app to update once.".into());
    }
    let (picked, cancel) = {
        let mut s = state.lock();
        if s.updates_busy.is_some() {
            return Err("winget is already running.".into());
        }
        let listed = s.upgrades.as_ref().ok_or("Check for updates first.")?;
        let mut picked = Vec::with_capacity(ids.len());
        for id in &ids {
            let up = listed
                .iter()
                .find(|u| &u.id == id)
                .ok_or("That app isn't in the latest list. Check for updates again.")?;
            if !up.can_update || !winget::valid_id(&up.id) {
                return Err(format!("{} can't be updated from here.", up.name));
            }
            picked.push(up.clone());
        }
        let c = CancelToken::new();
        s.updates_busy = Some(c.clone());
        (picked, c)
    };
    let spawned = std::thread::Builder::new()
        .name("winget-upgrade".into())
        .spawn(move || {
            let total = picked.len();
            let mut cancelled = false;
            for (index, up) in picked.iter().enumerate() {
                if cancel.is_cancelled() {
                    cancelled = true;
                    break;
                }
                let _ = on_event.send(UpdateEvent::Started {
                    id: up.id.clone(),
                    index,
                    total,
                });
                let result = catch_unwind(AssertUnwindSafe(|| winget::upgrade(&winget, up)))
                    .unwrap_or_else(|_| UpgradeResult {
                        id: up.id.clone(),
                        ok: false,
                        message: "Stopped because of an internal error.".into(),
                    });
                let _ = on_event.send(UpdateEvent::Finished { result });
            }
            let state = app.state::<AppState>();
            let mut s = state.lock();
            s.updates_busy = None;
            // The list is out of date now; the UI checks again.
            s.upgrades = None;
            drop(s);
            let _ = on_event.send(UpdateEvent::Done { cancelled });
        });
    if let Err(e) = spawned {
        state.lock().updates_busy = None;
        return Err(format!("Could not start the updates: {e}"));
    }
    Ok(())
}

#[tauri::command]
pub fn updates_cancel(state: State<'_, AppState>) {
    if let Some(c) = &state.lock().updates_busy {
        c.cancel();
    }
}
