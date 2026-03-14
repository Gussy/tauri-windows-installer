// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Handle uninstall on Windows
    #[cfg(target_os = "windows")]
    if twi_uninstall::handle_uninstall() {
        std::process::exit(0);
    }

    demo_app_lib::run()
}
