mod commands;
mod startup;
mod tray;
mod window;

use std::sync::Mutex;
use std::time::Instant;

use hearth_core::ipc::{self, CoreState, ServerHandle};
use hearth_core::{settings, CoreError, HearthPaths};
use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
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
    let state = CoreState { paths, started: Instant::now() };

    tauri::Builder::default()
        // Registered first: a second Hearth process only brings the first one's window up.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| window::show(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![startup::HIDDEN_FLAG])))
        .plugin(tauri_plugin_dialog::init())
        .manage(state.clone())
        .manage(Server::default())
        .invoke_handler(tauri::generate_handler![commands::core_status, commands::settings_get, commands::settings_set])
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
            match tauri::async_runtime::block_on(ipc::serve(&state.paths, ipc::router(state.clone()))) {
                Ok(server) => *app.state::<Server>().0.lock().unwrap() = Some(server),
                Err(e) => {
                    let message = match e {
                        CoreError::AlreadyRunning => "Hearth is already running.".to_string(),
                        other => format!("Hearth could not start its core: {other}"),
                    };
                    app.dialog().message(message).title("Hearth").show(move |_| handle.exit(0));
                    return Ok(());
                }
            }
            tray::create(&handle)?;
            let args: Vec<String> = std::env::args().collect();
            if !startup::start_hidden(&settings::load(&state.paths), &args) {
                window::show(&handle);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Hearth could not start");
}

/// Stops the core server, then exits.
pub(crate) fn quit(app: &AppHandle) {
    let server = app.state::<Server>().0.lock().unwrap().take();
    if let Some(server) = server {
        tauri::async_runtime::block_on(server.shutdown());
    }
    app.exit(0);
}
