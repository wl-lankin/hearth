// No console window next to Hearth in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    hearth_app_lib::run()
}
