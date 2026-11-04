use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::{panel, window};

const TRAY: &str = "main";

/// The tray icon: a left click shows or hides the main window, a right click opens the panel with
/// the quick actions (it replaces the system menu, which cannot take Hearth's look).
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY).tooltip("Hearth").on_tray_icon_event(|tray, event| {
        if let TrayIconEvent::Click { button, button_state: MouseButtonState::Up, rect, .. } = event {
            match button {
                MouseButton::Left => window::toggle(tray.app_handle()),
                MouseButton::Right => panel::toggle(tray.app_handle(), rect),
                _ => {}
            }
        }
    });
    // macOS tints a one-color template image to match the menu bar; Windows shows the color logo.
    if cfg!(target_os = "macos") {
        builder = builder.icon(Image::from_bytes(include_bytes!("../icons/tray-template.png"))?).icon_as_template(true);
    } else {
        builder = builder.icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?);
    }
    builder.build(app)?;
    Ok(())
}
