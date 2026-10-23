//! Keeps Hearth current: checks the GitHub releases 10 s after start and then every 6 hours,
//! downloads a new version in the background (verified against Hearth's updater key by the plugin)
//! and installs it when Hearth quits, or at once on "Restart to update".

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

const FIRST_CHECK: Duration = Duration::from_secs(10);
const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum UpdateState {
    Idle,
    Disabled,
    Checking,
    Current,
    Downloading { version: String, progress: f64 },
    Ready { version: String },
    Installing { version: String },
    Error { message: String },
}

/// A check is pointless while one runs, a download is waiting to be installed, or updates are off.
pub fn next_check_allowed(state: &UpdateState) -> bool {
    matches!(state, UpdateState::Idle | UpdateState::Current | UpdateState::Error { .. })
}

/// The updater's state plus the downloaded package that is waiting for installation.
pub struct Updates {
    state: Mutex<UpdateState>,
    pending: Mutex<Option<(Update, Vec<u8>)>>,
}

impl Updates {
    pub fn new(enabled: bool) -> Self {
        let state = if enabled { UpdateState::Idle } else { UpdateState::Disabled };
        Updates { state: Mutex::new(state), pending: Mutex::new(None) }
    }

    pub fn state(&self) -> UpdateState {
        self.state.lock().unwrap().clone()
    }

    pub fn ready_version(&self) -> Option<String> {
        match self.state() {
            UpdateState::Ready { version } => Some(version),
            _ => None,
        }
    }
}

fn set(app: &AppHandle, next: UpdateState) {
    *app.state::<Updates>().state.lock().unwrap() = next.clone();
    let _ = app.emit("update-state", &next);
    crate::tray::refresh(app);
}

/// Starts the background schedule (does nothing when updates are disabled).
pub fn start(app: &AppHandle) {
    if app.state::<Updates>().state() == UpdateState::Disabled {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            check(&app).await;
            tokio::time::sleep(RECHECK).await;
        }
    });
}

/// Checks for a new version and downloads it in the background.
pub async fn check(app: &AppHandle) {
    if !next_check_allowed(&app.state::<Updates>().state()) {
        return;
    }
    set(app, UpdateState::Checking);
    let update = match app.updater().map_err(|e| e.to_string()) {
        Ok(updater) => updater.check().await.map_err(|e| e.to_string()),
        Err(e) => Err(e),
    };
    let update = match update {
        Ok(Some(update)) => update,
        Ok(None) => return set(app, UpdateState::Current),
        Err(message) => return set(app, UpdateState::Error { message }),
    };
    let version = update.version.clone();
    set(app, UpdateState::Downloading { version: version.clone(), progress: 0.0 });
    let mut received = 0usize;
    let mut last_percent = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                received += chunk;
                let Some(total) = total.filter(|t| *t > 0) else { return };
                let percent = (received as u64 * 100 / total).min(100);
                // One event per percent, not per network chunk.
                if percent > last_percent {
                    last_percent = percent;
                    let progress = percent as f64 / 100.0;
                    set(app, UpdateState::Downloading { version: version.clone(), progress });
                }
            },
            || {},
        )
        .await;
    match bytes {
        Ok(bytes) => {
            *app.state::<Updates>().pending.lock().unwrap() = Some((update, bytes));
            set(app, UpdateState::Ready { version });
        }
        Err(e) => set(app, UpdateState::Error { message: e.to_string() }),
    }
}

/// Installs a downloaded update: stops the core first, then hands over to the installer. With
/// `restart`, Hearth starts again afterwards (on Windows the installer relaunches it). Returns false
/// when no update is waiting, so the caller can quit normally.
pub fn install(app: &AppHandle, restart: bool) -> bool {
    let Some((update, bytes)) = app.state::<Updates>().pending.lock().unwrap().take() else { return false };
    set(app, UpdateState::Installing { version: update.version.clone() });
    crate::stop_core(app);
    match update.install(bytes) {
        Ok(()) if restart => app.restart(),
        Ok(()) => app.exit(0),
        Err(e) => {
            set(app, UpdateState::Error { message: e.to_string() });
            // Hearth keeps running on the current version, so its core must come back.
            if let Err(e) = crate::start_core(app) {
                eprintln!("Hearth could not restart its core: {e}");
            }
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_new_check_while_busy_or_ready() {
        let v = || "0.2.0".to_string();
        assert!(next_check_allowed(&UpdateState::Idle));
        assert!(next_check_allowed(&UpdateState::Current));
        assert!(next_check_allowed(&UpdateState::Error { message: "offline".into() }));
        assert!(!next_check_allowed(&UpdateState::Disabled));
        assert!(!next_check_allowed(&UpdateState::Checking));
        assert!(!next_check_allowed(&UpdateState::Downloading { version: v(), progress: 0.5 }));
        assert!(!next_check_allowed(&UpdateState::Ready { version: v() }));
        assert!(!next_check_allowed(&UpdateState::Installing { version: v() }));
    }

    #[test]
    fn state_serializes_with_phase_tag() {
        let state = UpdateState::Downloading { version: "0.2.0".into(), progress: 0.5 };
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            serde_json::json!({"phase": "downloading", "version": "0.2.0", "progress": 0.5})
        );
        assert_eq!(serde_json::to_value(UpdateState::Current).unwrap(), serde_json::json!({"phase": "current"}));
    }
}
