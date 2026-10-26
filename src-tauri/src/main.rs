// No console window next to Hearth in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Started as the Windows .test helper (its install, removal or the service itself).
    if let Some(code) = hearth_helper::run_from_args() {
        std::process::exit(code);
    }
    hearth_app_lib::run()
}
