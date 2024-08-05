mod application;
mod bundle;
mod webview2;
mod windows;

use crate::application::Application;
use crate::bundle::Bundle;
use crate::webview2::Webview2;
use crate::windows::get_local_app_data;

use std::fs;
use std::path::Path;

fn main() {
    // Handle bundled application
    let app = Application::load();
    println!("Application: {}", app.name);

    // Handle bundled WebView2 runtime
    let webview2 = Webview2::load();
    println!("Webview2 bundled: {}", webview2.bundled);

    // Check if WebView2 runtime is installed
    let webview2_installed = webview2.installed;
    println!("Webview2 installed: {}", webview2_installed);
    if !webview2_installed {
        println!("Installing webview2 runtime...");
        webview2
            .install(false)
            .expect("Failed to install webview2 runtime");
    }

    // Determine the installation directory
    println!("Determining install directory...");
    let appdata = get_local_app_data().expect("Failed to get local app data path");
    let root_path = Path::new(&appdata).join(&app.id);
    if !root_path.exists() {
        fs::create_dir_all(&root_path).expect("Failed to create installation directory");
    }
    let root_path_str = root_path.to_str().unwrap();
    println!("Installation Directory: {:?}", root_path_str);

    // TODO:
    // - Extract and copy the bundled installer
    // - Set the windows registry keys
    // - Handle uninstall with command line flag
}
