//! Privacy tab: Windows traces. Counts and clears; never reads contents.

use std::collections::HashSet;

use cleaner_core::privacy::{self, Locations, Trace, TraceInfo, TraceResult};

use crate::cleaning::{context, data_dir};

type CmdResult<T> = Result<T, String>;

#[tauri::command(async)]
pub fn privacy_survey() -> Vec<TraceInfo> {
    privacy::survey(&Locations::system())
}

#[tauri::command(async)]
pub fn privacy_clear(traces: Vec<Trace>) -> CmdResult<Vec<TraceResult>> {
    let unique: HashSet<Trace> = traces.iter().copied().collect();
    if unique.is_empty() || unique.len() != traces.len() {
        return Err("Pick each item to clear once.".into());
    }
    let (_, _, ctx) = context()?;
    Ok(privacy::clear(
        &traces,
        &Locations::system(),
        &ctx.guard,
        &data_dir()?,
    ))
}
