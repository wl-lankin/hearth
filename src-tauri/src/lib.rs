mod commands;
mod startup;
mod tray;
mod updates;
mod window;

use std::sync::Mutex;

use hearth_core::ipc::{self, CoreState, ServerHandle};
use hearth_core::{paths, settings, CoreError, HearthPaths};
use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;

/// The running IPC server, taken on quit so its endpoint is released before the process exits.
#[derive(Default)]
struct Server(Mutex<Option<ServerHandle>>);

pub fn run() {
    let paths = match HearthPaths::from_env().and_then(|p| p.ensure().map(|_| p)) {
        Ok(paths) => paths,
        Err(e) => {
            eprintln!("Hearth could not prepare its home folder: {e}");
            std::process::exit(1);
        }
    };
    let first_run = !settings::exists(&paths);
    let default_home = startup::use_single_instance(std::env::var_os(paths::HOME_VAR).as_deref());
    // Only an installed copy registers itself at login and updates itself: never a dev build or a
    // custom (test) home.
    let installed = !cfg!(debug_assertions) && default_home;
    let state = CoreState::new(paths);

    let mut builder = tauri::Builder::default();
    if default_home {
        // Registered first: a second Hearth process only brings the first one's window up.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| window::show(app)));
    }
    builder
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name(startup::LOGIN_ENTRY_NAME)
                .arg(startup::HIDDEN_FLAG)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state.clone())
        .manage(Server::default())
        .manage(updates::Updates::new(installed))
        .invoke_handler(tauri::generate_handler![
            commands::core_status,
            commands::settings_get,
            commands::settings_set,
            commands::update_state,
            commands::update_check,
            commands::update_install_now,
            commands::core_call,
        ])
        .on_window_event(|window, event| {
            // Closing the window keeps Hearth running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == window::MAIN {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            if let Err(e) = start_core(&handle) {
                let message = match e {
                    CoreError::AlreadyRunning => "Hearth is already running.".to_string(),
                    other => format!("Hearth could not start its core: {other}"),
                };
                app.dialog().message(message).title("Hearth").show(move |_| handle.exit(0));
                return Ok(());
            }
            tray::create(&handle)?;
            let current = settings::load(&state.paths);
            if let Some(next) = startup::first_run_settings(first_run, installed, &current) {
                // A failure leaves the setting off; the next start tries again (no settings file yet).
                match handle.autolaunch().enable() {
                    Ok(()) => {
                        if let Err(e) = settings::save(&state.paths, &next) {
                            eprintln!("Hearth could not save its settings: {e}");
                        }
                    }
                    Err(e) => eprintln!("Hearth could not register Start at login: {e}"),
                }
            } else if startup::reassert_login_entry(installed, &current) {
                if let Err(e) = handle.autolaunch().enable() {
                    eprintln!("Hearth could not renew Start at login: {e}");
                }
            }
            let args: Vec<String> = std::env::args().collect();
            if !startup::start_hidden(&current, &args) {
                window::show(&handle);
            }
            updates::start(&handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Hearth could not start");
}

/// Starts the IPC server for this Hearth home and keeps its handle.
pub(crate) fn start_core(app: &AppHandle) -> Result<(), CoreError> {
    let state = app.state::<CoreState>().inner().clone();
    let server = tauri::async_runtime::block_on(ipc::serve(&state.paths, ipc::router(state.clone())))?;
    *app.state::<Server>().0.lock().unwrap() = Some(server);
    Ok(())
}

/// Stops the IPC server and releases its endpoint.
pub(crate) fn stop_core(app: &AppHandle) {
    let server = app.state::<Server>().0.lock().unwrap().take();
    if let Some(server) = server {
        tauri::async_runtime::block_on(server.shutdown());
    }
}

/// Quits Hearth; a downloaded update is installed on the way out.
pub(crate) fn quit(app: &AppHandle) {
    if updates::install(app, false) {
        return;
    }
    stop_core(app);
    app.exit(0);
}
