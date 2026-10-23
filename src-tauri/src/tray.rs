use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::updates::Updates;
use crate::window;

const TRAY: &str = "main";
const OPEN: &str = "open";
const UPDATE: &str = "update";
const QUIT: &str = "quit";

/// The tray menu; "Restart to update" appears once an update has been downloaded.
fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::with_items(app, &[&MenuItem::with_id(app, OPEN, "Open Hearth", true, None::<&str>)?])?;
    if let Some(version) = app.state::<Updates>().ready_version() {
        menu.append(&MenuItem::with_id(
            app,
            UPDATE,
            format!("Restart to update Hearth ({version})"),
            true,
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, QUIT, "Quit Hearth", true, None::<&str>)?)?;
    Ok(menu)
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY)
        .tooltip("Hearth")
        .menu(&menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            OPEN => window::show(app),
            UPDATE => {
                crate::updates::install(app, true);
            }
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

/// Rebuilds the menu after the update state changed (a no-op before the tray exists).
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY) else { return };
    if let Ok(menu) = menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
}
