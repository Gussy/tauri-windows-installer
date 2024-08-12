// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Handle uninstall on Windows
    #[cfg(target_os = "windows")]
    twi_uninstall::handle_uninstall(&"Demo App", &"com.gussy.demo-app"); // TODO: Get this at build time

    demo_app_lib::run()
}
