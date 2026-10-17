//! Health check for the Home tab. Read-only.

use serde::Serialize;

use cleaner_core::health::{self, Check, Status};

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
    let checks = health::evaluate(&health::gather(&data, &settings), &settings.health);
    Ok(HealthView {
        overall: health::overall(&checks),
        checks,
    })
}
