// No console window next to Stoke in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Started as the Windows .test helper (its install, removal or the service itself).
    if let Some(code) = stoke_helper::run_from_args() {
        std::process::exit(code);
    }
    stoke_app_lib::run()
}
