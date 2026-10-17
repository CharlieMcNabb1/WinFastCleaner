//! Startup tab: what starts at sign-in, turned off and on by ID. The
//! frontend can only switch an item that is in a fresh listing; it never
//! names a registry key, file, or task itself.

use serde::Serialize;

use cleaner_core::platform;
use cleaner_core::startup::{self, Locations, ServiceItem, StartupItem};

type CmdResult<T> = Result<T, String>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupView {
    items: Vec<StartupItem>,
    errors: Vec<String>,
    services: Vec<ServiceItem>,
    services_error: Option<String>,
    elevated: bool,
}

#[tauri::command(async)]
pub fn startup_list() -> StartupView {
    let list = startup::list(&Locations::system());
    let (services, services_error) = match startup::services() {
        Ok(s) => (s, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    StartupView {
        items: list.items,
        errors: list.errors,
        services,
        services_error,
        elevated: platform::is_elevated(),
    }
}

/// Turns one item on or off and returns it as it is now.
#[tauri::command(async)]
pub fn startup_set_enabled(id: String, enabled: bool) -> CmdResult<StartupItem> {
    if id.is_empty() || id.len() > 20_000 {
        return Err("Unknown startup item.".into());
    }
    let loc = Locations::system();
    let item = startup::list(&loc)
        .items
        .into_iter()
        .find(|i| i.id == id)
        .ok_or("This item is no longer in the startup list. Refresh and try again.")?;
    if item.enabled != enabled {
        startup::set_enabled(&loc, &item, enabled)?;
    }
    startup::list(&loc)
        .items
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| "The item disappeared from the startup list.".into())
}
