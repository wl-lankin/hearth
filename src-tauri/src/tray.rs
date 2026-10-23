use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::image::Image;
use tauri::AppHandle;

use crate::window;

const OPEN: &str = "open";
const QUIT: &str = "quit";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, OPEN, "Open Hearth", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, QUIT, "Quit Hearth", true, None::<&str>)?,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Hearth")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            OPEN => window::show(app),
            QUIT => crate::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                window::toggle(tray.app_handle());
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
