use tauri::{AppHandle, Manager};

pub const MAIN: &str = "main";

pub fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn toggle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN) else { return };
    if window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false) {
        let _ = window.hide();
    } else {
        show(app);
    }
}
