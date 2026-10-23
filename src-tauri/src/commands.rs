use hearth_core::ipc::{self, CoreState};
use hearth_core::{settings, Settings};
use serde_json::Value;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::updates::{self, UpdateState, Updates};

#[tauri::command]
pub fn core_status(state: State<'_, CoreState>) -> Value {
    ipc::status_of(&state)
}

#[tauri::command]
pub fn update_state(updates: State<'_, Updates>) -> UpdateState {
    updates.state()
}

#[tauri::command]
pub async fn update_check(app: AppHandle) {
    updates::check(&app).await;
}

#[tauri::command]
pub fn update_install_now(app: AppHandle) {
    updates::install(&app, true);
}

#[tauri::command]
pub fn settings_get(state: State<'_, CoreState>) -> Settings {
    settings::load(&state.paths)
}

/// Applies "Start at login" first, so a refused change is not saved as if it had worked.
#[tauri::command]
pub fn settings_set(app: AppHandle, state: State<'_, CoreState>, settings: Settings) -> Result<Settings, String> {
    let autolaunch = app.autolaunch();
    let enabled = autolaunch.is_enabled().unwrap_or(false);
    if settings.launch_at_login != enabled {
        let result = if settings.launch_at_login { autolaunch.enable() } else { autolaunch.disable() };
        result.map_err(|e| format!("Could not change Start at login: {e}"))?;
    }
    settings::save(&state.paths, &settings).map_err(|e| format!("Could not save the settings: {e}"))?;
    Ok(settings)
}
