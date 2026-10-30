// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> tauri::Result<()> {
    grain_to_pixel_desktop_lib::run()
}
