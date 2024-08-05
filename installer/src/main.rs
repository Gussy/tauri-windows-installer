mod application;
mod bundle;
mod webview2;
mod windows;

use crate::application::Application;
use crate::bundle::Bundle;
use crate::webview2::Webview2;
use crate::windows::{get_local_app_data, string_to_u16};

use ::windows::core::PCWSTR;
use ::windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
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

    // Check if there is enough space to install the application
    let required_space = app.data.len() as u64;
    println!("Required disk space: {}", required_space);

    let mut free_space: u64 = 0;
    let root_pcwstr = string_to_u16(root_path_str);
    let root_pcwstr: PCWSTR = PCWSTR(root_pcwstr.as_ptr());
    if let Ok(()) = unsafe { GetDiskFreeSpaceExW(root_pcwstr, None, None, Some(&mut free_space)) } {
        if free_space < required_space {
            panic!(
                "{} requires at least {} disk space to be installed. There is only {} available.",
                app.id,
                format_bytes(required_space),
                format_bytes(free_space)
            );
        }
    }

    println!(
        "There is {} free space available at destination, this package requires {}.",
        format_bytes(free_space),
        format_bytes(required_space)
    );

    // TODO:
    // - Extract and copy the bundled installer
    // - Set the windows registry keys
    // - Handle uninstall with command line flag
}

fn format_bytes(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < units.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.2} {}", size, units[unit_index])
}
