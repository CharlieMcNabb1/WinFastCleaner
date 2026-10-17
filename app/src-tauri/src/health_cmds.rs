//! Health check for the Home tab. Read-only.

use serde::Serialize;

use cleaner_core::health::{self, Check, Inputs, Status};
use cleaner_core::platform;
use cleaner_core::recovery::RecoveryStore;
use cleaner_core::startup::{self, Locations};
use cleaner_core::winget;

use crate::cleaning::context;

type CmdResult<T> = Result<T, String>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthView {
    checks: Vec<Check>,
    overall: Status,
}

#[tauri::command(async)]
pub fn health_check() -> CmdResult<HealthView> {
    let (data, settings, _ctx) = context()?;
    let startup = startup::list(&Locations::system());
    let inputs = Inputs {
        drives: platform::drives(),
        junk: health::load_junk(&data),
        startup_enabled: Some(startup.enabled_count()),
        disks: health::read_disks(),
        winget_installed: winget::find().is_some(),
        updates: health::load_updates(&data),
        bin: RecoveryStore::open(&data, settings.bin).usage().ok(),
    };
    let checks = health::evaluate(&inputs, &settings.health);
    Ok(HealthView {
        overall: health::overall(&checks),
        checks,
    })
}
